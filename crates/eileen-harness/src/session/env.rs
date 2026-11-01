//! The two environment policies of section 2.12, and the variables every session adds.
//!
//! Names are compared without regard to ASCII case, because Windows treats `Path` and `PATH` as
//! one variable: a denylisted name is caught in any case, and a session variable replaces every
//! spelling of its name instead of sitting beside it.

use std::collections::BTreeMap;
use std::fmt;

use eileen_contracts::harness::{EnvPolicy, SessionSpec};
use eileen_contracts::jobs::JobKind;

use crate::HarnessConfig;

/// The session's id, which the hooks send as `X-Eileen-Session`.
pub(super) const SESSION_ID: &str = "EILEEN_SESSION_ID";
/// The hook endpoint's bearer token.
pub(super) const HOOK_TOKEN: &str = "EILEEN_HOOK_TOKEN";
/// The shim's token for the dispatcher pipe.
pub(super) const SHIM_TOKEN: &str = "EILEEN_SHIM_TOKEN";

/// What the brain's environment may hold besides the session's own variables (section 2.12).
pub const BRAIN_ALLOWLIST: [&str; 16] = [
    "SystemRoot",
    "SystemDrive",
    "windir",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "PATH",
    "PATHEXT",
    "COMSPEC",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
];

/// What survives the worker denylist (section 2.12): the CLI finds Git Bash through the first,
/// and the rest carry the user's proxy and certificate settings.
pub const PASS_THROUGH: [&str; 5] =
    ["CLAUDE_CODE_GIT_BASH_PATH", "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY", "NODE_EXTRA_CA_CERTS"];

/// The prefixes of the worker denylist: every vendor's variables and every AWS variable.
const DENIED_PREFIXES: [&str; 8] =
    ["CLAUDE", "ANTHROPIC", "MCP_", "OPENAI", "CODEX_", "XAI_", "DEEPSEEK", "AWS_"];

/// The suffixes of the worker denylist: anything named like a key, token or secret.
const DENIED_SUFFIXES: [&str; 3] = ["_API_KEY", "_TOKEN", "_SECRET"];

/// The names the worker denylist removes on their own.
const DENIED_NAMES: [&str; 2] = ["GH_TOKEN", "GITHUB_TOKEN"];

/// Git configuration every worker gets, at command scope, which outranks the repository's: no
/// fsmonitor hook runs, and git never adopts a bare repository it finds (section 2.12, "Git
/// runs code too").
const GIT_CONFIG: [(&str, &str); 5] = [
    ("GIT_CONFIG_COUNT", "2"),
    ("GIT_CONFIG_KEY_0", "core.fsmonitor"),
    ("GIT_CONFIG_VALUE_0", "false"),
    ("GIT_CONFIG_KEY_1", "safe.bareRepository"),
    ("GIT_CONFIG_VALUE_1", "explicit"),
];

/// The separator between folders in `PATH`.
const PATH_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

/// What the runtime read from the user's machine for a spawn. The harness reads none of it
/// itself, so the policies are tested without touching the process's environment.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct HostEnv {
    /// The user's environment, as the runtime read it.
    pub vars: Vec<(String, String)>,
    /// System plus user `PATH` read from the registry, so a freshly installed toolchain is
    /// seen. A worker's `PATH` when present; otherwise the user's own.
    pub registry_path: Option<String>,
    /// The brain's constructed `PATH`: the system folders, git and the claude install folder.
    /// The brain's `PATH` when present; otherwise the user's own.
    pub brain_path: Option<String>,
    /// Toolchain folders the runtime found (`%USERPROFILE%\.cargo\bin`, Node, uv), put in front
    /// of a coding session's `PATH` when it lacks them.
    pub toolchain_dirs: Vec<String>,
}

/// Names only: the values can hold the user's secrets.
impl fmt::Debug for HostEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostEnv")
            .field("vars", &self.vars.iter().map(|(name, _)| name).collect::<Vec<_>>())
            .field("registry_path", &self.registry_path.is_some())
            .field("brain_path", &self.brain_path.is_some())
            .field("toolchain_dirs", &self.toolchain_dirs.len())
            .finish()
    }
}

/// A session's whole environment. `Debug` shows the names only, because the values include the
/// session's tokens.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SessionEnv(BTreeMap<String, String>);

impl SessionEnv {
    /// The value of `name`, in any ASCII case.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The variables, in name order, for the spawn.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value.as_str()))
    }

    /// The names, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// Sets `name`, replacing it in every ASCII case.
    fn set(&mut self, name: &str, value: String) {
        self.0.retain(|key, _| !key.eq_ignore_ascii_case(name));
        self.0.insert(name.to_owned(), value);
    }
}

impl fmt::Debug for SessionEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SessionEnv").field(&self.0.keys().collect::<Vec<_>>()).finish()
    }
}

/// Whether the worker denylist removes `name` (section 2.12). The pass-through list is not
/// denylisted.
#[must_use]
pub fn is_denylisted(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if PASS_THROUGH.contains(&upper.as_str()) {
        return false;
    }
    DENIED_NAMES.contains(&upper.as_str())
        || DENIED_PREFIXES.iter().any(|prefix| upper.starts_with(prefix))
        || DENIED_SUFFIXES.iter().any(|suffix| upper.ends_with(suffix))
}

/// `MCP_TOOL_TIMEOUT` for a hold of `hook_hold_s`: the hold plus 30 s, in milliseconds, the
/// backstop on an `approve` call held while the user decides (contract report, finding 8).
#[must_use]
pub fn mcp_tool_timeout_ms(hook_hold_s: u16) -> u32 {
    (u32::from(hook_hold_s) + 30) * 1000
}

pub(super) fn session_env(spec: &SessionSpec, cfg: &HarnessConfig, host: &HostEnv) -> SessionEnv {
    let mut env = match spec.env {
        EnvPolicy::WorkerDenylist => worker_base(spec, host),
        EnvPolicy::ConstructedAllowlist => brain_base(host),
    };
    add_session_vars(&mut env, spec, cfg);
    env
}

fn worker_base(spec: &SessionSpec, host: &HostEnv) -> SessionEnv {
    let mut env = SessionEnv::default();
    for (name, value) in &host.vars {
        if !is_denylisted(name) {
            env.set(name, value.clone());
        }
    }
    if let Some(path) = &host.registry_path {
        env.set("PATH", path.clone());
    }
    if spec.kind == JobKind::Coding {
        let path = env.get("PATH").unwrap_or_default().to_owned();
        let missing: Vec<&str> = host
            .toolchain_dirs
            .iter()
            .map(String::as_str)
            .filter(|dir| !path.split(PATH_SEPARATOR).any(|part| part.eq_ignore_ascii_case(dir)))
            .collect();
        if !missing.is_empty() {
            let separator = PATH_SEPARATOR.to_string();
            let mut parts = missing;
            if !path.is_empty() {
                parts.push(&path);
            }
            env.set("PATH", parts.join(&separator));
        }
    }
    for (name, value) in GIT_CONFIG {
        env.set(name, value.to_owned());
    }
    env
}

fn brain_base(host: &HostEnv) -> SessionEnv {
    let mut env = SessionEnv::default();
    for (name, value) in &host.vars {
        if BRAIN_ALLOWLIST.iter().any(|allowed| allowed.eq_ignore_ascii_case(name)) {
            env.set(name, value.clone());
        }
    }
    if let Some(path) = &host.brain_path {
        env.set("PATH", path.clone());
    }
    env
}

/// The session's own variables, which every policy adds last, so a user's variable of the same
/// name never survives.
fn add_session_vars(env: &mut SessionEnv, spec: &SessionSpec, cfg: &HarnessConfig) {
    env.set("ENABLE_CLAUDEAI_MCP_SERVERS", "false".to_owned());
    // D77: vendor memory stays off in every session until the memory gate passes.
    env.set("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1".to_owned());
    env.set("CLAUDE_CODE_DISABLE_FILE_CHECKPOINTING", "1".to_owned());
    env.set(SESSION_ID, spec.session.to_string());
    env.set(HOOK_TOKEN, spec.hook_endpoint.token.expose().to_owned());
    env.set(SHIM_TOKEN, spec.shim.token.expose().to_owned());
    env.set("MCP_TOOL_TIMEOUT", mcp_tool_timeout_ms(cfg.hook_hold_s).to_string());
    // The span of the spawn is the session's own: the low 64 bits of its ULID.
    let span = u64::try_from(spec.session.ulid().0 & u128::from(u64::MAX)).unwrap_or(u64::MAX);
    if let Some(traceparent) = spec.trace.traceparent(span) {
        env.set("TRACEPARENT", traceparent);
    }
    if let Some(slot) = spec.build_slot {
        env.set("CARGO_BUILD_JOBS", slot.cargo_jobs.to_string());
    }
}
