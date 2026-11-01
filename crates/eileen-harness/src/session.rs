//! What a Claude session spawns with (BUILD.md sections 2.3, 2.5 and 2.12): the command line,
//! the per-session settings file, the generated MCP config and the environment, built from a
//! [`SessionSpec`] and the [`HarnessConfig`]. Pure: [`session_files`] reads no file, no clock and
//! no environment of its own; the runtime passes the user's environment in as a [`HostEnv`],
//! and the driver writes the two files and spawns through `eileen_supervisor`.
//!
//! - Every session gets `--setting-sources ""` with its own settings file and
//!   `--strict-mcp-config` with its own MCP config, both under `<data_dir>/run/<session>/`, so
//!   nothing of the user's own Claude settings, MCP servers or connectors loads.
//! - The brain's prompt replaces the CLI's default one (`--system-prompt`, section 2.3); a
//!   worker's is appended to it (`--append-system-prompt`, section 2.5).
//! - `--tools` names the kind's built-in tools (none for the brain), `--disallowedTools` names
//!   [`SUBAGENT_TOOLS`], and neither `--tools` nor `--allowedTools` ever lists them.
//! - Permissions run under `--permission-mode dontAsk` with `--permission-prompt-tool
//!   mcp__eileen__approve`, the route WP3's approval contract test picked.
//! - The hook and shim tokens travel only in the environment: the settings file names them as
//!   `$EILEEN_HOOK_TOKEN` through `allowedEnvVars`, and the MCP config as
//!   `${EILEEN_SHIM_TOKEN}`, so neither file holds a secret.
//! - Vendor memory and file checkpointing are off in every session (D77), through the
//!   environment and the settings file both.
//! - No path the harness generates or passes lies under a `.claude` folder, and every path it
//!   passes is absolute, so nothing resolves against the worker's working folder, a repository
//!   Eileen does not trust, or through a search of `PATH`.
//! - A third-party MCP server starts only when the user approved it for the session's job kind,
//!   never for the brain, and never with a `$` in its command or arguments: the CLI expands
//!   `${VAR}` there from its own environment, which holds both tokens.

mod env;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use eileen_contracts::core_io::{Rung, ShimRole};
use eileen_contracts::harness::{Effort, EnvPolicy, SessionSpec};
use eileen_contracts::jobs::{JobKind, Toolset};
use serde_json::{Map, Value, json};

pub(crate) use self::env::worker_env;
pub use self::env::{
    BRAIN_ALLOWLIST, HostEnv, PASS_THROUGH, SessionEnv, is_denylisted, mcp_tool_timeout_ms,
};
use crate::{HarnessConfig, SUBAGENT_TOOLS};

/// Every flag Eileen passes to `claude` that `claude --help` lists. The nightly CLI drift job
/// (section 4.6) checks each against the help of the pinned and the latest CLI; a test checks
/// that every flag [`session_files`] writes is here or in [`CLAUDE_UNLISTED_FLAGS`].
pub const CLAUDE_FLAGS: [&str; 24] = [
    "-p",
    "--input-format",
    "--output-format",
    "--verbose",
    "--include-partial-messages",
    "--include-hook-events",
    "--model",
    "--effort",
    "--system-prompt",
    "--append-system-prompt",
    "--setting-sources",
    "--settings",
    "--strict-mcp-config",
    "--mcp-config",
    "--tools",
    "--allowedTools",
    "--disallowedTools",
    "--permission-mode",
    "--plugin-dir",
    "--no-session-persistence",
    "--resume",
    "--fork-session",
    "--chrome",
    "--no-chrome",
];

/// Flags Eileen passes that `claude --help` no longer lists, though the CLI accepts them:
/// `--permission-prompt-tool` left the help on 2.1.304 (section 2.5). The drift job checks
/// these by behaviour, not against the help.
pub const CLAUDE_UNLISTED_FLAGS: [&str; 1] = ["--permission-prompt-tool"];

/// The name of Eileen's own MCP server, so Claude sees its tools as `mcp__eileen__<tool>`.
pub const EILEEN_SERVER: &str = "eileen";

/// The permission-prompt tool, which the CLI calls after the guard answers `ask`.
pub const APPROVE_TOOL: &str = "mcp__eileen__approve";

/// The brain's whole `--allowedTools`: Eileen's own tools and nothing else.
pub const BRAIN_ALLOWED_TOOLS: &str = "mcp__eileen__*";

/// How long the CLI waits for the `PreToolUse` guard and the `PermissionRequest` tripwire,
/// seconds.
pub const GUARD_TIMEOUT_S: u16 = 10;

/// How long the CLI waits for each event hook to the loopback bus, seconds. Explicit, because
/// an HTTP hook's default is 600 s.
pub const BUS_HOOK_TIMEOUT_S: u16 = 5;

/// The hooks that report to the loopback bus, with the path each posts to.
pub const BUS_HOOKS: [(&str, &str); 7] = [
    ("PermissionDenied", "permission-denied"),
    ("PostToolUseFailure", "post-tool-use-failure"),
    ("Notification", "notification"),
    ("SubagentStart", "subagent-start"),
    ("SubagentStop", "subagent-stop"),
    ("Stop", "stop"),
    ("StopFailure", "stop-failure"),
];

/// The settings file's name in the session's run folder.
pub const SETTINGS_FILE: &str = "claude-settings.json";

/// The MCP config's name in the session's run folder.
pub const MCP_FILE: &str = "mcp.json";

/// Everything a Claude session spawns with. The driver writes [`Self::settings`] to
/// [`Self::settings_path`] and [`Self::mcp_config`] to [`Self::mcp_path`], then spawns the CLI
/// with [`Self::argv`] and exactly [`Self::env`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFiles {
    /// The session's run folder, `<data_dir>/run/<session>`.
    pub run_dir: PathBuf,
    /// The folder the CLI starts in: the spec's `cwd`, under the data folder when it is
    /// relative (section 2.5: the job's repository, or `run/<session>/cwd`).
    pub cwd: PathBuf,
    /// The arguments after the executable.
    pub argv: Vec<String>,
    /// Where the settings file goes.
    pub settings_path: PathBuf,
    /// The settings file. It holds no token.
    pub settings: Value,
    /// Where the MCP config goes.
    pub mcp_path: PathBuf,
    /// The MCP config. It holds no token.
    pub mcp_config: Value,
    /// Each approved third-party server's rung, by its name in the MCP config, so the harness
    /// can tag each of its calls (section 2.5). Eileen's own server is not listed.
    pub mcp_rungs: BTreeMap<String, Rung>,
    /// The whole environment the session starts with.
    pub env: SessionEnv,
}

/// Why a session's files could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    /// `HarnessConfig.data_dir` is relative, so the run folder would depend on the working
    /// folder.
    #[error("the data folder is not an absolute path")]
    RelativeDataDir,
    /// A path the CLI would be given is relative, so the CLI would look for it from the
    /// session's working folder, or search `PATH` for a bare command: a file planted there would
    /// run with the session's tokens in reach.
    #[error("the session's {0} is not an absolute path")]
    RelativePath(&'static str),
    /// The runtime has not filled the hook or shim endpoint: a session never starts without
    /// its tokens.
    #[error("the session's {0} is not filled in")]
    EndpointMissing(&'static str),
    /// A path the session would use lies under a `.claude` folder, where the user's own
    /// sessions load files from (section 2.12).
    #[error("the session's {0} lies under a .claude folder")]
    UnderClaudeFolder(&'static str),
    /// A path is not valid UTF-8, so it cannot be written into a command line or a JSON file.
    #[error("the session's {0} is not valid UTF-8")]
    NotUtf8(&'static str),
    /// An approved third-party server took Eileen's own server name.
    #[error("an approved MCP server cannot be named {EILEEN_SERVER}")]
    ReservedServerName,
    /// Two approved third-party servers share a name.
    #[error("two approved MCP servers share a name")]
    DuplicateServerName,
    /// The brain and the librarian, which have no shell, start only with the constructed
    /// allowlist (section 2.12).
    #[error("a session without a shell must start with the constructed allowlist")]
    NeedsAllowlist,
    /// A third-party server's app has no entry in `mcp.servers`, so the user never approved it.
    #[error("an MCP server's app has no approved entry in the config")]
    UnapprovedServer,
    /// A third-party server's config entry does not list the session's job kind (section 2.5).
    #[error("an MCP server is not approved for this job kind")]
    ServerNotForKind,
    /// The brain gets only Eileen's own server (section 2.3).
    #[error("the brain gets no third-party MCP server")]
    BrainExtraServer,
    /// A third-party server's command or an argument holds a `$`. The CLI expands `${VAR}` in
    /// them from its own environment, which holds Eileen's tokens.
    #[error("an MCP server's command or arguments hold a $")]
    ExpandableMcpArg,
}

/// Builds what the session in `spec` spawns with.
pub fn session_files(
    spec: &SessionSpec,
    cfg: &HarnessConfig,
    host: &HostEnv,
) -> Result<SessionFiles, SessionError> {
    if !cfg.data_dir.is_absolute() {
        return Err(SessionError::RelativeDataDir);
    }
    check_endpoints(spec)?;
    let brain = is_brain(spec);
    if needs_allowlist(spec) && spec.env != EnvPolicy::ConstructedAllowlist {
        return Err(SessionError::NeedsAllowlist);
    }
    let run_dir = cfg.data_dir.join("run").join(spec.session.to_string());
    let settings_path = run_dir.join(SETTINGS_FILE);
    let mcp_path = run_dir.join(MCP_FILE);
    let settings_text = path_text(&settings_path, "settings file")?;
    let mcp_text = path_text(&mcp_path, "MCP config")?;
    // `join` keeps an absolute `cwd` as it is and puts a relative one under the data folder.
    let cwd = cfg.data_dir.join(&spec.cwd);
    path_text(&cwd, "working folder")?;
    let plugin_dirs = spec
        .plugin_dirs
        .iter()
        .map(|dir| path_text(Path::new(dir), "plugin folder"))
        .collect::<Result<Vec<_>, _>>()?;
    let (mcp_config, mcp_rungs) = mcp_config(spec, cfg)?;
    Ok(SessionFiles {
        argv: argv(spec, brain, settings_text, mcp_text, &plugin_dirs),
        settings: settings(spec),
        mcp_config,
        mcp_rungs,
        env: env::session_env(spec, cfg, host),
        run_dir,
        cwd,
        settings_path,
        mcp_path,
    })
}

/// Whether `spec` is the brain's session: the one with the brain's toolset.
#[must_use]
pub fn is_brain(spec: &SessionSpec) -> bool {
    spec.toolsets.contains(&Toolset::Brain)
}

/// Whether `spec` must start with the constructed allowlist: the brain and the librarian, which
/// have no shell (section 2.12). The scribe joins them once it has a job kind.
#[must_use]
pub fn needs_allowlist(spec: &SessionSpec) -> bool {
    is_brain(spec) || spec.kind == JobKind::Librarian
}

/// The role the session's shim names in its hello (section 3.3.9).
#[must_use]
pub fn shim_role(spec: &SessionSpec) -> ShimRole {
    if is_brain(spec) {
        ShimRole::Brain
    } else if spec.kind == JobKind::Librarian {
        ShimRole::Librarian
    } else {
        ShimRole::Worker(spec.session)
    }
}

fn check_endpoints(spec: &SessionSpec) -> Result<(), SessionError> {
    let hook = &spec.hook_endpoint;
    if hook.port == 0 {
        return Err(SessionError::EndpointMissing("hook port"));
    }
    if hook.token.expose().is_empty() {
        return Err(SessionError::EndpointMissing("hook token"));
    }
    if spec.shim.pipe.is_empty() {
        return Err(SessionError::EndpointMissing("shim pipe"));
    }
    if spec.shim.token.expose().is_empty() {
        return Err(SessionError::EndpointMissing("shim token"));
    }
    Ok(())
}

/// The path as text, refused when it lies under a `.claude` folder, is relative or is not
/// UTF-8.
fn path_text<'a>(path: &'a Path, what: &'static str) -> Result<&'a str, SessionError> {
    if under_claude_folder(path) {
        return Err(SessionError::UnderClaudeFolder(what));
    }
    if !path.is_absolute() {
        return Err(SessionError::RelativePath(what));
    }
    path.to_str().ok_or(SessionError::NotUtf8(what))
}

/// Whether any folder in `path` is named `.claude`, in any case. Windows paths are split on
/// both separators, so the check holds for a Windows path read on Linux too.
#[must_use]
pub fn under_claude_folder(path: &Path) -> bool {
    path.components().any(|part| match part {
        Component::Normal(name) => name.to_str().is_some_and(|name| {
            name.split(['\\', '/']).any(|part| part.eq_ignore_ascii_case(".claude"))
        }),
        Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir => {
            false
        }
    })
}

fn argv(
    spec: &SessionSpec,
    brain: bool,
    settings: &str,
    mcp: &str,
    plugin_dirs: &[&str],
) -> Vec<String> {
    let mut argv: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--include-hook-events",
        "--model",
    ]
    .map(str::to_owned)
    .into();
    argv.push(spec.model.clone());
    if let Some(effort) = spec.effort {
        argv.extend(["--effort".to_owned(), effort_text(effort).to_owned()]);
    }
    if !spec.append_system_prompt.is_empty() {
        // The brain's prompt replaces the CLI's long default one, which would slow every turn
        // (section 2.3); a worker keeps the default and adds its own (section 2.5).
        let flag = if brain { "--system-prompt" } else { "--append-system-prompt" };
        argv.extend([flag.to_owned(), spec.append_system_prompt.clone()]);
    }
    let (tools, allowed) = if brain {
        (String::new(), BRAIN_ALLOWED_TOOLS.to_owned())
    } else {
        (tool_list(&spec.builtin_tools), tool_list(&spec.allowed_tools))
    };
    let mut disallowed: Vec<String> = SUBAGENT_TOOLS.map(str::to_owned).into();
    disallowed.extend(spec.disallowed_tools.iter().filter(|tool| !disallowed_has(tool)).cloned());
    argv.extend(
        [
            "--setting-sources",
            "",
            "--settings",
            settings,
            "--strict-mcp-config",
            "--mcp-config",
            mcp,
            "--tools",
            &tools,
            "--allowedTools",
            &allowed,
            "--disallowedTools",
            &disallowed.join(","),
            "--permission-mode",
            "dontAsk",
            "--permission-prompt-tool",
            APPROVE_TOOL,
        ]
        .map(str::to_owned),
    );
    for dir in plugin_dirs {
        argv.extend(["--plugin-dir".to_owned(), (*dir).to_owned()]);
    }
    match &spec.resume_session {
        Some(id) => argv.extend(["--resume".to_owned(), id.clone(), "--fork-session".to_owned()]),
        None => argv.push("--no-session-persistence".to_owned()),
    }
    argv.push(if spec.chrome { "--chrome" } else { "--no-chrome" }.to_owned());
    argv
}

const fn effort_text(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
    }
}

fn disallowed_has(tool: &str) -> bool {
    SUBAGENT_TOOLS.contains(&tool)
}

/// The tools comma-separated, without any subagent tool: a spec that lists one still never
/// gets it.
fn tool_list(tools: &[String]) -> String {
    tools.iter().filter(|tool| !disallowed_has(tool)).cloned().collect::<Vec<_>>().join(",")
}

/// The headers every hook sends: the bearer token and the session id, interpolated by the CLI
/// from the session's environment, so neither sits in the file.
fn hook(url: &str, timeout_s: u16, block: bool) -> Value {
    let mut hook = json!({
        "type": "http",
        "url": url,
        "timeout": timeout_s,
        "headers": {
            "Authorization": "Bearer $EILEEN_HOOK_TOKEN",
            "X-Eileen-Session": "$EILEEN_SESSION_ID",
        },
        "allowedEnvVars": [env::HOOK_TOKEN, env::SESSION_ID],
    });
    if block && let Value::Object(fields) = &mut hook {
        fields.insert("onFailure".to_owned(), Value::from("block"));
    }
    json!([{ "hooks": [hook] }])
}

fn settings(spec: &SessionSpec) -> Value {
    let endpoint = &spec.hook_endpoint;
    let base = format!("http://127.0.0.1:{}/{}", endpoint.port, endpoint.path.trim_matches('/'));
    let base = base.trim_end_matches('/');
    let mut hooks = Map::new();
    hooks.insert(
        "PreToolUse".to_owned(),
        hook(&format!("{base}/pre-tool-use"), GUARD_TIMEOUT_S, true),
    );
    hooks.insert(
        "PermissionRequest".to_owned(),
        hook(&format!("{base}/permission-request"), GUARD_TIMEOUT_S, true),
    );
    for (event, path) in BUS_HOOKS {
        hooks.insert(event.to_owned(), hook(&format!("{base}/{path}"), BUS_HOOK_TIMEOUT_S, false));
    }
    json!({
        "disableSkillShellExecution": true,
        "fileCheckpointingEnabled": false,
        "hooks": hooks,
    })
}

fn mcp_config(
    spec: &SessionSpec,
    cfg: &HarnessConfig,
) -> Result<(Value, BTreeMap<String, Rung>), SessionError> {
    let shim = path_text(&cfg.shim_exe, "shim executable")?;
    let mut args = vec!["--pipe".to_owned(), spec.shim.pipe.clone(), "--role".to_owned()];
    match shim_role(spec) {
        ShimRole::Worker(session) => {
            args.extend(["worker".to_owned(), "--session".to_owned(), session.to_string()]);
        }
        ShimRole::Brain => args.push("brain".to_owned()),
        ShimRole::Librarian => args.push("librarian".to_owned()),
        ShimRole::Session => args.push("session".to_owned()),
        ShimRole::Dev => args.push("dev".to_owned()),
    }
    let mut servers = Map::new();
    servers.insert(
        EILEEN_SERVER.to_owned(),
        json!({
            "type": "stdio",
            "command": shim,
            "args": args,
            "env": { (env::SHIM_TOKEN): format!("${{{}}}", env::SHIM_TOKEN) },
        }),
    );
    let mut rungs = BTreeMap::new();
    if is_brain(spec) && !spec.extra_mcp_servers.is_empty() {
        return Err(SessionError::BrainExtraServer);
    }
    for server in &spec.extra_mcp_servers {
        if server.name == EILEEN_SERVER {
            return Err(SessionError::ReservedServerName);
        }
        let approved = cfg.mcp.servers.get(&server.app).ok_or(SessionError::UnapprovedServer)?;
        if !approved.job_kinds.contains(&spec.kind) {
            return Err(SessionError::ServerNotForKind);
        }
        // The CLI expands `${VAR}` in a server's command and arguments from its own
        // environment, which holds both tokens, so neither may hold a `$` at all.
        if server.command.get().contains('$') || server.args.iter().any(|arg| arg.contains('$')) {
            return Err(SessionError::ExpandableMcpArg);
        }
        let command = path_text(Path::new(server.command.get()), "MCP server command")?;
        // A third-party server may inherit the CLI's environment, so Eileen's tokens are
        // blanked for it: only Eileen's own shim may hold them.
        let entry = json!({
            "type": "stdio",
            "command": command,
            "args": server.args,
            "env": { (env::HOOK_TOKEN): "", (env::SHIM_TOKEN): "" },
        });
        if servers.insert(server.name.clone(), entry).is_some() {
            return Err(SessionError::DuplicateServerName);
        }
        rungs.insert(server.name.clone(), approved.rung);
    }
    Ok((json!({ "mcpServers": servers }), rungs))
}

#[cfg(test)]
mod tests;
