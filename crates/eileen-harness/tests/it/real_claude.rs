//! `ClaudeWorker::start` against the signed-in claude CLI on the owner's PC (`cargo xtask test
//! --owner`; the default profiles skip `real_claude::`). It writes no user line, so no model
//! turn runs and nothing is spent: it shows that the CLI answers the readiness requests
//! (`initialize`, then `mcp_status`) before any user line, which the amendment of 2026-11-01
//! (#161) builds readiness on. The session's `eileen-shim` path names no file, so the CLI
//! reports the `eileen` server as failed and `start` refuses the session at once, instead of
//! waiting for a `system/init` that never comes.
//!
//! The init deadline stays at its default (`eileen_harness::INIT_DEADLINE`), since a cold start
//! on a loaded PC took 30 to 111 s (section 0) and a shorter one could fail a CLI that answers
//! correctly. Any deadline fails a start that waits for the init, because none comes before a
//! user line.

use std::path::PathBuf;
use std::sync::Arc;

use eileen_contracts::config::RoutingConfig;
use eileen_contracts::harness::{
    EnvPolicy, HookEndpoint, SessionSpec, SessionToken, ShimEndpoint, WorkerEvent,
};
use eileen_contracts::ids::{TraceCtx, WorkerSessionId};
use eileen_contracts::jobs::{JobKind, Route, Toolset};
use eileen_harness::locate::{LocateEnv, locate_claude};
use eileen_harness::session::HostEnv;
use eileen_harness::{ClaudeWorker, DriverError, HarnessConfig, WorkerDriver as _};
use eileen_trace::Redactor;

/// The default config's fast model: aliases live only in config (section 2.3), and the probe
/// runs no model turn, so the cheapest is enough.
fn fast_model() -> String {
    let routing = RoutingConfig::default();
    routing.models.get("fast").cloned().expect("the default config names a fast model")
}

fn spec(session: WorkerSessionId) -> SessionSpec {
    SessionSpec {
        session,
        pool: None,
        kind: JobKind::Coding,
        route: Route::Claude,
        model: fast_model(),
        effort: None,
        cwd: format!("run/{session}/cwd"),
        worktree: None,
        snapshot_ref: None,
        append_system_prompt: String::new(),
        toolsets: vec![Toolset::Worker],
        allowed_tools: Vec::new(),
        extra_mcp_servers: Vec::new(),
        builtin_tools: Vec::new(),
        disallowed_tools: Vec::new(),
        plugin_dirs: Vec::new(),
        snapshot: None,
        resume_session: None,
        chrome: false,
        env: EnvPolicy::WorkerDenylist,
        build_slot: None,
        memory_limit_mb: None,
        trace: TraceCtx::default(),
        hook_endpoint: HookEndpoint {
            port: 9,
            path: "hooks".to_owned(),
            token: SessionToken::new("hook-token-for-a-probe".to_owned()),
        },
        shim: ShimEndpoint {
            pipe: "eileen-probe-pipe".to_owned(),
            token: SessionToken::new("shim-token-for-a-probe".to_owned()),
        },
    }
}

#[tokio::test]
async fn start_is_answered_through_mcp_status_before_any_user_line() {
    let env = LocateEnv {
        path_dirs: std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default(),
        appdata: std::env::var_os("APPDATA").map(PathBuf::from),
        home: std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from),
        windows: cfg!(windows),
    };
    let install = locate_claude(&env).expect("the claude CLI is installed on this PC");
    let dir = std::env::temp_dir().join(format!("eileen-real-claude-{}", std::process::id()));
    let cfg = HarnessConfig {
        data_dir: dir.join("data"),
        shim_exe: dir.join("no-such-eileen-shim.exe"),
        claude_exe: install.exe,
        host: HostEnv {
            vars: std::env::vars().collect(),
            names_ignore_case: cfg!(windows),
            ..HostEnv::default()
        },
        ..HarnessConfig::default()
    };
    let mut worker = ClaudeWorker::new(cfg, Arc::new(Redactor::new()));
    let started = worker.start(spec(WorkerSessionId::from_parts(1_700_000_000_000, 11))).await;
    let _removed: std::io::Result<()> = std::fs::remove_dir_all(&dir);
    assert_eq!(
        started.err(),
        Some(DriverError::EileenNotConnected),
        "mcp_status answered before any user line, reporting the missing shim"
    );
    let next: Option<WorkerEvent> = worker.next_event().await;
    assert_eq!(next, None, "a refused session gives no event");
}
