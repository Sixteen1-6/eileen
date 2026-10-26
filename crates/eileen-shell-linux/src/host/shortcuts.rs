//! The shortcuts holder behind `register_hotkey` and `watch_input`: one task on the bridge's
//! thread that keeps Eileen's shortcuts in a `GlobalShortcuts` portal session ([`super`]'s doc,
//! "Hotkeys and the dictation key"), with what the settings name them by, and the shell's events,
//! the tray's choices among them.

use std::collections::{BTreeMap, BTreeSet};
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

/// How long a bind waits for the desktop's answer, since GNOME's dialog has no timeout of its
/// own; past it the request is closed and the bind is refused. GNOME leaves the dialog up all
/// the same: an accept that comes later is stored but binds nothing, and the next call that
/// binds the shortcut, or Eileen's next start, gets it without a dialog.
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
/// Why the holder ended when the portal closed its session.
const CLOSED: &str = "the portal closed it";

/// What the shell reports beyond `ShellHost`'s answers, to the sink of
/// [`LinuxShell::with_events`](super::LinuxShell::with_events).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxShellEvent {
    /// The hotkey `register_hotkey` was given this ID for went down: once for each press, though
    /// GNOME repeats its `Activated` while the key is held.
    ///
    /// A press names no device. The portal does not say whether a keyboard sent the chord
    /// (xdg-desktop-portal-gnome drops the device gnome-shell names), and input injected through
    /// libei, as the `RemoteDesktop` portal hands it to Eileen's desktop server or any app the
    /// user granted it, fires the shortcut too. So a press cannot meet the rule that the approve, deny
    /// and unmute hotkeys count only when a keyboard sent them (BUILD.md sections 2.5 and 2.9,
    /// and `UserInput`'s doc in `eileen-contracts`): a root must not map one to `Approve`,
    /// `Deny` or an unmuting `SetMute` or `ToggleMute` until the owner decides how Linux meets
    /// it.
    Hotkey(u32),
    /// The shortcuts the desktop bound, as a bind's answer, each with its trigger. A change the
    /// user makes later in the desktop's settings is not reported yet.
    ShortcutsBound(Vec<BoundHotkey>),
    /// The desktop bound none: the user dismissed its dialog or the portal refused, both of
    /// which GNOME answers with 2 ("refused or cancelled"), no answer came within
    /// [`CONSENT_WAIT`], or a new session could not open. A changed set is bound in a new
    /// session once the one before has closed, so the shortcuts bound before are off while its
    /// dialog waits. Once the desktop refuses the changed set, the holder binds the set it had
    /// again, which GNOME answers from what it stored, without a dialog, and the new shortcut
    /// waits for the next call.
    ShortcutsRefused(DbusError),
    /// The user chose the tray menu's item with this `TrayItem::id` (dbusmenu's `clicked`).
    ///
    /// Nothing says that a person chose it. Any program on the session bus may send the menu the
    /// same event, and the item cannot tell a device's click from it, so a choice cannot meet the
    /// rule that the shell emits `Approve`, `Deny` and an unmuting `SetMute` or `ToggleMute` from
    /// the tray only when the click came from a device (BUILD.md section 3.3, beside
    /// `UserInput`): a root must not map one to them until the owner decides how Linux meets it.
    TrayChoice(String),
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
    /// Each hotkey's description, by its `register_hotkey` ID; one not listed is `Eileen
    /// shortcut 7` for the ID 7.
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
    /// The shortcut `id` stands for, under the ID the portal knows it by, offered with `trigger`.
    pub(super) fn shortcut(&self, id: ShortcutId, trigger: Option<String>) -> Shortcut {
        let (id, description) = match id {
            ShortcutId::Hotkey(n) => {
                let named = self.hotkeys.get(&n).cloned();
                (format!("{HOTKEY}{n}"), named.unwrap_or_else(|| format!("Eileen shortcut {n}")))
            }
            ShortcutId::Dictation => (DICTATION.to_owned(), self.dictation.clone()),
        };
        Shortcut { id, description, trigger }
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

/// What a shell call asks the holder: to bind `shortcut` with the rest of the set and, for the
/// dictation key, to send its edges to `edges`. `answer` is `Ok` once the holder has a session
/// that will bind it.
#[derive(Debug)]
pub(super) struct Ask {
    shortcut: Shortcut,
    edges: Option<mpsc::Sender<KeyEdge>>,
    answer: oneshot::Sender<Result<(), DbusError>>,
}

/// What a new holder takes from the shell that starts it.
#[derive(Debug)]
pub(super) struct Holder {
    /// The asks, from every call while the holder lives.
    pub(super) asks: mpsc::Receiver<Ask>,
    /// The shell's deadline, which bounds connecting, and opening each session.
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

/// Asks the holder to bind `shortcut` and send its edges to `edges`, first starting the holder
/// on `buses` when `holder` is a new one, and waits for the answer.
pub(super) async fn put(
    buses: Buses,
    asks: mpsc::Sender<Ask>,
    holder: Option<Holder>,
    shortcut: Shortcut,
    edges: Option<mpsc::Sender<KeyEdge>>,
) -> Result<(), DbusError> {
    if let Some(holder) = holder {
        drop(tokio::task::spawn_local(hold(buses, holder)));
    }
    let gone = || DbusError::Io { what: HOLDER, detail: "it ended without answering".to_owned() };
    let (answer, answered) = oneshot::channel();
    asks.send(Ask { shortcut, edges, answer }).await.map_err(|_ended| gone())?;
    answered.await.unwrap_or_else(|_ended| Err(gone()))
}

/// The holder: on a session connection of its own, so that its close ends the session and the
/// registry's call is the connection's first, it opens a session and serves it, and a new one
/// each time a set must be bound again, the old one closed first so that its grabs stand in no
/// new one's way. It ends when the shell's end of the asks drops, when the portal ends the
/// session, or when a session cannot open, which every ask then waiting is answered with; the
/// next ask starts a new holder.
async fn hold(buses: Buses, holder: Holder) {
    let Holder { mut asks, deadline, app_id, sink } = holder;
    let report = |event: LinuxShellEvent| {
        if let Some(EventSink(sink)) = &sink {
            sink(event);
        }
    };
    // zbus bounds no connect, so the holder bounds its own, as the keep-awake holder does.
    let until = Instant::now().checked_add(deadline.budget());
    let conn = match super::within(until, super::SESSION_BUS, buses.unshared_session()).await {
        Ok(conn) => conn,
        Err(error) => return refuse(asks, &error),
    };
    drop(buses);
    // What a new session binds at once, when the holder opened it to bind again.
    let (mut wanted, mut rebind) = (Wanted::default(), None);
    loop {
        // The portal's answer waits the shell's deadline; its events come before any bind.
        let open = async {
            let session = ShortcutsSession::open(&conn, &app_id, deadline.budget()).await?;
            let events = session.events().await?;
            Ok::<_, DbusError>((session, events))
        };
        let (session, mut events) = match open.await {
            Ok(opened) => opened,
            Err(error) => {
                // The session before is closed, so the desktop now binds none of the set.
                if rebind.is_some() {
                    report(LinuxShellEvent::ShortcutsRefused(error.clone()));
                }
                return refuse(asks, &error);
            }
        };
        let first = rebind.take();
        let served = serve(&session, &mut events, &mut asks, &mut wanted, &report, first).await;
        drop(events);
        wanted.session_ended();
        let _ignored = session.close().await;
        match served {
            Ok(next) => rebind = Some(next),
            Err(ended) => return refuse(asks, &ended),
        }
    }
}

/// What a session the holder opened to bind again binds at once.
#[derive(Debug, Clone, Copy)]
enum Rebind {
    /// The whole set, as it then stands.
    Whole,
    /// The set the desktop bound last, which the close of the session that held it unbound.
    Accepted,
}

/// A bind under way: the set it sent, and the desktop's answer once the user has seen its
/// dialog.
type Binding =
    Pin<Box<dyn Future<Output = (Vec<Shortcut>, Result<Vec<BoundShortcut>, DbusError>)>>>;

/// Serves `session`: takes each ask in, binds the whole set [`SETTLE`] after the last ask that
/// finds the session holding another, and hands the session's events on; a session opened to
/// bind again binds `first` at once. A session takes one bind, so `Ok` asks for a new session
/// that binds again: the whole set, for an ask that came once this session had had its bind,
/// or the set the desktop bound last, once it refuses a changed set. So after a refusal any
/// call binds again, and shows the dialog again. `Err` ends the holder, with what the asks
/// still waiting are answered.
async fn serve(
    session: &ShortcutsSession,
    events: &mut ShortcutEvents,
    asks: &mut mpsc::Receiver<Ask>,
    wanted: &mut Wanted,
    report: &impl Fn(LinuxShellEvent),
    mut first: Option<Rebind>,
) -> Result<Rebind, DbusError> {
    let mut bind_at = first.is_some().then(Instant::now);
    let mut binding: Option<Binding> = None;
    // Whether this session has had its bind, and what it sent, unless the desktop refused it.
    let (mut had_bind, mut held) = (false, None::<Vec<Shortcut>>);
    loop {
        tokio::select! {
            ask = asks.recv() => {
                let ask = ask.ok_or_else(|| ended(HOLDER, "the shell is gone"))?;
                if wanted.take(ask) && held.as_ref() != Some(&wanted.whole()) {
                    bind_at = Instant::now().checked_add(SETTLE);
                }
            }
            event = events.next() => wanted.hand_on(event, report)?,
            () = tokio::time::sleep_until(bind_at.unwrap_or_else(Instant::now)),
                if bind_at.is_some() && binding.is_none() =>
            {
                if had_bind {
                    return Ok(Rebind::Whole);
                }
                (bind_at, had_bind) = (None, true);
                let list = match first.take() {
                    Some(Rebind::Accepted) => wanted.accepted.clone(),
                    Some(Rebind::Whole) | None => wanted.whole(),
                };
                held = Some(list.clone());
                let session = session.clone();
                binding = Some(Box::pin(async move {
                    let answer = session.bind(&list, CONSENT_WAIT).await;
                    (list, answer)
                }));
            }
            (list, answer) = async {
                match binding.as_mut() {
                    Some(bind) => bind.await,
                    None => std::future::pending().await,
                }
            }, if binding.is_some() => {
                binding = None;
                match answer {
                    Ok(bound) => {
                        report(LinuxShellEvent::ShortcutsBound(hotkeys(bound)));
                        wanted.accepted = list;
                    }
                    Err(error) => {
                        held = None;
                        report(LinuxShellEvent::ShortcutsRefused(error));
                        // The session before, which held them, is closed: bind them again.
                        if !wanted.accepted.is_empty() && list != wanted.accepted {
                            return Ok(Rebind::Accepted);
                        }
                    }
                }
            }
        }
    }
}

/// The set the holder binds, kept across its sessions, where the dictation key's edges go, and
/// which of Eileen's shortcuts are down in the session.
#[derive(Debug, Default)]
struct Wanted {
    /// The shortcuts, by their portal IDs.
    shortcuts: BTreeMap<String, Shortcut>,
    /// One sender for each `watch_input` that is still subscribed.
    listeners: Vec<mpsc::Sender<KeyEdge>>,
    /// The shortcuts that went down in this session and have not come up.
    down: BTreeSet<ShortcutId>,
    /// The set the desktop bound last, which a refused change of it binds again.
    accepted: Vec<Shortcut>,
}

impl Wanted {
    /// Takes `ask` in and answers it; says whether it was taken. A caller that has stopped
    /// waiting was answered `DeadlineExceeded` by the bridge, so its ask is passed over.
    fn take(&mut self, ask: Ask) -> bool {
        let Ask { shortcut, edges, answer } = ask;
        if answer.send(Ok(())).is_err() {
            return false;
        }
        self.listeners.extend(edges);
        self.shortcuts.insert(shortcut.id.clone(), shortcut);
        true
    }

    /// The whole set, in the order of its IDs.
    fn whole(&self) -> Vec<Shortcut> {
        self.shortcuts.values().cloned().collect()
    }

    /// Hands `event`, the session's next, on: a hotkey's press to `report`, and the dictation
    /// key's edges to each listener. Only a change is an edge: GNOME repeats `Activated` while a
    /// key is held, so one that is down already says nothing, nor does a hotkey's release. `Err`
    /// once the session has ended, the portal having closed it or its signals having stopped,
    /// which the asks still waiting are answered with.
    fn hand_on(
        &mut self,
        event: Option<ShortcutEvent>,
        report: &impl Fn(LinuxShellEvent),
    ) -> Result<(), DbusError> {
        let (id, down) = match event.ok_or_else(|| ended(SESSION, "its signals ended"))? {
            ShortcutEvent::Activated(id) => (id, true),
            ShortcutEvent::Deactivated(id) => (id, false),
            ShortcutEvent::Closed => return Err(ended(SESSION, CLOSED)),
        };
        let Some(shortcut) = ShortcutId::of(&id) else { return Ok(()) };
        let changed = if down { self.down.insert(shortcut) } else { self.down.remove(&shortcut) };
        match shortcut {
            _ if !changed => {}
            ShortcutId::Hotkey(number) if down => report(LinuxShellEvent::Hotkey(number)),
            ShortcutId::Hotkey(_) => {}
            ShortcutId::Dictation => self.send(if down { KeyEdge::Down } else { KeyEdge::Up }),
        }
        Ok(())
    }

    /// Ends the session's part: a dictation key still down is released, since the session's
    /// `Deactivated` can no longer come, and the next session starts with every key up.
    fn session_ended(&mut self) {
        if self.down.remove(&ShortcutId::Dictation) {
            self.send(KeyEdge::Up);
        }
        self.down.clear();
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

/// What the holder ends with when `what` ends for `detail`: an `Io` error, never `Unavailable`,
/// which a call would read as a desktop without the portal's `GlobalShortcuts`.
fn ended(what: &'static str, detail: &str) -> DbusError {
    DbusError::Io { what, detail: detail.to_owned() }
}

/// Ends the holder: it takes no ask any more, and each one still waiting is answered `error`.
fn refuse(mut asks: mpsc::Receiver<Ask>, error: &DbusError) {
    asks.close();
    while let Ok(ask) = asks.try_recv() {
        let _ignored = ask.answer.send(Err(error.clone()));
    }
}

#[cfg(test)]
mod tests {
    use eileen_platform::PlatformError;

    use super::{ShortcutEvent, Wanted};

    #[test]
    fn a_session_that_ends_reaches_a_waiting_call_as_no_missing_portal() {
        for event in [Some(ShortcutEvent::Closed), None] {
            let ended = Wanted::default().hand_on(event.clone(), &|_event| {}).unwrap_err();
            let answered = super::super::unavailable("global hotkey")(ended);
            assert!(
                matches!(answered, PlatformError::Io { what: "GlobalShortcuts session", .. }),
                "{event:?} reached a waiting call as {answered:?}, not as Unavailable"
            );
        }
    }
}
