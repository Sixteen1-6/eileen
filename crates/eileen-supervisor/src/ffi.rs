//! The supervisor's only unsafe code: the Win32 calls for quality of service and, for the
//! tests, process liveness. Every function here is safe to call; each unsafe block holds one
//! operation and says why its preconditions hold.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, GetCurrentProcess,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetProcessInformation,
};
use windows::core::HSTRING;

use crate::qos::QosError;

fn qos_error(call: &'static str, error: &windows::core::Error) -> QosError {
    QosError { call, detail: error.message() }
}

/// Opts this process out of `EcoQoS` execution-speed throttling.
pub(crate) fn opt_out_of_ecoqos() -> Result<(), QosError> {
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    };
    let size = u32::try_from(size_of::<PROCESS_POWER_THROTTLING_STATE>()).unwrap_or(u32::MAX);
    // SAFETY: GetCurrentProcess returns a pseudo-handle for this process; it takes nothing
    // and cannot fail.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: `state` is a PROCESS_POWER_THROTTLING_STATE that lives across the call, as the
    // ProcessPowerThrottling class requires, and `size` is its size.
    unsafe {
        SetProcessInformation(process, ProcessPowerThrottling, (&raw const state).cast(), size)
    }
    .map_err(|e| qos_error("SetProcessInformation", &e))
}

/// A thread's MMCSS registration, reverted on drop.
#[derive(Debug)]
pub(crate) struct MmcssHandle(HANDLE);

impl MmcssHandle {
    /// Joins the calling thread to the MMCSS task `task`.
    pub(crate) fn join(task: &str) -> Result<Self, QosError> {
        let name = HSTRING::from(task);
        let mut index: u32 = 0;
        // SAFETY: `name` is a NUL-terminated wide string that outlives the call, and `index`
        // is a writable u32 the call may fill.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(&name, &raw mut index) }
            .map_err(|e| qos_error("AvSetMmThreadCharacteristicsW", &e))?;
        Ok(Self(handle))
    }
}

impl Drop for MmcssHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from AvSetMmThreadCharacteristicsW on this thread (the boost
        // that owns it is not Send, on any OS) and is reverted exactly once, here.
        let _reverted = unsafe { AvRevertMmThreadCharacteristics(self.0) };
    }
}

/// Process liveness, termination and waiting, for the tests that check what a Job Object
/// kills.
#[cfg(test)]
pub(crate) mod process {
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
    use std::time::Duration;

    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, HANDLE, WAIT_EVENT, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::System::Threading::{
        ExitProcess, GetExitCodeProcess, OpenProcess, PROCESS_ACCESS_RIGHTS,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        TerminateProcess, WaitForSingleObject,
    };
    use windows::core::HRESULT;

    /// Opens `pid` so its end can be waited on and its exit code read; `None` when no such
    /// process runs.
    pub(crate) fn open_for_wait(pid: u32) -> Option<OwnedHandle> {
        open(pid, PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION).ok()
    }

    /// Waits up to `timeout` for the process behind `process` to end; true once it has.
    pub(crate) fn wait_exit(process: &OwnedHandle, timeout: Duration) -> bool {
        wait(process, timeout) == WAIT_OBJECT_0
    }

    /// Waits up to `timeout` on a handle open with synchronize access.
    fn wait(process: &OwnedHandle, timeout: Duration) -> WAIT_EVENT {
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: the handle is open with synchronize access for as long as `process` lives.
        unsafe { WaitForSingleObject(HANDLE(process.as_raw_handle()), ms) }
    }

    /// Opens a process with `rights`, owning the handle.
    fn open(pid: u32, rights: PROCESS_ACCESS_RIGHTS) -> windows::core::Result<OwnedHandle> {
        // SAFETY: OpenProcess only reads its arguments; the handle is not inheritable.
        let handle = unsafe { OpenProcess(rights, false, pid) }?;
        // SAFETY: OpenProcess just returned this handle to this process, which owns it alone.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
    }

    /// Whether the process `pid` is still running: it is open to waiting and has not ended,
    /// or it exists but may not be opened. It waits with a zero timeout rather than reading the
    /// exit code, because a process may exit with 259, which is `STILL_ACTIVE` (the build
    /// guard's `procs::process_alive` has the same rule).
    pub(crate) fn alive(pid: u32) -> bool {
        match open(pid, PROCESS_SYNCHRONIZE) {
            Ok(process) => wait(&process, Duration::ZERO) == WAIT_TIMEOUT,
            // A process we may not open still exists; a missing one gives ERROR_INVALID_PARAMETER.
            Err(e) => e.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0),
        }
    }

    /// The exit code of the ended process behind `process`, which `open_for_wait` opened.
    pub(crate) fn exit_code(process: &OwnedHandle) -> Option<u32> {
        let mut code: u32 = 0;
        // SAFETY: the handle is open with query access and `code` is writable.
        unsafe { GetExitCodeProcess(HANDLE(process.as_raw_handle()), &raw mut code) }.ok()?;
        Some(code)
    }

    /// Ends this process at once with `code`, for the helper that exits with 259.
    pub(crate) fn exit_now(code: u32) -> ! {
        // SAFETY: ExitProcess takes a plain exit code and ends the calling process.
        unsafe { ExitProcess(code) }
    }

    /// Terminates the process `pid`.
    pub(crate) fn terminate(pid: u32) -> bool {
        let Ok(process) = open(pid, PROCESS_TERMINATE) else { return false };
        // SAFETY: the handle is open with terminate access.
        unsafe { TerminateProcess(HANDLE(process.as_raw_handle()), 1) }.is_ok()
    }
}
