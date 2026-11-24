//! How Eileen starts every child process (BUILD.md section 2.1): supervised in a kill-on-close
//! Job Object with no console window and stderr into tracing ([`spawn`]), the guard's detached
//! spawn ([`spawn_detached`]), deadlines, heartbeats and bounded restarts ([`watch`]), and
//! the scheduling opt-outs every process makes at start ([`qos`]), and the pipes between
//! Eileen's processes with the process id of each end ([`ipc`], section 3.1, decision D86).
//!
//! # Owns
//!
//! - The one `Command::new` for every child of the running product, and how it is wrapped (on
//!   Linux, LINUX.md section 5: own session, parent-death signal, group kills).
//! - [`spawn_detached`] for `eileen-guard.exe`: `CREATE_BREAKAWAY_FROM_JOB`, null handles.
//! - [`watch`] (deadlines, heartbeats, restart backoff) and [`qos`] (`EcoQoS`, MMCSS).
//! - [`ipc`]: [`ipc::PipeServer`], which makes a pipe as its first instance for the current user
//!   only and keeps an instance listening, and [`ipc::PipeConn`], one connection with both ends'
//!   process ids and the moment it began. The caller checks, the Job Object registry and the
//!   user-only token files join it in `wp2/supervisor-callers`.
//!
//! # Does not own
//!
//! - Launching apps for the user (`user_launch`, phase B), the guard itself (`eileen-guard`), cargo
//!   under the build guard (`eileen-buildguard::spawn_cargo`), worker flags (`eileen-harness`).
//! - Which caller a pipe accepts, and the incident for a pipe that cannot make its next
//!   instance: `ipc` reports facts and errors, and `eileen-runtime` decides and reports.
//!
//! # Invariants
//!
//! - Killing a child's Job Object kills its grandchildren (Windows test
//!   `tests::killing_the_job_kills_the_grandchild`).
//! - [`spawn`] forwards stderr at once, holding no runtime thread, so a full pipe never blocks a
//!   child (`tests::a_chatty_child_exits_*`, `tests::a_quiet_child_leaves_the_blocking_pool_free`);
//!   it survives bytes that are not UTF-8 and keeps at most [`MAX_STDERR_LINE`] bytes of a line
//!   (`tests::hostile_stderr_keeps_forwarding_with_lines_cut`).
//! - On Linux a child leads its own session, and its whole group dies before the reap: on drop,
//!   on `wait`, on a `try_wait` that reaps (which forgets the pid), and with the process that
//!   spawned it, from any thread; a Claude session runs ten nice steps below its forking thread
//!   (a test of `tests::linux` for each).
//! - The detached spawn outlives its parent (on Windows) and is reaped once it exits (on Unix):
//!   `tests::the_guard_spawn_outlives_its_parent` and `tests::linux::the_guard_spawn_is_reaped`.
//! - A failing child restarts at most `max_restarts` times per window
//!   (`watch::tests::restarts_back_off_and_give_up_inside_the_window`).
//! - Unsafe code only in the private `ffi` module (on Linux `src/ffi/linux.rs`), one operation
//!   per block (the `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` lints, and
//!   check 10).
//! - A child's stderr reaches the log files only as length and hash, in `pii.line` (test
//!   `tests::stderr_lines_become_child_stderr_events`), and a [`ChildSpec`]'s `Debug` names its
//!   environment without the values (`tests::a_spec_never_shows_its_environment_values_in_debug`).
//!
//! - A pipe exists only as its first instance, with remote clients rejected and a protected
//!   DACL whose one ACE allows the current user, on every instance
//!   (`tests::ipc::win::bind_refuses_a_name_that_exists`,
//!   `tests::ipc::win::every_instance_grants_only_the_current_user`).
//! - [`ipc::PipeServer::accept`] hands a connection over only once the instance that listens in
//!   its place exists; when that instance cannot be made it says so and keeps the connection
//!   for the next call (`tests::ipc::win::an_instance_listens_while_connections_are_open`,
//!   `tests::ipc::win::a_next_instance_that_cannot_be_made_is_an_error_and_keeps_the_connection`).
//! - Off Windows every `ipc` constructor answers [`ipc::IpcError::Unsupported`]
//!   (`tests::ipc::every_call_answers_unsupported_off_windows`).
//!
//! # Boundaries
//!
//! - Called by `eileen-runtime`, `eileen-harness`, `eileen-scenarios`, the sidecars.
//! - May depend on `eileen-rt`, `eileen-stdx`, `eileen-trace`, `process-wrap`, `thiserror`,
//!   `tokio` (its `net` feature for the named pipes) and `tracing`; `windows` on Windows;
//!   `rustix` on Linux.
//! - Wire types: none; the protocols on the pipes belong to their crates.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-supervisor`.
//! - Layers: unit tests for the watch logic; process tests that run the test binary again
//!   as a helper child (Job Object semantics on Windows, sessions, process groups and parent
//!   death on Linux, stderr forwarding on both); real named pipes on Windows, with the helper
//!   child as the client whose process id the server reads.
//! - A bug here starts as an orphaned process after Eileen exits, or a restart storm.
//!
//! # Traps
//!
//! - A [`Child`] kills its tree when dropped (its Job Object, or on Linux its process group,
//!   which a descendant can leave, or outlive in a crash): keep it while the child should live.
//! - On Linux [`Child::wait`] kills what is left of the group once the child exits, where
//!   Windows waits for the whole job; a set-user-ID program loses its parent-death signal.
//! - [`spawn`] needs a tokio runtime with I/O (process driver, stderr task, Linux pidfd): with
//!   none it fails before the child starts, without I/O it panics or fails after (see [`spawn`]).
//!   A [`ChildSpec::raw_stderr`] caller must read stderr.
//! - On Linux a nice value orders a child only within its CPU cgroup or autogroup (`child.rs`).
//! - The detached spawn falls back to a plain spawn when the parent's job refuses breakaway
//!   ([`Detached::broke_away`] says which); its handles are null, so it cannot log to stderr.
//! - Paused-time tests need tokio's `test-util`, which is not in the workspace feature union.
//! - A pipe's DACL cannot keep the user's other processes from adding instances, so a client
//!   may reach a server that is not the core: it checks [`ipc::PipeConn::server_pid`] before it
//!   sends anything (BUILD.md section 2.12).
//! - [`ipc::PipeServer::bind`] and [`ipc::PipeConn::connect`] need a tokio runtime with I/O.
//!   `connect` never waits: a busy or missing pipe is an error the caller retries under its own
//!   deadline. A client that connects before `accept` is called is stamped when `accept` sees
//!   it, so call `accept` again as soon as it returns.
//! - [`qos::AudioBoost`] is not `Send` on any OS: MMCSS is reverted on the thread that joined.
//!
//! # Logs and tracing
//!
//! - Emits `child.spawned`, `child.stderr` (the line in `pii.line`), `child.exited` and
//!   `deadline.passed`; the restart span `supervisor.restart` belongs to the runtime that drives
//!   [`watch::Restarts`].
//! - `ipc` emits nothing: its errors go to the caller, which logs them and raises the incident.
//!
//! # Do not change without a decision record
//!
//! - The one `#[expect]`ed `Command::new` and the flags every child gets (on Linux its own
//!   session, its parent-death signal and the fork thread); the breakaway rule.

mod child;
mod detached;
#[cfg(windows)]
#[expect(unsafe_code, reason = "the Win32 calls for EcoQoS, MMCSS, pipes and process liveness")]
mod ffi;
#[cfg(target_os = "linux")]
#[expect(unsafe_code, reason = "a Linux child's setup between fork and exec, and its pidfd")]
#[path = "ffi/linux.rs"]
mod ffi;
pub mod ipc;
#[cfg(target_os = "linux")]
mod linux;
pub mod qos;
#[cfg(test)]
mod tests;
pub mod watch;

pub use child::{Child, ChildClass, ChildSpec, MAX_STDERR_LINE, spawn};
pub use detached::{Detached, spawn_detached};

/// Why a child could not be started.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    /// The OS refused to start it.
    #[error("could not start {program}: {source}")]
    Spawn {
        /// The program.
        program: String,
        /// The OS's error.
        source: std::io::Error,
    },
}
