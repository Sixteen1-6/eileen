//! The Win32 calls behind the `ipc` module's pipes: an instance made with a DACL that grants
//! only the current user, the process id of each end, and the precise system clock.

use std::ffi::c_void;
use std::io;
use std::os::windows::io::{AsRawHandle as _, BorrowedHandle, FromRawHandle as _, OwnedHandle};

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, GetLengthSid, GetTokenInformation,
    InitializeAcl, InitializeSecurityDescriptor, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
    SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR, SetSecurityDescriptorControl,
    SetSecurityDescriptorDacl, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::Storage::FileSystem::{FILE_GENERIC_READ, FILE_GENERIC_WRITE};
use windows::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::Win32::System::SystemServices::SECURITY_DESCRIPTOR_REVISION;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// The name [`create_instance`] reports when the pipe itself could not be made, which is
/// where a taken name shows.
pub(crate) const CREATE: &str = "CreateNamedPipeW";

/// What the one ACE grants the user: reading and writing the pipe. Writing includes
/// `FILE_CREATE_PIPE_INSTANCE`, which the server needs for its next instance and which a
/// DACL cannot keep from the user's other processes (BUILD.md section 2.12).
const USER_RIGHTS: u32 = FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0;

/// An OS call that failed, by name.
#[derive(Debug)]
pub(crate) struct Fault {
    pub(crate) call: &'static str,
    pub(crate) source: io::Error,
}

fn fault(call: &'static str) -> impl FnOnce(windows::core::Error) -> Fault {
    move |error| Fault { call, source: os_error(error) }
}

/// The `windows` crate's error as the Win32 code it wraps, so that every `io::Error` this
/// module returns carries a code callers can compare with the `ERROR_` constants, as tokio's
/// do. An `HRESULT` that wraps no Win32 code is kept as it is.
pub(crate) fn os_error(error: windows::core::Error) -> io::Error {
    let hresult = error.code().0.cast_unsigned();
    // Facility 7, FACILITY_WIN32, with the failure bit: 0x8007xxxx holds the Win32 code xxxx.
    if hresult >> 16 == 0x8007
        && let Ok(win32) = i32::try_from(hresult & 0xFFFF)
    {
        io::Error::from_raw_os_error(win32)
    } else {
        io::Error::from(error)
    }
}

/// The current user's SID, inside the `TOKEN_USER` the OS wrote it into.
struct UserSid {
    /// 64-bit words, so the `TOKEN_USER` at the front is aligned: 128 bytes hold it (16) with
    /// the longest SID there is (68). Boxed, because the `TOKEN_USER` points at the SID behind
    /// it by address, so the buffer must stay where the OS filled it.
    token_user: Box<[u64; 16]>,
}

impl UserSid {
    fn current() -> Result<Self, Fault> {
        // SAFETY: GetCurrentProcess returns a pseudo-handle for this process; it takes nothing
        // and cannot fail.
        let process = unsafe { GetCurrentProcess() };
        let mut token = HANDLE::default();
        // SAFETY: the pseudo-handle is valid for this process and `token` is a writable HANDLE.
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) }
            .map_err(fault("OpenProcessToken"))?;
        // SAFETY: OpenProcessToken just returned this handle to this process, which owns it
        // alone.
        let token = unsafe { OwnedHandle::from_raw_handle(token.0) };
        let mut token_user = Box::new([0_u64; 16]);
        let size = u32::try_from(size_of::<[u64; 16]>()).unwrap_or(0);
        let mut written: u32 = 0;
        // SAFETY: the token is open with query access for as long as `token` lives, the buffer
        // is writable for `size` bytes and aligned for a TOKEN_USER, and `written` is a
        // writable u32.
        unsafe {
            GetTokenInformation(
                HANDLE(token.as_raw_handle()),
                TokenUser,
                Some(token_user.as_mut_ptr().cast()),
                size,
                &raw mut written,
            )
        }
        .map_err(fault("GetTokenInformation"))?;
        Ok(Self { token_user })
    }

    /// A pointer to the SID, valid while `self` lives.
    fn as_psid(&self) -> PSID {
        let user: *const TOKEN_USER = self.token_user.as_ptr().cast();
        // SAFETY: `current` returned only after GetTokenInformation wrote a TOKEN_USER at the
        // front of this buffer, which is aligned for one and lives as long as `self`.
        unsafe { (*user).User.Sid }
    }
}

/// A DACL with one ACE, which allows the user `sid` [`USER_RIGHTS`], as 32-bit words so the
/// ACL is aligned.
fn user_only_acl(user: &UserSid) -> Result<Vec<u32>, Fault> {
    let sid = user.as_psid();
    // SAFETY: `sid` points at the valid SID inside `user`, which the borrow keeps alive.
    let sid_len = unsafe { GetLengthSid(sid) };
    // An ACL header, then an ACE whose last field is where the SID starts.
    let head = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>();
    let bytes = u32::try_from(head).unwrap_or(u32::MAX).saturating_add(sid_len);
    let mut acl = vec![0_u32; usize::try_from(bytes.div_ceil(4)).unwrap_or(0)];
    let size = u32::try_from(size_of_val(acl.as_slice())).unwrap_or(0);
    // SAFETY: `acl` is writable for `size` bytes and aligned for an ACL.
    unsafe { InitializeAcl(acl.as_mut_ptr().cast(), size, ACL_REVISION) }
        .map_err(fault("InitializeAcl"))?;
    // SAFETY: `acl` is the ACL initialised above, with room for this ACE, and `sid` is the
    // valid SID inside `user`.
    unsafe { AddAccessAllowedAce(acl.as_mut_ptr().cast(), ACL_REVISION, USER_RIGHTS, sid) }
        .map_err(fault("AddAccessAllowedAce"))?;
    Ok(acl)
}

/// Makes one instance of the pipe at `path` on the current tokio runtime: remote clients
/// rejected, and a protected DACL that grants only the current user. `first` asks for the
/// pipe's first instance, which fails when the pipe exists. `max_instances` holds the pipe to
/// that many instances (under 255); `None` leaves the count to the OS.
pub(crate) fn create_instance(
    path: &str,
    first: bool,
    max_instances: Option<usize>,
) -> Result<NamedPipeServer, Fault> {
    let sid = UserSid::current()?;
    let acl = user_only_acl(&sid)?;
    let mut descriptor = SECURITY_DESCRIPTOR::default();
    let descriptor_ptr = PSECURITY_DESCRIPTOR((&raw mut descriptor).cast());
    // SAFETY: `descriptor` is a writable SECURITY_DESCRIPTOR.
    unsafe { InitializeSecurityDescriptor(descriptor_ptr, SECURITY_DESCRIPTOR_REVISION) }
        .map_err(fault("InitializeSecurityDescriptor"))?;
    // SAFETY: `descriptor` was initialised above, and `acl` is a valid ACL that outlives every
    // use of the descriptor, which keeps only a pointer to it.
    unsafe { SetSecurityDescriptorDacl(descriptor_ptr, true, Some(acl.as_ptr().cast()), false) }
        .map_err(fault("SetSecurityDescriptorDacl"))?;
    // Protected: the pipe takes no inheritable ACE from anywhere else.
    // SAFETY: `descriptor` is the initialised descriptor above.
    unsafe { SetSecurityDescriptorControl(descriptor_ptr, SE_DACL_PROTECTED, SE_DACL_PROTECTED) }
        .map_err(fault("SetSecurityDescriptorControl"))?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: descriptor_ptr.0,
        bInheritHandle: false.into(),
    };
    let mut options = ServerOptions::new();
    options.first_pipe_instance(first).reject_remote_clients(true);
    if let Some(max) = max_instances {
        options.max_instances(max);
    }
    let attributes_ptr: *mut c_void = (&raw mut attributes).cast();
    // SAFETY: `attributes_ptr` points at a SECURITY_ATTRIBUTES whose descriptor, ACL and SID
    // are the locals above, all alive across the call, which copies what it needs.
    unsafe { options.create_with_security_attributes_raw(path, attributes_ptr) }
        .map_err(|source| Fault { call: CREATE, source })
}

/// The process id of the client end of the pipe `pipe` is an end of.
pub(crate) fn client_pid(pipe: BorrowedHandle<'_>) -> io::Result<u32> {
    let mut pid: u32 = 0;
    // SAFETY: the borrow keeps the pipe handle open across the call, and `pid` is writable.
    unsafe { GetNamedPipeClientProcessId(HANDLE(pipe.as_raw_handle()), &raw mut pid) }
        .map_err(os_error)?;
    Ok(pid)
}

/// The process id of the server end of the pipe `pipe` is an end of.
pub(crate) fn server_pid(pipe: BorrowedHandle<'_>) -> io::Result<u32> {
    let mut pid: u32 = 0;
    // SAFETY: the borrow keeps the pipe handle open across the call, and `pid` is writable.
    unsafe { GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &raw mut pid) }
        .map_err(os_error)?;
    Ok(pid)
}

/// The system clock as a file time: 100 ns steps since 1601-01-01 UTC, the unit of a
/// process's creation time.
pub(crate) fn now_filetime() -> u64 {
    // SAFETY: GetSystemTimePreciseAsFileTime takes nothing and cannot fail.
    let now = unsafe { GetSystemTimePreciseAsFileTime() };
    (u64::from(now.dwHighDateTime) << 32) | u64::from(now.dwLowDateTime)
}

/// Whether the DACL of the kernel object behind `object` is protected and holds exactly one
/// ACE, which allows the current user exactly [`USER_RIGHTS`] and is inherited by nothing.
/// The tests read a pipe's DACL back with it.
#[cfg(test)]
pub(crate) fn grants_only_user(object: BorrowedHandle<'_>) -> Result<bool, Fault> {
    use windows::Win32::Security::{
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetKernelObjectSecurity,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
    };
    use windows::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
    use windows::core::BOOL;

    let user = UserSid::current()?;
    // 64-bit words, so the self-relative descriptor the OS writes is aligned.
    let mut descriptor = [0_u64; 64];
    let descriptor_ptr = PSECURITY_DESCRIPTOR(descriptor.as_mut_ptr().cast());
    let size = u32::try_from(size_of_val(&descriptor)).unwrap_or(0);
    let mut needed: u32 = 0;
    // SAFETY: the borrow keeps the handle open, the buffer is writable for `size` bytes and
    // `needed` is a writable u32.
    unsafe {
        GetKernelObjectSecurity(
            HANDLE(object.as_raw_handle()),
            DACL_SECURITY_INFORMATION.0,
            Some(descriptor_ptr),
            size,
            &raw mut needed,
        )
    }
    .map_err(fault("GetKernelObjectSecurity"))?;
    let mut present = BOOL::default();
    let mut defaulted = BOOL::default();
    let mut acl: *mut ACL = std::ptr::null_mut();
    // SAFETY: the buffer holds the descriptor the call above wrote, and the three outputs are
    // writable.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor_ptr,
            &raw mut present,
            &raw mut acl,
            &raw mut defaulted,
        )
    }
    .map_err(fault("GetSecurityDescriptorDacl"))?;
    if !present.as_bool() || acl.is_null() {
        // No DACL at all grants everyone everything.
        return Ok(false);
    }
    let mut control: u16 = 0;
    let mut revision: u32 = 0;
    // SAFETY: the buffer holds a valid descriptor and both outputs are writable.
    unsafe { GetSecurityDescriptorControl(descriptor_ptr, &raw mut control, &raw mut revision) }
        .map_err(fault("GetSecurityDescriptorControl"))?;
    // SAFETY: GetSecurityDescriptorDacl set `acl` to the ACL inside the buffer, which is
    // aligned and alive.
    let aces = unsafe { (*acl).AceCount };
    if aces != 1 || control & SE_DACL_PROTECTED.0 == 0 {
        return Ok(false);
    }
    let mut ace: *mut c_void = std::ptr::null_mut();
    // SAFETY: `acl` is the valid ACL above, which holds one ACE, and `ace` is writable.
    unsafe { GetAce(acl, 0, &raw mut ace) }.map_err(fault("GetAce"))?;
    let ace: *const ACCESS_ALLOWED_ACE = ace.cast();
    // SAFETY: GetAce set `ace` to the first ACE inside the buffer; every ACE starts with a
    // header.
    let kind = unsafe { (*ace).Header.AceType };
    // SAFETY: `ace` is the same ACE, whose header also holds its flags.
    let flags = unsafe { (*ace).Header.AceFlags };
    // SAFETY: `ace` is the same ACE; every kind of ACE a DACL holds has a mask after its
    // header.
    let mask = unsafe { (*ace).Mask };
    if u32::from(kind) != ACCESS_ALLOWED_ACE_TYPE || flags != 0 || mask != USER_RIGHTS {
        return Ok(false);
    }
    // An allow ACE's SID starts at its last field.
    let ace_sid = PSID(
        ace.wrapping_byte_add(std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart)).cast_mut().cast(),
    );
    // SAFETY: `ace_sid` is the SID inside the allow ACE above and `user` holds a valid SID.
    Ok(unsafe { EqualSid(ace_sid, user.as_psid()) }.is_ok())
}
