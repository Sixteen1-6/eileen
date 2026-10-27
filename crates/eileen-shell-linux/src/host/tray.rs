//! The tray holder behind `set_tray`: one task on the bridge's thread that keeps Eileen's
//! [`StatusNotifier`] on a session connection of its own, registered with the watcher whenever
//! one is there, and passes the menu's choices to the shell's events ([`super`]'s doc, "The
//! tray").
//!
//! The holder registers beside its other work, never in turn with it, and publishes where the
//! item stands as a [`Status`]. A call gets that status once the holder has shown its state, and
//! waits for a registration under way only until [`answer_by`], so what the watcher does never
//! holds a `set_tray` past its deadline.

use std::future::Future;
use std::pin::Pin;

use eileen_dbus_linux::{Buses, Names, OwnerChanges, name_owner, owner_changes};
use eileen_platform::{Deadline, PlatformError, TrayState};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use super::{EventSink, LinuxShellEvent, TRAY};
use crate::tray::{StatusNotifier, WATCHER};
use crate::{DbusError, TraySettings};

/// How many asks wait for the holder, at most.
pub(super) const ASKS: usize = 8;
/// How many choices wait for the holder, at most: past that, the menu drops a click.
const CHOICES: usize = 16;
/// How many times the holder registers again with a watcher that answered with an error, one
/// for each later ask, before it waits for the name's next owner: enough for a watcher that was
/// not ready yet, and no call for every ask to one that refuses for good.
const RETRIES: u8 = 3;
/// The detail of a call whose holder ended without answering: its connection closed, it
/// panicked, or the bridge stopped.
const HOLDER_GONE: &str = "the tray holder ended without answering";

/// Where the item stands with the watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// A registration is under way.
    Registering,
    /// The watcher has the item.
    Registered,
    /// No watcher has it: there is none, or it refused or did not answer.
    Unregistered,
}

/// One `set_tray`: the state to show, and where to answer, once it is shown, with the item's
/// status to follow.
#[derive(Debug)]
pub(super) struct Ask {
    state: TrayState,
    shown: oneshot::Sender<watch::Receiver<Status>>,
}

/// A holder that has not started: the asks it takes, and what it needs to serve the item.
#[derive(Debug)]
pub(super) struct Holder {
    pub(super) asks: mpsc::Receiver<Ask>,
    /// The item's application ID: the desktop entry toasts give.
    pub(super) id: String,
    pub(super) settings: TraySettings,
    pub(super) sink: Option<EventSink>,
}

/// How long a call that started at `start` with `deadline` waits for a registration under way:
/// until half the deadline (`None` for no bound, past what an instant holds). The other half is
/// for what the call does before, the holder's connect and its first calls when the call starts
/// one, and leaves the answer before the bridge gives up on the call.
pub(super) fn answer_by(start: Instant, deadline: Deadline) -> Option<Instant> {
    start.checked_add(deadline.budget() / 2)
}

/// What `set_tray` does on the bridge's thread: starts `holder` on `state`, when the call brings
/// one, or else asks the holder on `asks` to show `state`. `Ok` once a watcher has the item, and
/// `Unavailable` while none does, the item staying on the bus to register once one comes. A
/// registration still under way at `by` counts as none.
pub(super) async fn show(
    buses: Buses,
    asks: mpsc::Sender<Ask>,
    holder: Option<Holder>,
    state: TrayState,
    by: Option<Instant>,
) -> Result<(), PlatformError> {
    let gone = || PlatformError::Io { what: TRAY, detail: HOLDER_GONE.to_owned() };
    let (shown, answer) = oneshot::channel();
    let ask = Ask { state, shown };
    match holder {
        Some(holder) => start(&buses, holder, ask).await?,
        None => asks.send(ask).await.map_err(|_closed| gone())?,
    }
    drop(buses);
    let mut status = answer.await.map_err(|_gone| gone())?;
    let settled = status.wait_for(|status| *status != Status::Registering);
    let registered = match by {
        Some(by) => tokio::time::timeout_at(by, settled).await.ok(),
        None => Some(settled.await),
    };
    match registered {
        Some(Ok(status)) if *status == Status::Registered => Ok(()),
        Some(Ok(_)) | None => Err(PlatformError::Unavailable { what: TRAY }),
        Some(Err(_ended)) => Err(gone()),
    }
}

/// Serves the item on a session connection of the holder's own, shows `first`'s state, follows
/// the watcher's owner from before it asks who that is, and leaves the rest, the registration
/// and `first`'s answer among it, to the holder's task. Cut short, it drops the connection, and
/// with it everything it served.
async fn start(buses: &Buses, holder: Holder, first: Ask) -> Result<(), DbusError> {
    let Holder { asks, id, settings, sink } = holder;
    let conn = buses.unshared_session().await?;
    let (choose, choices) = mpsc::channel(CHOICES);
    let item = StatusNotifier::serve(&conn, &id, settings, choose).await?;
    // Before any watcher hears of the item, so that a host's first read shows the state.
    item.show(&first.state).await;
    let watchers = owner_changes(&conn, Names::Name(WATCHER)).await?;
    let watched = name_owner(&conn, WATCHER).await?.is_some();
    let status = watch::Sender::new(Status::Unregistered);
    let mut held =
        Held { item, watchers, asks, choices, sink, status, registering: None, retries: None };
    if watched {
        held.register();
    }
    let _ignored = first.shown.send(held.status.subscribe());
    drop(tokio::task::spawn_local(held.run()));
    Ok(())
}

/// A registration under way, which holds no borrow of the holder.
type Registration = Pin<Box<dyn Future<Output = Result<(), DbusError>>>>;

/// A started holder: the item, the watcher's owner changes, its channels, and where the item
/// stands.
struct Held {
    item: StatusNotifier,
    watchers: OwnerChanges,
    asks: mpsc::Receiver<Ask>,
    choices: mpsc::Receiver<String>,
    sink: Option<EventSink>,
    /// Where the item stands, which each ask's answer follows.
    status: watch::Sender<Status>,
    /// The registration under way, if one is.
    registering: Option<Registration>,
    /// How many more asks may register again, once the watcher has answered a registration
    /// with an error: `None` before that, and again from each new owner of its name.
    retries: Option<u8>,
}

impl Held {
    /// Settles each registration, registers again for each new owner of the watcher's name,
    /// shows each ask's state, registering again first when the watcher refused and retries are
    /// left, and answers it with the status, and passes each choice to the sink. It ends when the
    /// shell drops its end of the asks or the connection closes, and the connection, its last
    /// holder, closes with it.
    async fn run(mut self) {
        loop {
            tokio::select! {
                outcome = settled(&mut self.registering) => self.settle(&outcome),
                ask = self.asks.recv() => {
                    let Some(ask) = ask else { break };
                    self.show(ask).await;
                }
                change = self.watchers.next() => {
                    let Some(change) = change else { break };
                    self.retries = None;
                    if change.new.is_some() {
                        self.register();
                    } else {
                        self.registering = None;
                        self.status.send_replace(Status::Unregistered);
                    }
                }
                Some(choice) = self.choices.recv() => {
                    if let Some(EventSink(sink)) = &self.sink {
                        sink(LinuxShellEvent::TrayChoice(choice));
                    }
                }
            }
        }
    }

    /// Starts a registration, in place of any under way.
    fn register(&mut self) {
        self.registering = Some(Box::pin(self.item.register()));
        self.status.send_replace(Status::Registering);
    }

    /// Takes a registration's `outcome`. The first error answer of an owner's, from a watcher
    /// that is there and said no, gives the next asks [`RETRIES`] registrations; no answer, or a
    /// watcher that has gone, ends them, leaving the item to the name's next owner.
    fn settle(&mut self, outcome: &Result<(), DbusError>) {
        self.registering = None;
        let refused = matches!(
            outcome,
            Err(DbusError::Refused { .. } | DbusError::NotFound { .. } | DbusError::Invalid { .. })
        );
        if refused {
            self.retries.get_or_insert(RETRIES);
        } else if outcome.is_err() {
            self.retries = None;
        }
        let status = if outcome.is_ok() { Status::Registered } else { Status::Unregistered };
        self.status.send_replace(status);
    }

    /// Shows `ask`'s state, registers again when a retry is due, and answers with the status.
    async fn show(&mut self, Ask { state, shown }: Ask) {
        self.item.show(&state).await;
        let due = self.registering.is_none() && *self.status.borrow() == Status::Unregistered;
        if due && let Some(left) = self.retries.and_then(|left| left.checked_sub(1)) {
            self.retries = Some(left);
            self.register();
        }
        let _ignored = shown.send(self.status.subscribe());
    }
}

/// The outcome of the registration under way, once it has one; with none, never. Cancel-safe:
/// the registration stays where it is.
async fn settled(registering: &mut Option<Registration>) -> Result<(), DbusError> {
    match registering.as_mut() {
        Some(under_way) => under_way.await,
        None => std::future::pending().await,
    }
}
