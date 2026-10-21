//! The build guard's only unsafe code: the Win32 calls for memory, paging, disk, the cargo
//! Job Object, process liveness and waiting for a process to end. Every function here is safe
//! to call; each unsafe block holds one operation and says why its preconditions hold.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle, RawHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Performance::{
    PDH_CALC_NEGATIVE_DENOMINATOR, PDH_CALC_NEGATIVE_VALUE, PDH_CSTATUS_INVALID_DATA,
    PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_INVALID_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW,
};
use windows_sys::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows_sys::Win32::System::SystemInformation::{
    GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenThread, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, ResumeThread,
    THREAD_SUSPEND_RESUME, WaitForSingleObject,
};

/// The size of `T` as the `u32` Win32 structure headers take.
fn size_of_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}

/// `text` as NUL-terminated UTF-16.
fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

/// Takes ownership of a handle a Win32 call just returned, or reports the call's error.
fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the caller passes a handle a Win32 call has just returned to this process, which
    // nothing else owns or closes, and which was checked to be neither null nor invalid.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Available physical memory (free plus standby), bytes.
pub(crate) fn available_physical_bytes() -> io::Result<u64> {
    let mut status =
        MEMORYSTATUSEX { dwLength: size_of_u32::<MEMORYSTATUSEX>(), ..Default::default() };
    // SAFETY: `status` is a writable MEMORYSTATUSEX owned by this frame, with `dwLength` set to
    // its size as the call requires.
    let ok = unsafe { GlobalMemoryStatusEx(&raw mut status) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(status.ullAvailPhys)
}

/// The commit limit minus the commit charge, bytes.
pub(crate) fn commit_headroom_bytes() -> io::Result<u64> {
    let cb = size_of_u32::<PERFORMANCE_INFORMATION>();
    let mut info = PERFORMANCE_INFORMATION { cb, ..Default::default() };
    // SAFETY: `info` is a writable PERFORMANCE_INFORMATION owned by this frame and `cb` is its
    // size.
    let ok = unsafe { GetPerformanceInfo(&raw mut info, cb) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let pages =
        u64::try_from(info.CommitLimit.saturating_sub(info.CommitTotal)).unwrap_or(u64::MAX);
    let page = u64::try_from(info.PageSize).unwrap_or(4096);
    Ok(pages.saturating_mul(page))
}

/// The system's page size, bytes.
pub(crate) fn page_size() -> u32 {
    let mut info = SYSTEM_INFO::default();
    // SAFETY: GetSystemInfo only writes the SYSTEM_INFO it is given, which this frame owns.
    unsafe { GetSystemInfo(&raw mut info) };
    if info.dwPageSize == 0 { 4096 } else { info.dwPageSize }
}

/// Converts a PDH status into a result.
fn pdh(status: u32, call: &'static str) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!("{call} failed with PDH status {status:#010x}")))
    }
}

/// The `\Memory\Pages Input/sec` counter, open for repeated reads.
#[derive(Debug)]
pub(crate) struct PagingCounter {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl PagingCounter {
    /// Opens the counter by its English name (so a localised Windows finds it too) and
    /// primes it, so the first read reports a rate.
    pub(crate) fn open() -> io::Result<Self> {
        let mut query: PDH_HQUERY = std::ptr::null_mut();
        // SAFETY: a null data source means live data; `query` is a writable slot for the handle.
        let status = unsafe { PdhOpenQueryW(std::ptr::null(), 0, &raw mut query) };
        pdh(status, "PdhOpenQueryW")?;
        let mut this = Self { query, counter: std::ptr::null_mut() };
        let path = wide(OsStr::new("\\Memory\\Pages Input/sec"));
        // SAFETY: `this.query` is open, `path` is NUL-terminated UTF-16 that outlives the call,
        // and `this.counter` is a writable slot for the counter handle.
        let status =
            unsafe { PdhAddEnglishCounterW(this.query, path.as_ptr(), 0, &raw mut this.counter) };
        pdh(status, "PdhAddEnglishCounterW")?;
        // SAFETY: `this.query` is open and holds the counter; collecting primes the rate.
        let status = unsafe { PdhCollectQueryData(this.query) };
        pdh(status, "PdhCollectQueryData")?;
        Ok(this)
    }

    /// The pages read per second since the previous read. Zero while PDH has no rate yet
    /// (two reads too close together).
    pub(crate) fn pages_per_sec(&mut self) -> io::Result<f64> {
        // SAFETY: `self.query` is open for as long as `self` lives.
        let status = unsafe { PdhCollectQueryData(self.query) };
        pdh(status, "PdhCollectQueryData")?;
        let mut value = PDH_FMT_COUNTERVALUE::default();
        // SAFETY: the counter belongs to the open query, `value` is a writable
        // PDH_FMT_COUNTERVALUE, and the type out-pointer may be null.
        let status = unsafe {
            PdhGetFormattedCounterValue(
                self.counter,
                PDH_FMT_DOUBLE,
                std::ptr::null_mut(),
                &raw mut value,
            )
        };
        if matches!(
            status,
            PDH_INVALID_DATA
                | PDH_CSTATUS_INVALID_DATA
                | PDH_CALC_NEGATIVE_DENOMINATOR
                | PDH_CALC_NEGATIVE_VALUE
        ) {
            return Ok(0.0);
        }
        pdh(status, "PdhGetFormattedCounterValue")?;
        // SAFETY: the call succeeded with PDH_FMT_DOUBLE, so it wrote the union's double member.
        Ok(unsafe { value.Anonymous.doubleValue })
    }
}

impl Drop for PagingCounter {
    fn drop(&mut self) {
        // SAFETY: the query came from PdhOpenQueryW and is closed exactly once, here; closing
        // it also frees its counter.
        let _status: u32 = unsafe { PdhCloseQuery(self.query) };
    }
}

/// Free space on the volume holding `path`, bytes, as the calling user may use it.
pub(crate) fn free_disk_bytes(path: &Path) -> io::Result<u64> {
    let dir = wide(path.as_os_str());
    let mut free: u64 = 0;
    // SAFETY: `dir` is NUL-terminated UTF-16 that outlives the call, `free` is writable, and the
    // two totals the guard does not need may be null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(dir.as_ptr(), &raw mut free, std::ptr::null_mut(), std::ptr::null_mut())
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(free)
}

/// A kill-on-close Job Object: closing its last handle, which happens when the guard's
/// process ends for any reason, terminates every process in it.
#[derive(Debug)]
pub(crate) struct Job(OwnedHandle);

impl Job {
    pub(crate) fn kill_on_close() -> io::Result<Self> {
        // SAFETY: null attributes and a null name create an unnamed job with default security.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        let job = Self(owned(handle)?);
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the job handle is open, `info` is a JOBOBJECT_EXTENDED_LIMIT_INFORMATION as
        // this information class requires, and its size is passed with it.
        let ok = unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of_u32::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// Terminates every process in the job now.
    pub(crate) fn terminate(&self) -> io::Result<()> {
        // SAFETY: the job handle is open for as long as `self` lives.
        let ok = unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Puts a process (still suspended, so it has started nothing yet) into the job.
    pub(crate) fn assign(&self, process: RawHandle) -> io::Result<()> {
        // SAFETY: the job handle is open, and `process` is the handle of a child this process
        // just created and still owns.
        let ok = unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Resumes every thread of `pid`, which was created suspended.
pub(crate) fn resume_threads(pid: u32) -> io::Result<()> {
    // SAFETY: a thread snapshot of the whole system; the process id argument is ignored.
    let snapshot = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
    let mut entry = THREADENTRY32 { dwSize: size_of_u32::<THREADENTRY32>(), ..Default::default() };
    // SAFETY: the snapshot is open and `entry` is writable with `dwSize` set.
    let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    let mut resumed = 0_u32;
    while more {
        if entry.th32OwnerProcessID == pid {
            resume_thread(entry.th32ThreadID)?;
            resumed += 1;
        }
        // SAFETY: as for Thread32First.
        more = unsafe { Thread32Next(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    }
    if resumed == 0 {
        return Err(io::Error::other(format!("no thread of process {pid} to resume")));
    }
    Ok(())
}

fn resume_thread(tid: u32) -> io::Result<()> {
    // SAFETY: opens a thread id the snapshot listed, with no handle inheritance.
    let thread = owned(unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, tid) })?;
    // SAFETY: the handle is open with THREAD_SUSPEND_RESUME access.
    let previous = unsafe { ResumeThread(thread.as_raw_handle()) };
    if previous == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Whether the process `pid` is still running: waiting on it times out at once. An exit code
/// is never read for this, because a process may exit with 259, which is `STILL_ACTIVE`.
pub(crate) fn process_alive(pid: u32) -> bool {
    match open_for_wait(pid) {
        Ok(process) => matches!(wait_exit(&process, std::time::Duration::ZERO), Ok(false)),
        // A process we may not open still exists; a missing one gives ERROR_INVALID_PARAMETER.
        Err(e) => e.raw_os_error() == i32::try_from(ERROR_ACCESS_DENIED).ok(),
    }
}

/// Opens `pid` with the right to wait for it.
pub(crate) fn open_for_wait(pid: u32) -> io::Result<OwnedHandle> {
    // SAFETY: synchronize and query access to a process id, with no handle inheritance.
    owned(unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })
}

/// Waits up to `timeout` for the process behind `process` to end; true once it has.
pub(crate) fn wait_exit(process: &OwnedHandle, timeout: std::time::Duration) -> io::Result<bool> {
    let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
    // SAFETY: the handle is open with synchronize access for as long as `process` lives.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), ms) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}
