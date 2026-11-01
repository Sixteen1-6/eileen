//! An in-memory speech transport, for this crate's tests and other crates' (WP2's speech
//! transport and its scenarios). Compiled for tests and under the `fake` feature, which only
//! dev-dependencies turn on, so no shipped binary carries it.
//!
//! [`memory_transport`] returns the component's side, a [`MemoryTransport`], and the test's side,
//! [`StandIns`], which plays the speech process: it connects a scripted stand-in whenever the test
//! likes (late, or again after a kill), and sees each connection the component hands back. Every
//! queue in it has a maximum: the connections waiting to be accepted, the bytes in flight each
//! way, and the record of connections handed back.

use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::time::Duration;

use eileen_rt::channel::{self, Receiver, Sender, TrySendError};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, DuplexStream, ReadHalf, WriteHalf};
use tokio::sync::Mutex;

use crate::transport::{
    ByteRead, ByteWrite, ConnectionId, IoFuture, SpeechConnection, SpeechTransport,
};

/// The maxima of a memory transport's queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryLimits {
    /// Connections made but not yet accepted.
    pub pending: NonZeroUsize,
    /// Bytes in flight in each direction of one connection, as a pipe's buffer bounds them.
    pub buffer_bytes: NonZeroUsize,
    /// Connections handed back that the test has not yet looked at; more are dropped unrecorded,
    /// and logged as `channel.full`.
    pub handed_back: NonZeroUsize,
}

impl Default for MemoryLimits {
    /// Two waiting connections, 64 KiB each way (a named pipe's usual buffer), and 16 hand-backs.
    fn default() -> Self {
        Self {
            pending: NonZeroUsize::MIN.saturating_add(1),
            buffer_bytes: NonZeroUsize::MIN.saturating_add(64 * 1024 - 1),
            handed_back: NonZeroUsize::MIN.saturating_add(15),
        }
    }
}

/// A memory transport with these limits: the component's side and the test's.
#[must_use]
pub fn memory_transport(limits: MemoryLimits) -> (MemoryTransport, StandIns) {
    let (pending_tx, pending_rx) = channel::bounded("audio.fake.pending", limits.pending);
    let (back_tx, back_rx) = channel::bounded("audio.fake.handed_back", limits.handed_back);
    let transport = MemoryTransport { pending: Mutex::new(pending_rx), handed_back: back_tx };
    let stand_ins = StandIns {
        pending: pending_tx,
        handed_back: back_rx,
        buffer_bytes: limits.buffer_bytes,
        next_id: 1,
    };
    (transport, stand_ins)
}

/// The component's side of a memory transport. Once its [`StandIns`] is dropped and every
/// waiting connection has been accepted, [`SpeechTransport::accept`] returns `None`.
#[derive(Debug)]
pub struct MemoryTransport {
    pending: Mutex<Receiver<SpeechConnection>>,
    handed_back: Sender<ConnectionId>,
}

impl SpeechTransport for MemoryTransport {
    fn accept(&self) -> Pin<Box<dyn Future<Output = Option<SpeechConnection>> + Send + '_>> {
        Box::pin(async move { self.pending.lock().await.recv().await })
    }

    /// Drops the connection, which closes both directions, and records its id.
    fn close(&self, connection: SpeechConnection) {
        let id = connection.id();
        drop(connection);
        // A full record drops the id; the channel counts and logs it.
        let _unrecorded = self.handed_back.try_send(id);
    }
}

/// The test's side of a memory transport: it plays the speech process.
#[derive(Debug)]
pub struct StandIns {
    pending: Sender<SpeechConnection>,
    handed_back: Receiver<ConnectionId>,
    buffer_bytes: NonZeroUsize,
    next_id: u64,
}

/// Why [`StandIns::connect`] made no connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConnectRefused {
    /// As many connections as [`MemoryLimits::pending`] allows are waiting to be accepted.
    #[error("too many speech connections are waiting to be accepted")]
    Full,
    /// The component's side is gone.
    #[error("the memory transport's component side is gone")]
    Gone,
}

/// Why [`StandIns::handed_back`] returned no connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotHandedBack {
    /// None came back within the deadline.
    #[error("no speech connection was handed back within {within:?}")]
    TimedOut {
        /// The deadline that passed.
        within: Duration,
    },
    /// The component's side is gone.
    #[error("the memory transport's component side is gone")]
    Gone,
}

impl StandIns {
    /// Connects a stand-in, as a speech process that has loaded its models and sent a verified
    /// `Hello`: the component's end waits to be accepted, and the stand-in's end is returned.
    /// Dropping the stand-in's end kills it: the component reads the end of the stream and its
    /// writes fail.
    pub fn connect(&mut self) -> Result<SpeechConnection, ConnectRefused> {
        let id = ConnectionId::new(self.next_id);
        let (component_end, stand_in_end) = tokio::io::duplex(self.buffer_bytes.get());
        match self.pending.try_send(connection(id, component_end)) {
            Ok(()) => {
                self.next_id = self.next_id.saturating_add(1);
                Ok(connection(id, stand_in_end))
            }
            Err(TrySendError::Full(_)) => Err(ConnectRefused::Full),
            Err(TrySendError::Closed(_)) => Err(ConnectRefused::Gone),
        }
    }

    /// The next connection the component handed back, waiting at most `within`.
    pub async fn handed_back(&mut self, within: Duration) -> Result<ConnectionId, NotHandedBack> {
        match tokio::time::timeout(within, self.handed_back.recv()).await {
            Err(_elapsed) => Err(NotHandedBack::TimedOut { within }),
            Ok(Some(id)) => Ok(id),
            Ok(None) => Err(NotHandedBack::Gone),
        }
    }
}

fn connection(id: ConnectionId, stream: DuplexStream) -> SpeechConnection {
    let (read, write) = tokio::io::split(stream);
    SpeechConnection::new(id, Box::new(DuplexRead(read)), Box::new(DuplexWrite(write)))
}

struct DuplexRead(ReadHalf<DuplexStream>);

impl ByteRead for DuplexRead {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> IoFuture<'a, usize> {
        Box::pin(self.0.read(buf))
    }
}

struct DuplexWrite(WriteHalf<DuplexStream>);

impl ByteWrite for DuplexWrite {
    fn write<'a>(&'a mut self, buf: &'a [u8]) -> IoFuture<'a, usize> {
        Box::pin(self.0.write(buf))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::time::Duration;

    use super::{ConnectRefused, MemoryLimits, NotHandedBack, memory_transport};
    use crate::transport::{ConnectionError, ConnectionId, SpeechConnection, SpeechTransport};

    const WAIT: Duration = Duration::from_secs(1);

    fn nz(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap()
    }

    /// Takes the next connection, or fails the test if none is waiting within `WAIT`.
    async fn accept(transport: &dyn SpeechTransport) -> SpeechConnection {
        tokio::time::timeout(WAIT, transport.accept()).await.expect("accepted").expect("open")
    }

    async fn read_some(connection: &mut SpeechConnection) -> Result<Vec<u8>, ConnectionError> {
        let mut buf = [0_u8; 64];
        let n = connection.reader().read(&mut buf, WAIT).await?;
        Ok(buf[..n].to_vec())
    }

    #[tokio::test(start_paused = true)]
    async fn a_stand_in_connected_late_is_accepted_and_carries_bytes_both_ways() {
        let (transport, mut stand_ins) = memory_transport(MemoryLimits::default());
        let none_yet = tokio::time::timeout(Duration::from_secs(5), transport.accept()).await;
        assert!(none_yet.is_err(), "nothing to accept before a stand-in connects");

        let mut stand_in = stand_ins.connect().unwrap();
        let mut component = accept(&transport).await;
        assert_eq!(component.id(), stand_in.id(), "both ends are one connection");

        component.writer().write_all(b"audio", WAIT).await.unwrap();
        assert_eq!(read_some(&mut stand_in).await.unwrap(), b"audio", "component to stand-in");
        stand_in.writer().write_all(b"event", WAIT).await.unwrap();
        assert_eq!(read_some(&mut component).await.unwrap(), b"event", "stand-in to component");
    }

    #[tokio::test(start_paused = true)]
    async fn a_killed_stand_in_breaks_the_connection_which_is_handed_back() {
        let (transport, mut stand_ins) = memory_transport(MemoryLimits::default());
        let stand_in = stand_ins.connect().unwrap();
        let mut component = accept(&transport).await;

        drop(stand_in);
        assert!(matches!(read_some(&mut component).await, Err(ConnectionError::Closed)), "read");
        let write = component.writer().write_all(b"audio", WAIT).await;
        assert!(matches!(write, Err(ConnectionError::Closed)), "write: {write:?}");
        let id = component.id();
        transport.close(component);
        assert_eq!(stand_ins.handed_back(WAIT).await, Ok(id), "handed back");

        let mut second = stand_ins.connect().unwrap();
        let mut component = accept(&transport).await;
        assert_eq!(component.id(), ConnectionId::new(2), "a new connection");
        second.writer().write_all(b"hello", WAIT).await.unwrap();
        assert_eq!(read_some(&mut component).await.unwrap(), b"hello", "the new one works");
    }

    #[tokio::test(start_paused = true)]
    async fn handing_a_connection_back_closes_the_stand_in_end() {
        let (transport, mut stand_ins) = memory_transport(MemoryLimits::default());
        let mut stand_in = stand_ins.connect().unwrap();
        let component = accept(&transport).await;

        transport.close(component);
        assert!(matches!(read_some(&mut stand_in).await, Err(ConnectionError::Closed)), "closed");
    }

    #[tokio::test(start_paused = true)]
    async fn a_read_with_nothing_to_read_times_out_and_loses_nothing() {
        let (transport, mut stand_ins) = memory_transport(MemoryLimits::default());
        let mut stand_in = stand_ins.connect().unwrap();
        let mut component = accept(&transport).await;

        let quarter = Duration::from_millis(250);
        let mut buf = [0_u8; 8];
        let late = component.reader().read(&mut buf, quarter).await;
        assert!(
            matches!(late, Err(ConnectionError::TimedOut { within }) if within == quarter),
            "timed out: {late:?}"
        );
        stand_in.writer().write_all(b"pong", WAIT).await.unwrap();
        assert_eq!(read_some(&mut component).await.unwrap(), b"pong", "still usable");
        assert_eq!(component.reader().read(&mut [], WAIT).await.unwrap(), 0, "empty buffer");
    }

    #[tokio::test(start_paused = true)]
    async fn bytes_in_flight_are_bounded_and_a_stuck_write_breaks_the_writer() {
        let limits = MemoryLimits { buffer_bytes: nz(8), ..MemoryLimits::default() };
        let (transport, mut stand_ins) = memory_transport(limits);
        let _stand_in = stand_ins.connect().unwrap();
        let mut component = accept(&transport).await;

        component.writer().write_all(&[1; 8], WAIT).await.unwrap();
        let stuck = component.writer().write_all(&[2; 1], WAIT).await;
        assert!(matches!(stuck, Err(ConnectionError::TimedOut { .. })), "no room: {stuck:?}");
        let after = component.writer().write_all(&[], WAIT).await;
        assert!(matches!(after, Err(ConnectionError::Broken)), "refused after: {after:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn connections_waiting_to_be_accepted_are_bounded() {
        let limits = MemoryLimits { pending: nz(1), ..MemoryLimits::default() };
        let (transport, mut stand_ins) = memory_transport(limits);
        let _first = stand_ins.connect().unwrap();
        assert_eq!(stand_ins.connect().unwrap_err(), ConnectRefused::Full, "one waits at most");

        let _accepted = accept(&transport).await;
        assert!(stand_ins.connect().is_ok(), "room again once accepted");
    }

    #[tokio::test(start_paused = true)]
    async fn the_record_of_connections_handed_back_is_bounded() {
        let limits = MemoryLimits { pending: nz(2), handed_back: nz(1), ..MemoryLimits::default() };
        let (transport, mut stand_ins) = memory_transport(limits);
        let _one = stand_ins.connect().unwrap();
        let _two = stand_ins.connect().unwrap();
        let first = accept(&transport).await;
        let second = accept(&transport).await;
        let first_id = first.id();

        transport.close(first);
        transport.close(second);
        assert_eq!(stand_ins.handed_back(WAIT).await, Ok(first_id), "the first is recorded");
        assert_eq!(
            stand_ins.handed_back(WAIT).await,
            Err(NotHandedBack::TimedOut { within: WAIT }),
            "the second found the record full"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn accept_ends_once_the_stand_ins_are_gone_and_connect_once_the_transport_is() {
        let (transport, mut stand_ins) = memory_transport(MemoryLimits::default());
        let _waiting = stand_ins.connect().unwrap();
        drop(stand_ins);
        assert!(transport.accept().await.is_some(), "a waiting connection is still accepted");
        assert!(transport.accept().await.is_none(), "then no more");

        let (transport, mut stand_ins) = memory_transport(MemoryLimits::default());
        drop(transport);
        assert_eq!(stand_ins.connect().unwrap_err(), ConnectRefused::Gone, "nobody accepts");
        assert_eq!(stand_ins.handed_back(WAIT).await, Err(NotHandedBack::Gone), "nothing back");
    }
}
