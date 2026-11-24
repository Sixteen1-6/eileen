//! The pipe types where they are not built yet: every constructor answers
//! [`IpcError::Unsupported`], so neither type ever has a value and no method can run.

use std::convert::Infallible;
use std::future::Ready;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::{Connection, IpcError, Moment};

/// A server that cannot exist.
#[derive(Debug)]
pub(super) struct Server {
    never: Infallible,
}

impl Server {
    pub(super) const fn bind(_name: &str, _max_instances: Option<usize>) -> Result<Self, IpcError> {
        Err(IpcError::Unsupported)
    }

    pub(super) const fn accept(&mut self) -> Ready<Result<Conn, IpcError>> {
        match self.never {}
    }
}

/// A connection that cannot exist.
#[derive(Debug)]
pub(super) struct Conn {
    never: Infallible,
}

impl Conn {
    pub(super) const fn connect(_name: &str) -> Result<Self, IpcError> {
        Err(IpcError::Unsupported)
    }
}

impl Connection for Conn {
    fn client_pid(&self) -> Result<u32, IpcError> {
        match self.never {}
    }

    fn server_pid(&self) -> Result<u32, IpcError> {
        match self.never {}
    }

    fn since(&self) -> Moment {
        match self.never {}
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.never {}
    }
}

impl AsyncWrite for Conn {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.never {}
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.never {}
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.never {}
    }
}
