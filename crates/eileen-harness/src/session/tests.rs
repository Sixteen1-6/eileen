//! What each session kind spawns with: snapshots of the command line, the settings file and the
//! MCP config for a coding worker, a computer-use worker and the brain, and the rules of
//! sections 2.5 and 2.12 checked on their own.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eileen_contracts::config::{McpConfig, McpServerConfig};
use eileen_contracts::core_io::Rung;
use eileen_contracts::harness::{
    ApprovedMcpServer, BuildSlotTicket, Effort, EnvPolicy, HookEndpoint, SessionSpec, SessionToken,
    ShimEndpoint,
};
use eileen_contracts::ids::{JobId, Personal, TraceCtx, WorkerSessionId};
use eileen_contracts::jobs::{JobKind, ObservedSource, PoolClass, Route, Toolset};
use eileen_contracts::learn::PackageType;
use serde_json::Value;

use super::{
    BRAIN_ALLOWLIST, CLAUDE_FLAGS, CLAUDE_UNLISTED_FLAGS, HostEnv, PASS_THROUGH, SessionError,
    SessionFiles, is_denylisted, mcp_tool_timeout_ms, session_files, under_claude_folder,
};
use crate::{HarnessConfig, SUBAGENT_TOOLS};

const HOOK_TOKEN: &str = "hook-token-c0ffee-0123456789";
const SHIM_TOKEN: &str = "shim-token-decaf-9876543210";

/// The variables Eileen sets in every session, whatever the user's environment held.
const SESSION_VARS: [&str; 8] = [
    "ENABLE_CLAUDEAI_MCP_SERVERS",
    "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
    "CLAUDE_CODE_DISABLE_FILE_CHECKPOINTING",
    "EILEEN_SESSION_ID",
    "EILEEN_HOOK_TOKEN",
    "EILEEN_SHIM_TOKEN",
    "MCP_TOOL_TIMEOUT",
    "TRACEPARENT",
];

fn session_id(n: u128) -> WorkerSessionId {
    WorkerSessionId::from_parts(1_790_000_000_000, n)
}

fn base_spec(session: WorkerSessionId, kind: JobKind, toolsets: Vec<Toolset>) -> SessionSpec {
    SessionSpec {
        session,
        pool: None,
        kind,
        route: Route::Claude,
        model: "claude-sonnet-5-5".to_owned(),
        effort: None,
        cwd: format!("/data/eileen/run/{session}/cwd"),
        worktree: None,
        snapshot_ref: None,
        append_system_prompt: "You are a Claude worker for Eileen.".to_owned(),
        toolsets,
        allowed_tools: Vec::new(),
        extra_mcp_servers: Vec::new(),
        builtin_tools: Vec::new(),
        disallowed_tools: Vec::new(),
        plugin_dirs: vec![format!("/data/eileen/run/{session}/plugin")],
        snapshot: None,
        resume_session: None,
        chrome: false,
        env: EnvPolicy::WorkerDenylist,
        build_slot: None,
        memory_limit_mb: None,
        trace: TraceCtx {
            turn: None,
            job: Some(JobId::from_parts(1_790_000_000_000, 42)),
            call: None,
        },
        hook_endpoint: HookEndpoint {
            port: 47615,
            path: "/v1/hooks/claude".to_owned(),
            token: SessionToken::new(HOOK_TOKEN.to_owned()),
        },
        shim: ShimEndpoint {
            pipe: format!(r"\\.\pipe\eileen-dispatch-{session}"),
            token: SessionToken::new(SHIM_TOKEN.to_owned()),
        },
    }
}

fn coding_worker() -> SessionSpec {
    let mut spec = base_spec(session_id(1), JobKind::Coding, vec![Toolset::Worker]);
    spec.builtin_tools =
        ["Bash", "PowerShell", "Read", "Edit", "Write", "Glob", "Grep"].map(str::to_owned).into();
    spec.allowed_tools = spec.builtin_tools.clone();
    spec.allowed_tools.push("mcp__eileen__*".to_owned());
    spec.effort = Some(Effort::High);
    spec.build_slot = Some(BuildSlotTicket { slot: 0, cargo_jobs: 4 });
    spec
}

fn computer_use_worker() -> SessionSpec {
    let mut spec = base_spec(
        session_id(2),
        JobKind::ComputerUse,
        vec![Toolset::Worker, Toolset::Desktop, Toolset::Apps],
    );
    spec.pool = Some(PoolClass::ComputerUse);
    spec.allowed_tools = vec!["mcp__eileen__*".to_owned(), "mcp__playwright__*".to_owned()];
    spec.extra_mcp_servers = vec![ApprovedMcpServer {
        app: "browser".to_owned(),
        name: "playwright".to_owned(),
        command: Personal::new("/data/eileen/mcp/browser/node_modules/.bin/mcp-server".to_owned()),
        args: vec!["--headless".to_owned()],
    }];
    spec
}

fn brain() -> SessionSpec {
    let mut spec = base_spec(session_id(3), JobKind::Act, vec![Toolset::Brain, Toolset::Act]);
    spec.model = "claude-haiku-5-5".to_owned();
    spec.effort = Some(Effort::Low);
    spec.append_system_prompt = "You are Eileen.".to_owned();
    spec.plugin_dirs = Vec::new();
    spec.env = EnvPolicy::ConstructedAllowlist;
    spec
}

fn config() -> HarnessConfig {
    let browser = McpServerConfig {
        registry_id: "io.example/browser-mcp".to_owned(),
        package_type: PackageType::Npm,
        version: "1.2.3".to_owned(),
        command: "/data/eileen/mcp/browser/node_modules/.bin/mcp-server".to_owned(),
        args: vec!["--headless".to_owned()],
        digest: "sha256-0000".to_owned(),
        job_kinds: vec![JobKind::ComputerUse],
        rung: Rung::Dom,
        observes: ObservedSource::Web,
    };
    HarnessConfig {
        data_dir: PathBuf::from("/data/eileen"),
        hook_hold_s: 120,
        shim_exe: PathBuf::from("/opt/eileen/eileen-shim"),
        mcp: McpConfig { servers: BTreeMap::from([("browser".to_owned(), browser)]) },
    }
}

/// A user's environment with one of each kind of variable: the brain's allowlist, the
/// denylist in several spellings, the pass-through list, and ordinary variables.
fn host() -> HostEnv {
    let vars = [
        ("Path", "/usr/bin"),
        ("SystemRoot", r"C:\Windows"),
        ("USERPROFILE", r"C:\Users\someone"),
        ("LOCALAPPDATA", r"C:\Users\someone\AppData\Local"),
        ("TEMP", r"C:\Users\someone\AppData\Local\Temp"),
        ("HOME", "/home/someone"),
        ("EDITOR", "notepad"),
        ("LANG", "en_GB.UTF-8"),
        ("ANTHROPIC_API_KEY", "not-a-real-key"),
        ("ANTHROPIC_BASE_URL", "https://example.invalid"),
        ("CLAUDE_CONFIG_DIR", "/home/someone/.claude"),
        ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "0"),
        ("claude_code_disable_file_checkpointing", "0"),
        ("MCP_TIMEOUT", "1"),
        ("MCP_TOOL_TIMEOUT", "1"),
        ("OPENAI_API_KEY", "not-a-real-key"),
        ("CODEX_HOME", "/home/someone/.codex"),
        ("XAI_API_KEY", "not-a-real-key"),
        ("DEEPSEEK_API_KEY", "not-a-real-key"),
        ("AWS_PROFILE", "default"),
        ("AWS_SECRET_ACCESS_KEY", "not-a-real-key"),
        ("GH_TOKEN", "not-a-real-token"),
        ("github_token", "not-a-real-token"),
        ("NPM_TOKEN", "not-a-real-token"),
        ("SERVICE_SECRET", "not-a-real-secret"),
        ("STRIPE_API_KEY", "not-a-real-key"),
        ("CLAUDE_CODE_GIT_BASH_PATH", r"C:\Program Files\Git\bin\bash.exe"),
        ("HTTPS_PROXY", "http://proxy.invalid:8080"),
        ("HTTP_PROXY", "http://proxy.invalid:8080"),
        ("NO_PROXY", "localhost"),
        ("NODE_EXTRA_CA_CERTS", "/etc/ssl/extra.pem"),
        ("ENABLE_CLAUDEAI_MCP_SERVERS", "true"),
    ];
    HostEnv {
        vars: vars.map(|(name, value)| (name.to_owned(), value.to_owned())).into(),
        registry_path: None,
        brain_path: None,
        toolchain_dirs: Vec::new(),
    }
}

fn files(spec: &SessionSpec) -> SessionFiles {
    session_files(spec, &config(), &host()).unwrap()
}

/// The path as the snapshots hold it: on Windows, `Path::join` puts a backslash after the test's
/// data folder, so paths under it are written with forward slashes on every OS.
fn portable(text: &str) -> String {
    if text.starts_with("/data/") { text.replace('\\', "/") } else { text.to_owned() }
}

fn portable_json(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(portable(text)),
        Value::Array(items) => Value::Array(items.iter().map(portable_json).collect()),
        Value::Object(fields) => Value::Object(
            fields.iter().map(|(key, value)| (key.clone(), portable_json(value))).collect(),
        ),
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
    }
}

fn portable_argv(files: &SessionFiles) -> Vec<String> {
    files.argv.iter().map(|arg| portable(arg)).collect()
}

/// The value after `flag` in the command line.
fn flag_value<'a>(files: &'a SessionFiles, flag: &str) -> Option<&'a str> {
    let at = files.argv.iter().position(|arg| arg == flag)?;
    files.argv.get(at + 1).map(String::as_str)
}

fn all_three() -> [(&'static str, SessionSpec); 3] {
    [("coding", coding_worker()), ("computer use", computer_use_worker()), ("brain", brain())]
}

#[test]
fn a_coding_worker_spawns_with_its_flags_settings_and_mcp_config() {
    let files = files(&coding_worker());
    insta::assert_json_snapshot!("coding_worker_argv", portable_argv(&files));
    insta::assert_json_snapshot!("coding_worker_settings", portable_json(&files.settings));
    insta::assert_json_snapshot!("coding_worker_mcp", portable_json(&files.mcp_config));
    assert!(files.mcp_rungs.is_empty(), "no third-party server, so no rung");
    assert_eq!(files.env.get("CARGO_BUILD_JOBS"), Some("4"), "the build slot's job count");
}

#[test]
fn a_computer_use_worker_spawns_with_its_flags_settings_and_mcp_config() {
    let files = files(&computer_use_worker());
    insta::assert_json_snapshot!("computer_use_worker_argv", portable_argv(&files));
    insta::assert_json_snapshot!("computer_use_worker_settings", portable_json(&files.settings));
    insta::assert_json_snapshot!("computer_use_worker_mcp", portable_json(&files.mcp_config));
    assert_eq!(
        files.mcp_rungs,
        BTreeMap::from([("playwright".to_owned(), Rung::Dom)]),
        "the approved server is tagged with the rung its config entry names"
    );
}

#[test]
fn the_brain_spawns_with_its_flags_settings_and_mcp_config() {
    let files = files(&brain());
    insta::assert_json_snapshot!("brain_argv", portable_argv(&files));
    insta::assert_json_snapshot!("brain_settings", portable_json(&files.settings));
    insta::assert_json_snapshot!("brain_mcp", portable_json(&files.mcp_config));
}

#[test]
fn the_brain_gets_no_built_in_tool_and_only_eileens_tools() {
    let mut spec = brain();
    spec.builtin_tools = vec!["Bash".to_owned()];
    spec.allowed_tools = vec!["Bash".to_owned(), "WebFetch".to_owned()];
    let files = files(&spec);
    assert_eq!(flag_value(&files, "--tools"), Some(""), "the brain has no built-in tool");
    assert_eq!(flag_value(&files, "--allowedTools"), Some("mcp__eileen__*"), "only Eileen's");
}

#[test]
fn an_approved_server_with_no_config_entry_is_an_app_mcp_rung() {
    let mut cfg = config();
    cfg.mcp.servers.clear();
    let files = session_files(&computer_use_worker(), &cfg, &host()).unwrap();
    assert_eq!(files.mcp_rungs.get("playwright"), Some(&Rung::AppMcp), "section 2.5's default");
}

#[test]
fn the_worker_environment_drops_the_denylist_and_keeps_the_pass_through_list() {
    for spec in [coding_worker(), computer_use_worker()] {
        let files = files(&spec);
        for (name, _) in &host().vars {
            if !is_denylisted(name) {
                assert!(files.env.get(name).is_some(), "{name} is not denylisted, so it stays");
                continue;
            }
            let ours = SESSION_VARS.iter().any(|var| var.eq_ignore_ascii_case(name));
            assert!(
                ours || files.env.get(name).is_none(),
                "{name} is denylisted and must not reach a worker"
            );
        }
        for name in PASS_THROUGH {
            assert_eq!(
                files.env.get(name),
                host().vars.iter().find(|(var, _)| var == name).map(|(_, value)| value.as_str()),
                "{name} passes through unchanged"
            );
        }
        for name in ["ANTHROPIC_API_KEY", "GH_TOKEN", "github_token", "AWS_PROFILE", "NPM_TOKEN"] {
            assert!(files.env.get(name).is_none(), "{name} is removed");
        }
        assert_eq!(files.env.get("GIT_CONFIG_COUNT"), Some("2"), "git's fsmonitor and bare rules");
        assert_eq!(files.env.get("GIT_CONFIG_KEY_1"), Some("safe.bareRepository"), "bare rule");
        assert_eq!(files.env.get("EDITOR"), Some("notepad"), "the user's own variables stay");
    }
}

#[test]
fn the_brain_environment_holds_only_the_allowlist() {
    let files = files(&brain());
    for name in files.env.names() {
        let allowed = BRAIN_ALLOWLIST.iter().any(|var| var.eq_ignore_ascii_case(name));
        assert!(
            allowed || SESSION_VARS.contains(&name),
            "{name} is neither on the brain's allowlist nor the session's own"
        );
    }
    assert_eq!(files.env.get("PATH"), Some("/usr/bin"), "PATH in any case is allowlisted");
    assert_eq!(files.env.get("SystemRoot"), Some(r"C:\Windows"), "an allowlisted variable");
    for name in ["HOME", "EDITOR", "HTTPS_PROXY", "CLAUDE_CODE_GIT_BASH_PATH", "GIT_CONFIG_COUNT"] {
        assert!(files.env.get(name).is_none(), "{name} is not on the brain's allowlist");
    }
}

#[test]
fn a_session_variable_replaces_the_users_in_every_spelling() {
    let files = files(&coding_worker());
    let names: Vec<&str> = files.env.names().collect();
    for var in SESSION_VARS {
        let spellings = names.iter().filter(|name| name.eq_ignore_ascii_case(var)).count();
        assert_eq!(spellings, 1, "{var} appears once, whatever case the user's copy had");
    }
    assert_eq!(files.env.get("ENABLE_CLAUDEAI_MCP_SERVERS"), Some("false"), "connectors are off");
}

#[test]
fn the_registry_path_and_the_brain_path_replace_the_users() {
    let mut host = host();
    host.registry_path = Some("/registry/bin".to_owned());
    host.brain_path = Some("/system/bin".to_owned());
    let worker = session_files(&computer_use_worker(), &config(), &host).unwrap();
    assert_eq!(worker.env.get("PATH"), Some("/registry/bin"), "the worker's PATH");
    assert_eq!(
        worker.env.names().filter(|name| name.eq_ignore_ascii_case("PATH")).count(),
        1,
        "one PATH, whatever case the user's had"
    );
    let brain = session_files(&brain(), &config(), &host).unwrap();
    assert_eq!(brain.env.get("PATH"), Some("/system/bin"), "the brain's constructed PATH");
}

#[test]
fn a_coding_session_gets_missing_toolchain_folders_first() {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let mut host = host();
    host.toolchain_dirs = vec!["/home/someone/.cargo/bin".to_owned(), "/usr/bin".to_owned()];
    let coding = session_files(&coding_worker(), &config(), &host).unwrap();
    assert_eq!(
        coding.env.get("PATH"),
        Some(format!("/home/someone/.cargo/bin{separator}/usr/bin").as_str()),
        "the missing folder goes first and a present one is not repeated"
    );
    let other = session_files(&computer_use_worker(), &config(), &host).unwrap();
    assert_eq!(other.env.get("PATH"), Some("/usr/bin"), "only coding sessions get them");
}

#[test]
fn mcp_tool_timeout_follows_the_hook_hold() {
    assert_eq!(mcp_tool_timeout_ms(120), 150_000, "the default hold of 120 s");
    assert_eq!(mcp_tool_timeout_ms(0), 30_000, "no hold leaves the 30 s margin");
    assert_eq!(mcp_tool_timeout_ms(u16::MAX), 65_565_000, "no overflow");
    let mut cfg = config();
    for (hold, expected) in [(120, "150000"), (300, "330000")] {
        cfg.hook_hold_s = hold;
        for (kind, spec) in all_three() {
            let files = session_files(&spec, &cfg, &host()).unwrap();
            assert_eq!(files.env.get("MCP_TOOL_TIMEOUT"), Some(expected), "{kind} at {hold} s");
        }
    }
    assert_eq!(HarnessConfig::default().hook_hold_s, 120, "approvals.hook_hold_s's default");
}

#[test]
fn no_token_appears_in_either_file_or_the_command_line() {
    for (kind, spec) in all_three() {
        let files = files(&spec);
        let settings = files.settings.to_string();
        let mcp = files.mcp_config.to_string();
        for token in [HOOK_TOKEN, SHIM_TOKEN] {
            assert!(!settings.contains(token), "{kind}: a token in the settings file");
            assert!(!mcp.contains(token), "{kind}: a token in the MCP config");
            assert!(!files.argv.iter().any(|arg| arg.contains(token)), "{kind}: in argv");
        }
        assert_eq!(files.env.get("EILEEN_HOOK_TOKEN"), Some(HOOK_TOKEN), "{kind}: hook token");
        assert_eq!(files.env.get("EILEEN_SHIM_TOKEN"), Some(SHIM_TOKEN), "{kind}: shim token");
        assert!(settings.contains("Bearer $EILEEN_HOOK_TOKEN"), "{kind}: interpolated");
        assert!(mcp.contains("${EILEEN_SHIM_TOKEN}"), "{kind}: interpolated by the CLI");
        let debug = format!("{files:?}");
        assert!(!debug.contains(HOOK_TOKEN) && !debug.contains(SHIM_TOKEN), "{kind}: Debug");
    }
}

#[test]
fn every_session_turns_vendor_memory_and_file_checkpointing_off() {
    for (kind, spec) in all_three() {
        let files = files(&spec);
        assert_eq!(files.env.get("CLAUDE_CODE_DISABLE_AUTO_MEMORY"), Some("1"), "{kind}: D77");
        assert_eq!(
            files.env.get("CLAUDE_CODE_DISABLE_FILE_CHECKPOINTING"),
            Some("1"),
            "{kind}: checkpointing off by the environment"
        );
        assert_eq!(
            files.settings.get("fileCheckpointingEnabled"),
            Some(&Value::Bool(false)),
            "{kind}: checkpointing off by the settings file"
        );
        assert_eq!(
            files.settings.get("disableSkillShellExecution"),
            Some(&Value::Bool(true)),
            "{kind}: no shell from a skill"
        );
    }
}

#[test]
fn disallowed_tools_names_task_and_agent_and_tools_never_lists_them() {
    assert_eq!(SUBAGENT_TOOLS, ["Task", "Agent"], "both names of the subagent tool");
    for (kind, mut spec) in all_three() {
        spec.builtin_tools.extend(["Task".to_owned(), "Agent".to_owned()]);
        spec.allowed_tools.push("Agent".to_owned());
        spec.disallowed_tools = vec!["Task".to_owned(), "WebSearch".to_owned()];
        let files = files(&spec);
        assert_eq!(
            flag_value(&files, "--disallowedTools"),
            Some("Task,Agent,WebSearch"),
            "{kind}: the subagent tools first, each once"
        );
        for flag in ["--tools", "--allowedTools"] {
            let listed = flag_value(&files, flag).unwrap_or_default();
            assert!(
                !listed.split(',').any(|tool| SUBAGENT_TOOLS.contains(&tool)),
                "{kind}: {flag} lists a subagent tool"
            );
        }
    }
}

#[test]
fn every_flag_is_in_the_flag_constant() {
    let mut spec = coding_worker();
    spec.resume_session = Some("earlier-session".to_owned());
    spec.chrome = true;
    for (kind, spec) in [all_three().to_vec(), vec![("resumed", spec)]].concat() {
        let files = files(&spec);
        for arg in files.argv.iter().filter(|arg| arg.starts_with('-')) {
            assert!(
                CLAUDE_FLAGS.contains(&arg.as_str())
                    || CLAUDE_UNLISTED_FLAGS.contains(&arg.as_str()),
                "{kind}: {arg} is missing from the flag constant the drift job reads"
            );
        }
    }
}

#[test]
fn a_resumed_session_forks_and_chrome_is_opt_in() {
    let mut spec = coding_worker();
    spec.resume_session = Some("earlier-session".to_owned());
    spec.chrome = true;
    let files = files(&spec);
    assert_eq!(flag_value(&files, "--resume"), Some("earlier-session"), "resumed");
    assert!(files.argv.iter().any(|arg| arg == "--fork-session"), "forked, never continued");
    assert!(!files.argv.iter().any(|arg| arg == "--no-session-persistence"), "it is resumed");
    assert!(files.argv.iter().any(|arg| arg == "--chrome"), "the user opted in");
    assert!(!files.argv.iter().any(|arg| arg == "--no-chrome"), "one or the other");
}

#[test]
fn no_generated_path_lies_under_the_users_claude_folder() {
    for (kind, spec) in all_three() {
        let files = files(&spec);
        let paths = [&files.run_dir, &files.settings_path, &files.mcp_path];
        for path in paths {
            assert!(
                path.starts_with("/data/eileen/run"),
                "{kind}: {} is not in run",
                path.display()
            );
            assert!(!under_claude_folder(path), "{kind}: {}", path.display());
        }
        for arg in &files.argv {
            assert!(!under_claude_folder(Path::new(arg)), "{kind}: {arg} names a .claude folder");
        }
        let mcp = files.mcp_config.to_string();
        assert!(!mcp.to_ascii_lowercase().contains(".claude"), "{kind}: the MCP config");
    }
    let mut cfg = config();
    cfg.data_dir = PathBuf::from("/home/someone/.claude/eileen");
    assert_eq!(
        session_files(&brain(), &cfg, &host()),
        Err(SessionError::UnderClaudeFolder("settings file")),
        "a data folder under .claude is refused"
    );
    let mut spec = coding_worker();
    spec.plugin_dirs = vec![r"C:\Users\someone\.Claude\plugins\x".to_owned()];
    assert_eq!(
        session_files(&spec, &config(), &host()),
        Err(SessionError::UnderClaudeFolder("plugin folder")),
        "a plugin folder under .claude, in any case and with Windows separators, is refused"
    );
}

#[test]
fn a_session_without_its_endpoints_does_not_start() {
    let cases: [(&str, fn(&mut SessionSpec)); 4] = [
        ("hook port", |spec| spec.hook_endpoint.port = 0),
        ("hook token", |spec| spec.hook_endpoint.token = SessionToken::new(String::new())),
        ("shim pipe", |spec| spec.shim.pipe.clear()),
        ("shim token", |spec| spec.shim.token = SessionToken::new(String::new())),
    ];
    for (what, empty) in cases {
        let mut spec = coding_worker();
        empty(&mut spec);
        assert_eq!(
            session_files(&spec, &config(), &host()),
            Err(SessionError::EndpointMissing(what)),
            "{what}"
        );
    }
}

#[test]
fn refused_specs_and_configs() {
    let mut spec = brain();
    spec.env = EnvPolicy::WorkerDenylist;
    assert_eq!(
        session_files(&spec, &config(), &host()),
        Err(SessionError::BrainNeedsAllowlist),
        "the brain never gets the user's environment"
    );
    let mut spec = computer_use_worker();
    spec.extra_mcp_servers[0].name = "eileen".to_owned();
    assert_eq!(
        session_files(&spec, &config(), &host()),
        Err(SessionError::ReservedServerName),
        "no server takes Eileen's name"
    );
    let mut spec = computer_use_worker();
    spec.extra_mcp_servers.push(spec.extra_mcp_servers[0].clone());
    assert_eq!(
        session_files(&spec, &config(), &host()),
        Err(SessionError::DuplicateServerName),
        "two servers with one name"
    );
    let mut cfg = config();
    cfg.data_dir = PathBuf::from("data");
    assert_eq!(
        session_files(&brain(), &cfg, &host()),
        Err(SessionError::RelativeDataDir),
        "a relative data folder"
    );
}

#[test]
fn the_denylist_matches_any_case_and_spares_the_pass_through_list() {
    for name in ["ANTHROPIC_API_KEY", "anthropic_base_url", "Claude_Config_Dir", "MCP_TIMEOUT"] {
        assert!(is_denylisted(name), "{name}");
    }
    for name in ["GH_TOKEN", "GITHUB_TOKEN", "AWS_REGION", "MY_API_KEY", "X_SECRET", "Y_TOKEN"] {
        assert!(is_denylisted(name), "{name}");
    }
    for name in PASS_THROUGH.iter().copied().chain(["https_proxy", "PATH", "EDITOR", "TOKENS"]) {
        assert!(!is_denylisted(name), "{name}");
    }
}
