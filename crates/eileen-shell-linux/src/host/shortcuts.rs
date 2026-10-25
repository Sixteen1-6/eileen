//! The shortcuts holder behind `register_hotkey` and `watch_input`: one task on the bridge's
//! thread that keeps Eileen's shortcuts in a `GlobalShortcuts` portal session ([`super`]'s doc,
//! "Hotkeys and the dictation key"), with what the shell's events and settings name them by.

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use eileen_contracts::core_io::KeyEdge;
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::{Buses, Stream};
use eileen_platform::Deadline;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::{BoundShortcut, DbusError, Shortcut, ShortcutEvent, ShortcutEvents, ShortcutsSession};

/// How long the set waits after a change for the next one before it is bound, so that the
/// calls Eileen makes as it starts bind together, in one dialog.
pub const SETTLE: Duration = Duration::from_millis(100);

/// How long a bind waits for the desktop's answer, past which its request is closed, which
/// dismisses the dialog: GNOME's has no timeout of its own.
pub const CONSENT_WAIT: Duration = Duration::from_secs(300);

/// How many asks wait for the holder, at most.
pub(super) const ASKS: usize = 8;
/// How many key edges wait for a listener, at most: one that falls this far behind is dropped.
pub(super) const EDGES: usize = 32;

/// The ID the desktop keeps the dictation key's trigger under, and the prefix of each hotkey's.
const DICTATION: &str = "dictation";
const HOTKEY: &str = "hotkey-";
/// What the holder's own errors name.
const HOLDER: &str = "GlobalShortcuts holder";
const SESSION: &str = "GlobalShortcuts session";

/// What the shell reports beyond `ShellHost`'s answers, to the sink of
/// [`LinuxShell::with_events`](super::LinuxShell::with_events).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxShellEvent {
    /// The hotkey `register_hotkey` was given this ID for went down.
    Hotkey(u32),
    /// The shortcuts the desktop bound, as a bind's answer, or those whose triggers the user has
    /// changed since: each with its trigger.
    ShortcutsBound(Vec<BoundHotkey>),
    /// The desktop bound none: the user cancelled, the portal refused, no answer came within
    /// [`CONSENT_WAIT`], or a new session for a changed set could not open.
    ShortcutsRefused(DbusError),
}

/// One of Eileen's shortcuts, as the desktop bound it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundHotkey {
    /// Which one.
    pub id: ShortcutId,
    /// How to trigger it, in the desktop's words, such as GNOME's "Press Ctrl+Alt+M"; empty when
    /// it has none yet.
    pub trigger: Untrusted<String>,
}

/// Which of Eileen's shortcuts a bound one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShortcutId {
    /// The hotkey `register_hotkey` was given this ID for: the portal's `hotkey-<id>`.
    Hotkey(u32),
    /// The dictation key of `watch_input`: the portal's `dictation`.
    Dictation,
}

impl ShortcutId {
    /// The ID the portal knows it by.
    fn portal_id(self) -> String {
        match self {
            Self::Hotkey(id) => format!("{HOTKEY}{id}"),
            Self::Dictation => DICTATION.to_owned(),
        }
    }

    /// The shortcut a portal ID names; `None` for one Eileen does not bind.
    fn of(id: &str) -> Option<Self> {
        if id == DICTATION {
            return Some(Self::Dictation);
        }
        id.strip_prefix(HOTKEY)?.parse().ok().map(Self::Hotkey)
    }
}

/// What the desktop's dialog shows beside each of Eileen's shortcuts: `ShellHost` gives a
/// chord and an ID, and no description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutNames {
    /// Each hotkey's description, by its `register_hotkey` ID; one not listed is "Eileen
    /// shortcut <ID>".
    pub hotkeys: BTreeMap<u32, String>,
    /// The dictation key's: "Hold to dictate" by default.
    pub dictation: String,
}

impl Default for ShortcutNames {
    fn default() -> Self {
        Self { hotkeys: BTreeMap::new(), dictation: "Hold to dictate".to_owned() }
    }
}

impl ShortcutNames {
    /// The shortcut `id` stands for, offered with `trigger`.
    pub(super) fn shortcut(&self, id: ShortcutId, trigger: Option<String>) -> Shortcut {
        let description = match id {
            ShortcutId::Hotkey(number) => self
                .hotkeys
                .get(&number)
                .cloned()
                .unwrap_or_else(|| format!("Eileen shortcut {number}")),
            ShortcutId::Dictation => self.dictation.clone(),
        };
        Shortcut { id: id.portal_id(), description, trigger }
    }
}

/// The composition root's sink for [`LinuxShellEvent`]s, shown by its type alone.
#[derive(Clone)]
pub(super) struct EventSink(pub(super) Arc<dyn Fn(LinuxShellEvent) + Send + Sync>);

impl fmt::Debug for EventSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EventSink")
    }
}

/// What a shell call asks the holder, with where its answer goes: `Ok` once the holder has a
/// session that will bind the shortcut.
#[derive(Debug)]
pub(super) enum Ask {
    /// Bind `shortcut`, a hotkey, with the rest of the set.
    Add { shortcut: Shortcut, answer: Answer },
    /// Bind `shortcut`, the dictation key, with the rest, and send its edges to `edges`.
    Listen { shortcut: Shortcut, edges: mpsc::Sender<KeyEdge>, answer: Answer },
}

/// Where an ask's answer goes.
pub(super) type Answer = oneshot::Sender<Result<(), DbusError>>;

/// What a new holder takes from the shell that starts it.
#[derive(Debug)]
pub(super) struct Holder {
    /// The asks, from every call while the holder lives.
    pub(super) asks: mpsc::Receiver<Ask>,
    /// When the call that starts it began: the first session opens within that call's deadline.
    pub(super) start: Instant,
    /// The shell's deadline, which bounds opening each session.
    pub(super) deadline: Deadline,
    /// The app ID the portal's host registry is given.
    pub(super) app_id: String,
    pub(super) sink: Option<EventSink>,
}

/// One `watch_input`'s dictation-key edges, as the bridge's watch takes them.
#[derive(Debug)]
pub(super) struct Edges(pub(super) mpsc::Receiver<KeyEdge>);

impl Stream for Edges {
    type Item = KeyEdge;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<KeyEdge>> {
        self.0.poll_recv(cx)
    }
}

/// Hands the holder the ask `make` builds around its answer, first starting the holder on
/// `buses` when `holder` is a new one, and waits for the answer.
pub(super) async fn put(
    buses: Buses,
    asks: mpsc::Sender<Ask>,
    holder: Option<Holder>,
    make: impl FnOnce(Answer) -> Ask,
) -> Result<(), DbusError> {
    if let Some(holder) = holder {
        drop(tokio::task::spawn_local(hold(buses, holder)));
    }
    let gone = || DbusError::Io { what: HOLDER, detail: "it ended without answering".to_owned() };
    let (answer, answered) = oneshot::channel();
    asks.send(make(answer)).await.map_err(|_ended| gone())?;
    answered.await.unwrap_or_else(|_ended| Err(gone()))
}

/// The holder: on a session connection of its own, so that its close ends the session and the
/// registry's call is the connection's first, it opens a session and serves it, and a new one
/// each time a changed set must be bound again. It ends when the shell's end of the asks drops,
/// when the portal ends the session, or when a session cannot open, which every ask then
/// waiting is answered with; the next ask starts a new holder.
async fn hold(buses: Buses, holder: Holder) {
    let Holder { mut asks, start, deadline, app_id, sink } = holder;
    let report = |event: LinuxShellEvent| {
        if let Some(EventSink(sink)) = &sink {
            sink(event);
        }
    };
    let mut until = start.checked_add(deadline.budget());
    let conn = match bounded(until, buses.unshared_session()).await {
        Ok(conn) => conn,
        Err(error) => return refuse(asks, &error),
    };
    drop(buses);
    let (mut wanted, mut again) = (Wanted::default(), false);
    loop {
        let (session, mut events) = match bounded(until, open(&conn, &app_id, until)).await {
            Ok(opened) => opened,
            Err(error) => {
                if again {
                    report(LinuxShellEvent::ShortcutsRefused(error.clone()));
                }
                return refuse(asks, &error);
            }
        };
        let served = serve(&session, &mut events, &mut asks, &mut wanted, &report, again).await;
        drop(events);
        let _ignored = session.close().await;
        if let Err(ended) = served {
            return refuse(asks, &ended);
        }
        (again, until) = (true, Instant::now().checked_add(deadline.budget()));
    }
}

/// A session on `conn` and its events, subscribed before any bind, by `until`.
async fn open(
    conn: &zbus::Connection,
    app_id: &str,
    until: Option<Instant>,
) -> Result<(ShortcutsSession, ShortcutEvents), DbusError> {
    let left = until.map_or(Duration::MAX, |until| until.saturating_duration_since(Instant::now()));
    let session = ShortcutsSession::open(conn, app_id, left).await?;
    let events = session.events().await?;
    Ok((session, events))
}

/// What `work` answers by `until` (`None` for no bound): past it, the session it was opening is
/// late.
async fn bounded<T>(
    until: Option<Instant>,
    work: impl Future<Output = Result<T, DbusError>>,
) -> Result<T, DbusError> {
    match until {
        Some(until) => tokio::time::timeout_at(until, work)
            .await
            .unwrap_or(Err(DbusError::DeadlineExceeded { call: SESSION })),
        None => work.await,
    }
}

/// A bind under way: the desktop's answer, once the user has seen its dialog.
type Binding = Pin<Box<dyn Future<Output = Result<Vec<BoundShortcut>, DbusError>>>>;

/// Serves `session`: takes each ask in, binds the whole set [`SETTLE`] after its last change
/// (at once when `again`, for a set already settled), and hands the session's events on. After
/// a bind the desktop refused, any ask binds again, so a call made again shows the dialog
/// again. `Ok` asks for a new session, for a set to bind once this one has had its bind; `Err`
/// ends the holder, with what the asks still waiting are answered.
async fn serve(
    session: &ShortcutsSession,
    events: &mut ShortcutEvents,
    asks: &mut mpsc::Receiver<Ask>,
    wanted: &mut Wanted,
    report: &impl Fn(LinuxShellEvent),
    again: bool,
) -> Result<(), DbusError> {
    let mut bind_at = again.then(Instant::now);
    let (mut binding, mut sent, mut refused): (Option<Binding>, _, _) = (None, false, false);
    loop {
        tokio::select! {
            ask = asks.recv() => {
                let ask = ask.ok_or(DbusError::Unavailable { what: HOLDER })?;
                if wanted.take(ask) || refused {
                    bind_at = Instant::now().checked_add(SETTLE);
                }
            }
            event = events.next() => match event {
                Some(ShortcutEvent::Closed) | None => {
                    return Err(DbusError::Unavailable { what: SESSION });
                }
                Some(event) => wanted.hand_on(event, report),
            },
            () = tokio::time::sleep_until(bind_at.unwrap_or_else(Instant::now)),
                if bind_at.is_some() && binding.is_none() =>
            {
                if sent {
                    return Ok(());
                }
                (bind_at, sent) = (None, true);
                let (session, shortcuts) = (session.clone(), wanted.set());
                let bind = async move { session.bind(&shortcuts, CONSENT_WAIT).await };
                binding = Some(Box::pin(bind));
            }
            answer = async {
                match binding.as_mut() {
                    Some(bind) => bind.await,
                    None => std::future::pending().await,
                }
            }, if binding.is_some() => {
                binding = None;
                refused = answer.is_err();
                report(match answer {
                    Ok(bound) => LinuxShellEvent::ShortcutsBound(hotkeys(bound)),
                    Err(error) => LinuxShellEvent::ShortcutsRefused(error),
                });
            }
        }
    }
}

/// The set the holder binds, kept across its sessions, and where the dictation key's edges go.
#[derive(Debug, Default)]
struct Wanted {
    /// The shortcuts, by their portal IDs.
    shortcuts: BTreeMap<String, Shortcut>,
    /// One sender for each `watch_input` that is still subscribed.
    listeners: Vec<mpsc::Sender<KeyEdge>>,
}

impl Wanted {
    /// Takes `ask` in and answers it; says whether the set changed. A caller that has stopped
    /// waiting was answered `DeadlineExceeded` by the bridge, so its ask is passed over.
    fn take(&mut self, ask: Ask) -> bool {
        let (shortcut, answer, edges) = match ask {
            Ask::Add { shortcut, answer } => (shortcut, answer, None),
            Ask::Listen { shortcut, edges, answer } => (shortcut, answer, Some(edges)),
        };
        if answer.send(Ok(())).is_err() {
            return false;
        }
        self.listeners.extend(edges);
        self.shortcuts.insert(shortcut.id.clone(), shortcut.clone()).as_ref() != Some(&shortcut)
    }

    /// The whole set, in the order of its IDs.
    fn set(&self) -> Vec<Shortcut> {
        self.shortcuts.values().cloned().collect()
    }

    /// Hands `event` on: a hotkey's press to `report`, the dictation key's edges to each
    /// listener, and a change of the bindings to `report` as the new bindings. A hotkey's
    /// release says nothing.
    fn hand_on(&mut self, event: ShortcutEvent, report: &impl Fn(LinuxShellEvent)) {
        match event {
            ShortcutEvent::Activated(id) => match ShortcutId::of(&id) {
                Some(ShortcutId::Hotkey(number)) => report(LinuxShellEvent::Hotkey(number)),
                Some(ShortcutId::Dictation) => self.send(KeyEdge::Down),
                None => {}
            },
            ShortcutEvent::Deactivated(id) => {
                if ShortcutId::of(&id) == Some(ShortcutId::Dictation) {
                    self.send(KeyEdge::Up);
                }
            }
            ShortcutEvent::Changed(bound) => {
                report(LinuxShellEvent::ShortcutsBound(hotkeys(bound)));
            }
            ShortcutEvent::Closed => {}
        }
    }

    /// Sends `edge` to each listener, dropping one that is full or gone.
    fn send(&mut self, edge: KeyEdge) {
        self.listeners.retain(|listener| listener.try_send(edge).is_ok());
    }
}

/// Eileen's shortcuts among `bound`, each with its trigger.
fn hotkeys(bound: Vec<BoundShortcut>) -> Vec<BoundHotkey> {
    let hotkey = |bound: BoundShortcut| {
        Some(BoundHotkey { id: ShortcutId::of(&bound.id)?, trigger: bound.trigger })
    };
    bound.into_iter().filter_map(hotkey).collect()
}

/// Ends the holder: it takes no ask any more, and each one still waiting is answered `error`.
fn refuse(mut asks: mpsc::Receiver<Ask>, error: &DbusError) {
    asks.close();
    while let Ok(ask) = asks.try_recv() {
        let (Ask::Add { answer, .. } | Ask::Listen { answer, .. }) = ask;
        let _ignored = answer.send(Err(error.clone()));
    }
}
