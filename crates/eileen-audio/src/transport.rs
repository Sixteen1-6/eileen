//! The audio component's side of the speech pipe's transport (BUILD.md section 6.6, decision
//! D81): [`SpeechTransport`] hands the component each connection whose `Hello` the transport has
//! already verified, and takes a connection back to close when it breaks or the codec refuses a
//! frame. The component never names the pipe, spawns a process or sees the token; WP2 builds the
//! transport (`eileen_runtime::speech`) and `eileen-cli` implements this trait over it.
//!
//! Like the ports of section 3.3.15, the trait and the byte traits beneath [`SpeechConnection`]
//! use std types only, so an implementation needs nothing from this crate's async runtime. The
//! connection's halves add the deadlines: every read and every write says how long it may wait,
//! and they run on a tokio runtime with its timer enabled.

use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::Duration;

/// A boxed I/O future, as the byte traits return it.
pub type IoFuture<'a, T> = Pin<Box<dyn Future<Output = io::Result<T>> + Send + 'a>>;

/// Where the component's connections to the speech process come from.
pub trait SpeechTransport: Send + Sync {
    /// Waits for the next connection whose `Hello` the transport has verified. `None` once the
    /// transport has shut down and will make no more.
    ///
    /// The wait has no deadline of its own: the speech process connects only once it listens (10
    /// to 20 s after it starts, or after a reload), the component keeps capturing into its ring
    /// meanwhile and races this wait against its effects and shutdown, and the core's 120 s load
    /// deadline (section 6.3) bounds how long the process may take.
    fn accept(&self) -> Pin<Box<dyn Future<Output = Option<SpeechConnection>> + Send + '_>>;

    /// Closes a connection the component is done with, because it broke or the codec refused a
    /// frame. It only closes it: the speech process exits when its connection closes, the
    /// transport reports the exit, and the core decides the restart.
    fn close(&self, connection: SpeechConnection);
}

/// The reading side of a connection's bytes, as a transport supplies it.
pub trait ByteRead: Send {
    /// Reads at least one byte into `buf`, or returns 0 at the end of the stream.
    ///
    /// The future must be cancel safe: one dropped before it completes has taken no bytes, so a
    /// read that missed its deadline loses nothing.
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> IoFuture<'a, usize>;
}

/// The writing side of a connection's bytes, as a transport supplies it.
pub trait ByteWrite: Send {
    /// Writes at least one byte of `buf`, or returns 0 when the peer takes no more. Bytes it
    /// reports written reach the peer without a flush.
    fn write<'a>(&'a mut self, buf: &'a [u8]) -> IoFuture<'a, usize>;
}

/// Which connection this is, numbered by the transport that made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionId(u64);

impl ConnectionId {
    /// The connection numbered `n`.
    #[must_use]
    pub const fn new(n: u64) -> Self {
        Self(n)
    }

    /// Its number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "speech connection {}", self.0)
    }
}

/// Why a read or a write on a connection failed.
#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    /// Nothing arrived, or the peer took nothing more, within the deadline.
    #[error("the speech connection did not move within {within:?}")]
    TimedOut {
        /// The deadline that passed.
        within: Duration,
    },
    /// The peer closed its end.
    #[error("the speech process closed its connection")]
    Closed,
    /// An earlier write failed part way, so the stream may end inside a frame.
    #[error("an earlier write on this speech connection failed")]
    Broken,
    /// The transport reported an error.
    #[error("the speech connection failed")]
    Io(#[source] io::Error),
}

impl From<io::Error> for ConnectionError {
    fn from(error: io::Error) -> Self {
        if PEER_GONE.contains(&error.kind()) { Self::Closed } else { Self::Io(error) }
    }
}

/// The error kinds that mean the peer's end is gone.
const PEER_GONE: [io::ErrorKind; 4] = [
    io::ErrorKind::BrokenPipe,
    io::ErrorKind::ConnectionReset,
    io::ErrorKind::ConnectionAborted,
    io::ErrorKind::UnexpectedEof,
];

/// A verified connection to the speech process: a byte duplex whose every read and write has a
/// deadline. The transport bounds what it buffers; the halves buffer nothing of their own.
pub struct SpeechConnection {
    reader: SpeechReader,
    writer: SpeechWriter,
}

impl SpeechConnection {
    /// A connection over a transport's two byte halves.
    #[must_use]
    pub const fn new(id: ConnectionId, read: Box<dyn ByteRead>, write: Box<dyn ByteWrite>) -> Self {
        Self {
            reader: SpeechReader { id, inner: read },
            writer: SpeechWriter { id, inner: write, broken: false },
        }
    }

    /// Which connection it is.
    #[must_use]
    pub const fn id(&self) -> ConnectionId {
        self.reader.id
    }

    /// Its read half.
    pub const fn reader(&mut self) -> &mut SpeechReader {
        &mut self.reader
    }

    /// Its write half.
    pub const fn writer(&mut self) -> &mut SpeechWriter {
        &mut self.writer
    }

    /// Its halves, so that one task can read while another writes.
    #[must_use]
    pub fn split(self) -> (SpeechReader, SpeechWriter) {
        (self.reader, self.writer)
    }

    /// The connection again from its two halves, so it can be handed back to the transport.
    pub fn rejoin(reader: SpeechReader, writer: SpeechWriter) -> Result<Self, NotAPair> {
        if reader.id == writer.id {
            Ok(Self { reader, writer })
        } else {
            Err(NotAPair { reader, writer })
        }
    }
}

impl fmt::Debug for SpeechConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpeechConnection")
            .field("id", &self.id())
            .field("broken", &self.writer.broken)
            .finish_non_exhaustive()
    }
}

/// Two halves of different connections, given back unchanged.
#[derive(Debug, thiserror::Error)]
#[error("{} and {} are halves of different connections", .reader.id, .writer.id)]
pub struct NotAPair {
    /// The read half.
    pub reader: SpeechReader,
    /// The write half.
    pub writer: SpeechWriter,
}

/// The read half of a [`SpeechConnection`].
pub struct SpeechReader {
    id: ConnectionId,
    inner: Box<dyn ByteRead>,
}

impl SpeechReader {
    /// Which connection it belongs to.
    #[must_use]
    pub const fn id(&self) -> ConnectionId {
        self.id
    }

    /// Reads at least one byte into `buf`, waiting at most `within`. Reads nothing into an empty
    /// `buf`. A read that times out has taken no bytes, so the connection stays usable.
    pub async fn read(
        &mut self,
        buf: &mut [u8],
        within: Duration,
    ) -> Result<usize, ConnectionError> {
        if buf.is_empty() {
            return Ok(0);
        }
        match tokio::time::timeout(within, self.inner.read(buf)).await {
            Err(_elapsed) => Err(ConnectionError::TimedOut { within }),
            Ok(Ok(0)) => Err(ConnectionError::Closed),
            Ok(Ok(n)) => Ok(n),
            Ok(Err(error)) => Err(error.into()),
        }
    }
}

impl fmt::Debug for SpeechReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpeechReader").field("id", &self.id).finish_non_exhaustive()
    }
}

/// The write half of a [`SpeechConnection`].
pub struct SpeechWriter {
    id: ConnectionId,
    inner: Box<dyn ByteWrite>,
    broken: bool,
}

impl SpeechWriter {
    /// Which connection it belongs to.
    #[must_use]
    pub const fn id(&self) -> ConnectionId {
        self.id
    }

    /// Writes all of `buf`, waiting at most `within` for the whole of it.
    ///
    /// On any error the peer may have received part of `buf`, so the stream may now end inside a
    /// frame: every later write fails with [`ConnectionError::Broken`], and the connection goes
    /// back to the transport.
    pub async fn write_all(&mut self, buf: &[u8], within: Duration) -> Result<(), ConnectionError> {
        if self.broken {
            return Err(ConnectionError::Broken);
        }
        let result = match tokio::time::timeout(within, write_all(self.inner.as_mut(), buf)).await {
            Err(_elapsed) => Err(ConnectionError::TimedOut { within }),
            Ok(result) => result,
        };
        self.broken = result.is_err();
        result
    }
}

impl fmt::Debug for SpeechWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpeechWriter")
            .field("id", &self.id)
            .field("broken", &self.broken)
            .finish_non_exhaustive()
    }
}

async fn write_all(inner: &mut dyn ByteWrite, mut buf: &[u8]) -> Result<(), ConnectionError> {
    while !buf.is_empty() {
        let n = inner.write(buf).await?;
        if n == 0 {
            return Err(ConnectionError::Closed);
        }
        buf = buf.get(n..).unwrap_or_default();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{ConnectionError, ConnectionId, SpeechConnection};
    use crate::fake::{MemoryLimits, memory_transport};

    #[tokio::test(start_paused = true)]
    async fn halves_rejoin_only_with_their_own_pair() {
        let (_transport, mut stand_ins) = memory_transport(MemoryLimits::default());
        let (reader_one, writer_one) = stand_ins.connect().unwrap().split();
        let (reader_two, writer_two) = stand_ins.connect().unwrap().split();

        let refused = SpeechConnection::rejoin(reader_one, writer_two).unwrap_err();
        assert_eq!(refused.reader.id(), ConnectionId::new(1), "the read half comes back");
        assert_eq!(refused.writer.id(), ConnectionId::new(2), "the write half comes back");
        let one = SpeechConnection::rejoin(refused.reader, writer_one).unwrap();
        let two = SpeechConnection::rejoin(reader_two, refused.writer).unwrap();
        assert_eq!((one.id().get(), two.id().get()), (1, 2), "each with its own");
    }

    #[test]
    fn a_vanished_peer_reads_as_closed_and_other_errors_keep_their_cause() {
        for kind in [io::ErrorKind::BrokenPipe, io::ErrorKind::ConnectionReset] {
            let error = ConnectionError::from(io::Error::from(kind));
            assert!(matches!(error, ConnectionError::Closed), "{kind:?}");
        }
        let denied = ConnectionError::from(io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(
            matches!(&denied, ConnectionError::Io(e) if e.kind() == io::ErrorKind::PermissionDenied),
            "kept: {denied:?}"
        );
    }
}
