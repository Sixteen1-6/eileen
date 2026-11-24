//! The pipe types on Windows, over tokio's named pipes. The calls tokio has no safe form of
//! (an instance with a security descriptor, the process ids, the clock) are in the crate's
//! `ffi` module.

use std::io;
use std::os::windows::io::AsHandle as _;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY};

use super::{Connection, IpcError, Moment, pipe_path};
use crate::ffi::pipe;

/// The Win32 code of an OS error, when it has one.
fn os_code(error: &io::Error) -> Option<u32> {
    error.raw_os_error().and_then(|code| u32::try_from(code).ok())
}

impl Moment {
    /// The system's clock now.
    pub(crate) fn now() -> Self {
        Self(pipe::now_filetime())
    }
}

#[derive(Debug)]
pub(super) struct Server {
    name: String,
    path: String,
    max_instances: Option<usize>,
    /// The instance clients reach: listening, or, while `connected_at` is set, connected and
    /// waiting for the instance that takes its place.
    front: NamedPipeServer,
    /// When `front`'s client connected, while that connection has not been handed over. It is
    /// read once, as the client connects, so a later retry does not move the moment forward
    /// past a process that took the client's id in between.
    connected_at: Option<Moment>,
}

impl Server {
    pub(super) fn bind(name: &str, max_instances: Option<usize>) -> Result<Self, IpcError> {
        let path = pipe_path(name)?;
        let front = pipe::create_instance(&path, true, max_instances).map_err(|fault| {
            // With the first-instance flag an existing pipe answers access denied, whatever
            // its DACL says; a full one may answer busy. Either way the name is not ours.
            let taken = [ERROR_ACCESS_DENIED.0, ERROR_PIPE_BUSY.0];
            if fault.call == pipe::CREATE
                && os_code(&fault.source).is_some_and(|c| taken.contains(&c))
            {
                IpcError::NameTaken { name: name.to_owned() }
            } else {
                IpcError::Os { call: fault.call, source: fault.source }
            }
        })?;
        Ok(Self { name: name.to_owned(), path, max_instances, front, connected_at: None })
    }

    pub(super) async fn accept(&mut self) -> Result<Conn, IpcError> {
        let since = if let Some(at) = self.connected_at {
            at
        } else {
            // The only await: a caller that drops this future leaves `front` listening, or
            // connected with the client still on it for the next call to find.
            if let Err(source) = self.front.connect().await {
                // A failed connect can leave the instance half open; disconnecting puts it
                // back to listening, and an instance that already listens only reports that.
                let _already_listening = self.front.disconnect();
                return Err(IpcError::Os { call: "ConnectNamedPipe", source });
            }
            let at = Moment::now();
            self.connected_at = Some(at);
            at
        };
        let next =
            pipe::create_instance(&self.path, false, self.max_instances).map_err(|fault| {
                IpcError::NextInstance {
                    name: self.name.clone(),
                    call: fault.call,
                    source: fault.source,
                }
            })?;
        self.connected_at = None;
        let connected = std::mem::replace(&mut self.front, next);
        Ok(Conn { end: End::Server(connected), since })
    }

    #[cfg(test)]
    pub(super) fn grants_only_user(&self) -> Result<bool, IpcError> {
        pipe::grants_only_user(self.front.as_handle())
            .map_err(|fault| IpcError::Os { call: fault.call, source: fault.source })
    }
}

/// Which end of the pipe a connection holds.
#[derive(Debug)]
enum End {
    Server(NamedPipeServer),
    Client(NamedPipeClient),
}

#[derive(Debug)]
pub(super) struct Conn {
    end: End,
    since: Moment,
}

impl Conn {
    pub(super) fn connect(name: &str) -> Result<Self, IpcError> {
        let path = pipe_path(name)?;
        // tokio's defaults: read and write access, and identification-level impersonation, so
        // a server that is not the core learns who called and can do nothing as the caller.
        match ClientOptions::new().open(&path) {
            Ok(client) => Ok(Self { end: End::Client(client), since: Moment::now() }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Err(IpcError::NoServer { name: name.to_owned() })
            }
            Err(e) if os_code(&e) == Some(ERROR_PIPE_BUSY.0) => {
                Err(IpcError::Busy { name: name.to_owned() })
            }
            Err(source) => Err(IpcError::Os { call: "CreateFileW", source }),
        }
    }
}

impl Connection for Conn {
    fn client_pid(&self) -> Result<u32, IpcError> {
        let handle = match &self.end {
            End::Server(pipe) => pipe.as_handle(),
            End::Client(pipe) => pipe.as_handle(),
        };
        pipe::client_pid(handle)
            .map_err(|source| IpcError::Os { call: "GetNamedPipeClientProcessId", source })
    }

    fn server_pid(&self) -> Result<u32, IpcError> {
        let handle = match &self.end {
            End::Server(pipe) => pipe.as_handle(),
            End::Client(pipe) => pipe.as_handle(),
        };
        pipe::server_pid(handle)
            .map_err(|source| IpcError::Os { call: "GetNamedPipeServerProcessId", source })
    }

    fn since(&self) -> Moment {
        self.since
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.get_mut().end {
            End::Server(pipe) => Pin::new(pipe).poll_read(cx, buf),
            End::Client(pipe) => Pin::new(pipe).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Conn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().end {
            End::Server(pipe) => Pin::new(pipe).poll_write(cx, buf),
            End::Client(pipe) => Pin::new(pipe).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().end {
            End::Server(pipe) => Pin::new(pipe).poll_flush(cx),
            End::Client(pipe) => Pin::new(pipe).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().end {
            End::Server(pipe) => Pin::new(pipe).poll_shutdown(cx),
            End::Client(pipe) => Pin::new(pipe).poll_shutdown(cx),
        }
    }
}
