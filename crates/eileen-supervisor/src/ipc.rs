//! The pipes between Eileen's processes (BUILD.md sections 2.12 and 3.1, decision D86): a
//! server that owns a pipe's name from its first instance, and one connection with the process
//! id of each end. The module reports facts and decides nothing: which caller a role or an
//! endpoint accepts is the runtime's decision over those facts.
//!
//! The types exist on every OS, so the runtime, the speech process and the shim call them with
//! no `cfg`. Off Windows every constructor answers [`IpcError::Unsupported`], so no build
//! accepts a caller it could not check; the Linux forms (LINUX.md section 5) come later behind
//! the same names.
//!
//! A pipe is made as its first instance, with remote clients rejected and a DACL that grants
//! only the current user. That keeps another process from holding the name before the core
//! starts, and nothing more: the user cannot be denied the right to add instances, so any
//! process of the user can add one later and catch a client. Every client therefore reads
//! [`PipeConn::server_pid`] and checks that process before it sends anything.
//!
//! # Invariants
//!
//! - A pipe is made only as its first instance, with remote clients rejected and a protected
//!   DACL whose one ACE allows the current user
//!   (`tests::ipc::win::bind_refuses_a_name_that_exists`,
//!   `tests::ipc::win::every_instance_grants_only_the_current_user`).
//! - [`PipeServer::accept`] hands a connection over only once the instance that listens in its
//!   place exists; when that instance cannot be made it says so and keeps the connection for
//!   the next call (`tests::ipc::win::an_instance_listens_while_connections_are_open`,
//!   `tests::ipc::win::a_next_instance_that_cannot_be_made_is_an_error_and_keeps_the_connection`).
//! - A connection names the process at each end
//!   (`tests::ipc::win::a_connection_names_the_process_at_each_end`).
//! - Off Windows every constructor answers [`IpcError::Unsupported`]
//!   (`tests::ipc::every_call_answers_unsupported_off_windows`).
//!
//! # Traps
//!
//! - Between a client's connect and the `accept` that makes the next instance, no instance of
//!   the server's listens, so a client can find the pipe busy, or reach another process's
//!   instance. [`PipeConn::connect`] never waits: the caller retries under its own deadline.
//! - A client that connects before `accept` is called is stamped when `accept` sees it, so a
//!   server keeps a task in `accept` for as long as it lives.
//! - [`PipeServer::bind`] and [`PipeConn::connect`] need a tokio runtime with I/O.
//! - The module logs nothing. Its errors go to the caller, which logs them and raises the
//!   incident for a pipe that cannot make its next instance.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[cfg(not(windows))]
mod other;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
use other as sys;
#[cfg(windows)]
use win as sys;

/// The longest pipe name [`PipeServer::bind`] and [`PipeConn::connect`] take, in bytes. Windows
/// allows 256 characters for the whole path; Eileen's names are a fixed word and an id.
pub const MAX_PIPE_NAME: usize = 128;

/// Why a pipe call failed.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// This OS has no pipes or caller checks yet, so nothing is served and nothing accepted.
    #[error("pipes and caller checks are not built for this OS")]
    Unsupported,
    /// The name is empty, longer than [`MAX_PIPE_NAME`], or holds a character other than an
    /// ASCII letter, a digit, `-` or `_`, so it could name something other than one pipe. A
    /// dot is refused too: Windows drops trailing dots and resolves `.` and `..` before the
    /// pipe file system sees the path.
    #[error("not a pipe name")]
    BadName,
    /// A pipe of this name already exists, so another process holds the name.
    #[error("the pipe {name} already exists")]
    NameTaken {
        /// The name asked for.
        name: String,
    },
    /// A client is connected, but the instance that would listen in its place could not be
    /// made. The server keeps the connection and hands it over once a later
    /// [`PipeServer::accept`] makes that instance.
    #[error("the pipe {name} could not make its next instance ({call}): {source}")]
    NextInstance {
        /// The pipe's name.
        name: String,
        /// The OS call that failed.
        call: &'static str,
        /// The OS's error.
        source: io::Error,
    },
    /// No pipe of this name exists.
    #[error("no pipe named {name} exists")]
    NoServer {
        /// The name asked for.
        name: String,
    },
    /// The pipe exists, but every instance of it holds a connection.
    #[error("every instance of the pipe {name} is busy")]
    Busy {
        /// The name asked for.
        name: String,
    },
    /// Any other failure of an OS call.
    #[error("{call} failed: {source}")]
    Os {
        /// The OS call that failed.
        call: &'static str,
        /// The OS's error.
        source: io::Error,
    },
}

/// A moment on the system's clock, read when a connection was accepted or made. It exists to
/// be compared with a process's creation time, which tells the process that connected from a
/// later one that took over its process id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Moment(u64);

impl Moment {
    /// The moment as Windows counts file times: 100 ns steps since 1601-01-01 UTC, the unit
    /// in which the OS reports when a process was created.
    #[must_use]
    pub const fn as_filetime(self) -> u64 {
        self.0
    }
}

/// The full path of the pipe `name`, or [`IpcError::BadName`]. A name with a separator, a
/// wildcard or a device prefix never reaches the OS.
#[cfg(any(windows, test))]
pub(crate) fn pipe_path(name: &str) -> Result<String, IpcError> {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_');
    if name.is_empty() || name.len() > MAX_PIPE_NAME || !name.chars().all(plain) {
        return Err(IpcError::BadName);
    }
    Ok(format!(r"\\.\pipe\{name}"))
}

/// What each OS's connection gives [`PipeConn`]: one shape, checked by the compiler, whether
/// or not the OS has pipes yet.
trait Connection: AsyncRead + AsyncWrite + Unpin {
    fn client_pid(&self) -> Result<u32, IpcError>;
    fn server_pid(&self) -> Result<u32, IpcError>;
    fn since(&self) -> Moment;
}

/// The serving side of one pipe. It owns the name from the pipe's first instance and, until it
/// is dropped, hands no connection over before an instance exists for the next client.
#[derive(Debug)]
pub struct PipeServer {
    inner: sys::Server,
}

impl PipeServer {
    /// Makes `\\.\pipe\<name>` as the pipe's first instance, with remote clients rejected and
    /// a DACL that grants only the current user. Fails with [`IpcError::NameTaken`] when a
    /// pipe of that name exists. Call it inside a tokio runtime with I/O, which the pipe joins.
    pub fn bind(name: &str) -> Result<Self, IpcError> {
        sys::Server::bind(name, None).map(|inner| Self { inner })
    }

    /// [`PipeServer::bind`] with the pipe held to `max_instances`, the way a test brings about
    /// a next instance that cannot be made.
    #[cfg(all(windows, test))]
    pub(crate) fn bind_limited(name: &str, max_instances: usize) -> Result<Self, IpcError> {
        sys::Server::bind(name, Some(max_instances)).map(|inner| Self { inner })
    }

    /// Waits for the next client, makes the instance that listens in its place, with the same
    /// DACL, and only then hands the connection over.
    ///
    /// When that instance cannot be made it returns [`IpcError::NextInstance`] and keeps the
    /// connection: the caller reports the error, waits a little and calls `accept` again,
    /// which tries the instance again before it does anything else. So a server whose caller
    /// keeps calling never stops listening without saying so.
    ///
    /// The wait for a client has no deadline of its own, because a server listens for as long
    /// as it lives. The future is cancel safe: dropping it loses no connection.
    pub async fn accept(&mut self) -> Result<PipeConn, IpcError> {
        self.inner.accept().await.map(|inner| PipeConn { inner })
    }

    /// Whether the DACL of the instance that listens now grants only the current user.
    #[cfg(all(windows, test))]
    pub(crate) fn grants_only_user(&self) -> Result<bool, IpcError> {
        self.inner.grants_only_user()
    }
}

/// One connection of a pipe, from [`PipeServer::accept`] or [`PipeConn::connect`]. It reads and
/// writes bytes; the protocol on them belongs to the crate that owns the pipe.
#[derive(Debug)]
pub struct PipeConn {
    inner: sys::Conn,
}

impl PipeConn {
    /// Connects to `\\.\pipe\<name>` without waiting: [`IpcError::NoServer`] when no such pipe
    /// exists, [`IpcError::Busy`] when every instance holds a connection. The caller owns the
    /// retry and its deadline. Call it inside a tokio runtime with I/O.
    ///
    /// The other end is whichever process owns the instance that answered, which need not be
    /// the one that made the pipe: check [`PipeConn::server_pid`] before sending anything.
    pub fn connect(name: &str) -> Result<Self, IpcError> {
        sys::Conn::connect(name).map(|inner| Self { inner })
    }

    /// The process id of the client end, as the OS recorded it when the client connected.
    pub fn client_pid(&self) -> Result<u32, IpcError> {
        self.inner.client_pid()
    }

    /// The process id of the server end: the process that made this instance of the pipe.
    pub fn server_pid(&self) -> Result<u32, IpcError> {
        self.inner.server_pid()
    }

    /// The moment this connection was accepted or made. A caller check refuses a process
    /// created after it.
    #[must_use]
    pub fn since(&self) -> Moment {
        self.inner.since()
    }
}

impl AsyncRead for PipeConn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for PipeConn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
