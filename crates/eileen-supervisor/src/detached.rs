//! The guard's spawn: the one child started outside every Eileen Job Object, so it survives
//! the core (section 2.1 rule 1).
//!
//! On Windows it is created with `CREATE_BREAKAWAY_FROM_JOB`, which matters only when Eileen
//! itself runs inside someone else's job; when that job refuses breakaway (access denied) it
//! falls back to a plain spawn. It gets null standard handles, so nothing ties its lifetime
//! to the core's pipes, and no console window. On Unix it leads a new process group.

use std::io;
use std::process::Stdio;

use crate::SupervisorError;
use crate::child::{ChildSpec, base_command};

/// A detached child: its pid, and whether it left the parent's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detached {
    /// Its process id.
    pub pid: u32,
    /// True when `CREATE_BREAKAWAY_FROM_JOB` was honoured; false after the fallback (or off
    /// Windows, where there is no job to leave).
    pub broke_away: bool,
}

/// Starts `spec` detached: outside Eileen's Job Objects, with null standard handles.
pub fn spawn_detached(spec: &ChildSpec) -> Result<Detached, SupervisorError> {
    let error = |source: io::Error| SupervisorError::Spawn {
        program: spec.program.display().to_string(),
        source,
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;

        use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
        use windows::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW};

        let start = |flags: u32| {
            let mut command = base_command(spec);
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(flags);
            command.spawn()
        };
        match start((CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB).0) {
            Ok(child) => Ok(Detached { pid: child.id(), broke_away: true }),
            Err(e) if e.raw_os_error() == i32::try_from(ERROR_ACCESS_DENIED.0).ok() => {
                let child = start(CREATE_NO_WINDOW.0).map_err(error)?;
                Ok(Detached { pid: child.id(), broke_away: false })
            }
            Err(e) => Err(error(e)),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;

        let mut command = base_command(spec);
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
        let child = command.spawn().map_err(error)?;
        Ok(Detached { pid: child.id(), broke_away: false })
    }
}
