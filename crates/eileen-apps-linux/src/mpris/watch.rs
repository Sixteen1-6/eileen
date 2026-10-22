//! [`MprisWatch`] (`MediaSessions::subscribe`): each player's state as it changes. Its Linux
//! sources are `NameOwnerChanged` for the namespace `org.mpris.MediaPlayer2`, which
//! `eileen_dbus_linux::owner_changes` takes from the bus alone, and `PropertiesChanged` for the
//! `Player` interface (the rule's `arg0`) and `Seeked`, both at `/org/mpris/MediaPlayer2`, which
//! count only for the players their sender owns
//! (<https://dbus.freedesktop.org/doc/dbus-specification.html#message-bus-routing-match-rules>).

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::{Context, Poll};

use eileen_contracts::desktop::MediaState;
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::{
    DRAIN_CAP, DbusError, Names, OwnerChange, OwnerChanges, QUEUE_SIZE, Stream, owner_changes,
};
use zbus::message::Type;
use zbus::names::OwnedUniqueName;
use zbus::{Connection, MatchRule, Message, MessageStream};

use super::{OBJECT, PLAYER, PREFIX, players, read};

/// The namespace of every player's bus name.
const NAMESPACE: &str = "org.mpris.MediaPlayer2";

/// How many reads one poll starts or finishes before it yields.
const MAX_STEPS: usize = 16;

/// What the errors of opening the watch name.
const SUBSCRIPTION: &str = "player signal subscription";

/// One change a [`MprisWatch`] reports.
#[derive(Debug, Clone, PartialEq)]
pub enum MprisEvent {
    /// A player appeared or its state changed: the state now.
    Changed(MediaState),
    /// A player left the bus, or another connection took its name: its last state, with
    /// `playing` false.
    Gone(MediaState),
}

impl MprisEvent {
    /// The state either way, which is what the `MediaSessions::subscribe` sink takes: a player
    /// that left is no longer playing.
    #[must_use]
    pub fn into_state(self) -> MediaState {
        match self {
            Self::Changed(state) | Self::Gone(state) => state,
        }
    }
}

/// A future the watch keeps while it waits.
type Waiting<T> = Pin<Box<dyn Future<Output = Result<T, DbusError>> + Send>>;

/// A player the watch follows.
struct Tracked {
    /// The unique name that owns the player's bus name; the player's signals come from it.
    owner: OwnedUniqueName,
    /// The app's name, once read: it is read once per owner.
    app: Option<Untrusted<String>>,
    /// The state last reported.
    last: Option<MediaState>,
}

/// What the watch waits for.
enum Op {
    /// The players on the bus: each player's bus name and the unique name that owns it.
    List(Waiting<Vec<(String, OwnedUniqueName)>>),
    /// One player's state, read from `owner`.
    Read {
        /// The player's bus name.
        name: String,
        /// The owner the read went to.
        owner: OwnedUniqueName,
        /// The reply: the app's name and the state.
        reply: Waiting<(Untrusted<String>, MediaState)>,
    },
}

/// When the watch lists the players again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Relist {
    /// Not unless a listing fails.
    #[default]
    No,
    /// At the next signal: the last listing failed.
    OnSignal,
    /// As soon as no read runs.
    Due,
}

/// How one step of a poll ended.
enum Step {
    /// The pending read or listing has not answered yet.
    Waiting,
    /// A change to report.
    Event(MprisEvent),
    /// Nothing to report; the next step can run.
    Done,
}

/// What the watch knows of the players, apart from its streams and the call it waits for.
#[derive(Default)]
struct Players {
    /// The players by bus name.
    tracked: BTreeMap<String, Tracked>,
    /// The players to read again, first marked first, each once; every one is tracked.
    to_read: VecDeque<String>,
    /// The players that left, with their last state, not yet reported.
    gone: BTreeMap<String, MediaState>,
    /// While a listing runs, the MPRIS names a `NameOwnerChanged` reported meanwhile: the
    /// signal is newer than the listing's view of them.
    reported: Option<BTreeSet<String>>,
    /// When the watch lists the players again.
    relist: Relist,
}

impl Players {
    /// A signal came: a listing that failed is due again.
    fn signalled(&mut self) {
        if self.relist == Relist::OnSignal {
            self.relist = Relist::Due;
        }
    }

    /// The bus says that `change.name` now belongs to `change.new`, or to no one.
    fn owner_changed(&mut self, change: &OwnerChange) {
        self.signalled();
        let name = change.name.as_str();
        if !name.starts_with(PREFIX) {
            return;
        }
        if let Some(reported) = &mut self.reported {
            reported.insert(name.to_owned());
        }
        if self.tracked.get(name).is_some_and(|player| change.new.as_ref() == Some(&player.owner)) {
            mark(&mut self.to_read, name);
            return;
        }
        if let Some(last) = self.tracked.remove(name).and_then(|player| player.last) {
            self.gone.insert(name.to_owned(), MediaState { playing: false, ..last });
        }
        self.to_read.retain(|queued| queued != name);
        if let Some(owner) = &change.new {
            let player = Tracked { owner: owner.clone(), app: None, last: None };
            self.tracked.insert(name.to_owned(), player);
            mark(&mut self.to_read, name);
        }
    }

    /// A `PropertiesChanged` or `Seeked` came: the players its sender owns are read again.
    fn changed(&mut self, message: &Message) {
        self.signalled();
        let header = message.header();
        let Some(sender) = header.sender() else {
            return;
        };
        for (name, player) in &self.tracked {
            if player.owner.as_str() == sender.as_str() {
                mark(&mut self.to_read, name);
            }
        }
    }

    /// Follows the players the listing found, read ahead of any marked since. A player already
    /// followed, or one that a `NameOwnerChanged` reported while the listing ran (`reported`),
    /// stays as the signals have it: they are newer than the listing's view, whether they
    /// brought the player or took it away.
    fn listed(&mut self, listed: Vec<(String, OwnedUniqueName)>, reported: &BTreeSet<String>) {
        let mut first = VecDeque::new();
        for (name, owner) in listed {
            if reported.contains(&name) {
                continue;
            }
            if let Entry::Vacant(entry) = self.tracked.entry(name) {
                first.push_back(entry.key().clone());
                entry.insert(Tracked { owner, app: None, last: None });
            }
        }
        first.append(&mut self.to_read);
        self.to_read = first;
    }

    /// Takes a finished read of `name` from `owner`: the change to report, if any. A read from
    /// an owner that has since left, or that failed, reports nothing.
    fn read_done(
        &mut self,
        name: &str,
        owner: &OwnedUniqueName,
        result: Result<(Untrusted<String>, MediaState), DbusError>,
    ) -> Option<MprisEvent> {
        let player = self.tracked.get_mut(name).filter(|player| player.owner == *owner)?;
        let (app, state) = result.ok()?;
        player.app = Some(app);
        if player.last.as_ref() == Some(&state) {
            return None;
        }
        player.last = Some(state.clone());
        Some(MprisEvent::Changed(state))
    }
}

/// Each player's state, then each change: `MediaSessions::subscribe` over MPRIS, as a stream
/// the caller drives through [`Self::next`] or `Stream`. It never spawns a task.
///
/// - First, every player on the bus when it opened, one [`MprisEvent::Changed`] each, in bus
///   name order. The signals are subscribed before the players are listed, so no change falls
///   between the two: a player that `NameOwnerChanged` reports while the listing runs is
///   taken as the signal has it, which is newer, whether it came or left.
/// - Then a [`MprisEvent::Changed`] for each player that arrives, and each time a player's
///   state changes: `PropertiesChanged` on its `Player` interface and `Seeked` mark it to be
///   read again. A read that finds the state as last reported reports nothing, and signals
///   faster than the reads fold into one read per player. Players are read in the order they
///   were marked, so one marked again while it is read waits behind the others. A signal from
///   a sender that owns no player's name is ignored, and so is a `NameOwnerChanged` that the
///   bus did not send.
/// - A [`MprisEvent::Gone`] when a player leaves the bus or another connection takes its name.
/// - `None` once the bus connection has ended, and only then: a stream's read error is passed
///   over, and the stream's end follows it.
///
/// A player that cannot be read is skipped until its next signal, and a listing that fails, as
/// one past the connection's method timeout does, is made again after the next signal. Reads
/// run one at a time, so a player that never answers holds back the others until the method
/// timeout. `Position` sends no signal as it advances: a player's position is the one read
/// with its last change.
///
/// # Polling and dropping
///
/// The watch holds zbus signal streams, with the traps `eileen-dbus-linux` gives for them:
///
/// - Keep polling it. Its signals queue on the connection, at most 256 of each kind, and a full
///   queue stops the connection reading anything else, method replies included.
/// - Drop it inside the tokio runtime's context: zbus spawns a task there to remove its match
///   rules. Dropped on a plain thread, as a `Subscription`'s release closure may be, it panics
///   and leaves its rules on the bus; dropped while a panic unwinds, it aborts the process. So
///   a `subscribe` adapter polls the watch in a runtime task and releases it by ending the task.
#[must_use = "a watch reports nothing unless it is polled"]
pub struct MprisWatch {
    /// The bus the reads go to.
    conn: Connection,
    /// `NameOwnerChanged` for the MPRIS names.
    owner_changes: OwnerChanges,
    /// `PropertiesChanged` on the `Player` interface.
    properties_changed: MessageStream,
    /// `Seeked`.
    seeked: MessageStream,
    /// What the watch knows of the players.
    players: Players,
    /// The listing or read the watch waits for.
    op: Option<Op>,
    /// The connection has ended.
    ended: bool,
}

impl MprisWatch {
    /// Subscribes to the signals on `conn`; the players are listed on the first poll.
    pub(super) async fn open(conn: &Connection) -> Result<Self, DbusError> {
        let failed = |error: zbus::Error| DbusError::from_zbus(SUBSCRIPTION, &error);
        let owner_changes = owner_changes(conn, Names::Namespace(NAMESPACE)).await?;
        let properties_changed = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .and_then(|rule| rule.member("PropertiesChanged"))
            .and_then(|rule| rule.path(OBJECT))
            .and_then(|rule| rule.add_arg(PLAYER))
            .map_err(failed)?
            .build();
        let seeked = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(PLAYER)
            .and_then(|rule| rule.member("Seeked"))
            .and_then(|rule| rule.path(OBJECT))
            .map_err(failed)?
            .build();
        let stream =
            |rule: MatchRule<'static>| MessageStream::for_match_rule(rule, conn, Some(QUEUE_SIZE));
        let properties_changed = stream(properties_changed).await.map_err(failed)?;
        let seeked = stream(seeked).await.map_err(failed)?;
        let mut watch = Self {
            conn: conn.clone(),
            owner_changes,
            properties_changed,
            seeked,
            players: Players::default(),
            op: None,
            ended: false,
        };
        watch.list();
        Ok(watch)
    }

    /// The next change; `None` once the bus connection has ended. It is cancel-safe: a read in
    /// progress stays with the watch for the next call.
    pub async fn next(&mut self) -> Option<MprisEvent> {
        poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }

    /// Takes the signals that have arrived, at most `DRAIN_CAP` of each kind.
    fn drain(&mut self, cx: &mut Context<'_>) {
        let players = &mut self.players;
        let mut ended = take_ready(&mut self.owner_changes, cx, |change| {
            players.owner_changed(&change);
        });
        for stream in [&mut self.properties_changed, &mut self.seeked] {
            ended |= take_ready(stream, cx, |item| {
                // A read error is passed over: the stream's end follows it.
                if let Ok(message) = item {
                    players.changed(&message);
                }
            });
        }
        self.ended |= ended;
    }

    /// Starts listing the players on the bus.
    fn list(&mut self) {
        self.players.reported = Some(BTreeSet::new());
        let conn = self.conn.clone();
        self.op = Some(Op::List(Box::pin(async move { players(&conn).await })));
    }

    /// Starts the next listing or read: a listing when one is due, else the read of the player
    /// marked first, which every marked player is tracked for. False when there is nothing to
    /// start.
    fn start_next(&mut self) -> bool {
        if self.players.relist == Relist::Due {
            self.players.relist = Relist::No;
            self.list();
            return true;
        }
        let Some(name) = self.players.to_read.pop_front() else {
            return false;
        };
        if let Some(player) = self.players.tracked.get(&name) {
            let (owner, app) = (player.owner.clone(), player.app.clone());
            let reply = Box::pin(read(self.conn.clone(), name.clone(), owner.clone(), app));
            self.op = Some(Op::Read { name, owner, reply });
        }
        true
    }

    /// Polls the listing or read the watch waits for.
    fn poll_op(&mut self, cx: &mut Context<'_>) -> Step {
        match self.op.as_mut() {
            None => Step::Done,
            Some(Op::List(reply)) => {
                let Poll::Ready(listed) = reply.as_mut().poll(cx) else {
                    return Step::Waiting;
                };
                self.op = None;
                let reported = self.players.reported.take().unwrap_or_default();
                match listed {
                    Ok(listed) => self.players.listed(listed, &reported),
                    // The connection may be fine, as when a call ran past its method timeout:
                    // its end shows in the signal streams instead.
                    Err(_) => self.players.relist = Relist::OnSignal,
                }
                Step::Done
            }
            Some(Op::Read { reply, .. }) => {
                let Poll::Ready(result) = reply.as_mut().poll(cx) else {
                    return Step::Waiting;
                };
                let Some(Op::Read { name, owner, .. }) = self.op.take() else {
                    return Step::Done;
                };
                self.players.read_done(&name, &owner, result).map_or(Step::Done, Step::Event)
            }
        }
    }
}

impl Stream for MprisWatch {
    type Item = MprisEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<MprisEvent>> {
        let this = self.get_mut();
        for _ in 0..MAX_STEPS {
            this.drain(cx);
            if let Some((_, state)) = this.players.gone.pop_first() {
                return Poll::Ready(Some(MprisEvent::Gone(state)));
            }
            if this.ended {
                return Poll::Ready(None);
            }
            if this.op.is_none() {
                if !this.start_next() {
                    return Poll::Pending;
                }
                continue;
            }
            match this.poll_op(cx) {
                Step::Waiting => return Poll::Pending,
                Step::Event(event) => return Poll::Ready(Some(event)),
                Step::Done => {}
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl fmt::Debug for MprisWatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counts only: a state holds a player's text.
        f.debug_struct("MprisWatch")
            .field("players", &self.players.tracked.len())
            .field("to_read", &self.players.to_read.len())
            .field("gone", &self.players.gone.len())
            .field("waiting", &self.op.is_some())
            .field("relist", &self.players.relist)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

/// Takes what `stream` has ready, at most `DRAIN_CAP` items, each through `take`; true once the
/// stream has ended. A stream with more ready wakes the task to come back for the rest, rather
/// than starve the reads.
fn take_ready<S: Stream + Unpin>(
    stream: &mut S,
    cx: &mut Context<'_>,
    mut take: impl FnMut(S::Item),
) -> bool {
    for _ in 0..DRAIN_CAP {
        match Pin::new(&mut *stream).poll_next(cx) {
            Poll::Ready(Some(item)) => take(item),
            Poll::Ready(None) => return true,
            Poll::Pending => return false,
        }
    }
    cx.waker().wake_by_ref();
    false
}

/// Queues the player `name` to be read, behind the others, unless it waits already. The scan
/// is as long as the queue, which holds at most every player once.
fn mark(to_read: &mut VecDeque<String>, name: &str) {
    if !to_read.iter().any(|queued| queued == name) {
        to_read.push_back(name.to_owned());
    }
}
