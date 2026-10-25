//! The tray holder behind `set_tray`: one task on the bridge's thread that keeps Eileen's
//! [`StatusNotifier`] on a session connection of its own, registered with the watcher whenever
//! one is there, and passes the menu's choices to the shell's events ([`super`]'s doc, "The
//! tray").

use eileen_dbus_linux::{Buses, Names, OwnerChanges, name_owner, owner_changes};
use eileen_platform::{PlatformError, TrayState};
use tokio::sync::{mpsc, oneshot};

use super::{EventSink, LinuxShellEvent, TRAY};
use crate::tray::{StatusNotifier, WATCHER};
use crate::{DbusError, TraySettings};

/// How many asks wait for the holder, at most.
pub(super) const ASKS: usize = 8;
/// How many choices wait for the holder, at most: past that, the menu drops a click.
const CHOICES: usize = 16;
/// The detail of a call whose holder ended without answering: its connection closed, it
/// panicked, or the bridge stopped.
const HOLDER_GONE: &str = "the tray holder ended without answering";

/// One `set_tray`: the state to show, and where to answer whether a watcher has the item.
#[derive(Debug)]
pub(super) struct Ask {
    state: TrayState,
    shown: oneshot::Sender<bool>,
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

/// What `set_tray` does on the bridge's thread: starts `holder`, when the call brings one,
/// then asks the holder on `asks` to show `state`. `Ok` once a watcher has the item, and
/// `Unavailable` while none does, the item staying on the bus to register once one comes.
pub(super) async fn show(
    buses: Buses,
    asks: mpsc::Sender<Ask>,
    holder: Option<Holder>,
    state: TrayState,
) -> Result<(), PlatformError> {
    if let Some(holder) = holder {
        start(&buses, holder).await?;
    }
    drop(buses);
    let gone = || PlatformError::Io { what: TRAY, detail: HOLDER_GONE.to_owned() };
    let (shown, answer) = oneshot::channel();
    asks.send(Ask { state, shown }).await.map_err(|_closed| gone())?;
    match answer.await {
        Ok(true) => Ok(()),
        Ok(false) => Err(PlatformError::Unavailable { what: TRAY }),
        Err(_gone) => Err(gone()),
    }
}

/// Serves the item on a session connection of the holder's own, follows the watcher's owner
/// from before it asks who that is, and leaves the rest to the holder's task. Cut short, it
/// drops the connection, and with it everything it served.
async fn start(buses: &Buses, holder: Holder) -> Result<(), DbusError> {
    let Holder { asks, id, settings, sink } = holder;
    let conn = buses.unshared_session().await?;
    let (choose, choices) = mpsc::channel(CHOICES);
    let item = StatusNotifier::serve(&conn, &id, settings, choose).await?;
    let watchers = owner_changes(&conn, Names::Name(WATCHER)).await?;
    let watched = name_owner(&conn, WATCHER).await?.is_some();
    let held = Held { item, watchers, asks, choices, sink };
    drop(tokio::task::spawn_local(held.run(watched)));
    Ok(())
}

/// A started holder: the item, the watcher's owner changes, and its channels.
struct Held {
    item: StatusNotifier,
    watchers: OwnerChanges,
    asks: mpsc::Receiver<Ask>,
    choices: mpsc::Receiver<String>,
    sink: Option<EventSink>,
}

impl Held {
    /// Registers the item when `watched`, a watcher being there, and once again for each new
    /// owner of the watcher's name; shows each ask's state, answering whether a watcher has
    /// the item; passes each choice to the sink. It ends when the shell drops its end of the
    /// asks or the connection closes, and the connection, its last holder, closes with it.
    async fn run(mut self, watched: bool) {
        let mut registered = watched && self.item.register().await.is_ok();
        loop {
            tokio::select! {
                ask = self.asks.recv() => {
                    let Some(Ask { state, shown }) = ask else { break };
                    self.item.show(&state).await;
                    let _ignored = shown.send(registered);
                }
                change = self.watchers.next() => {
                    let Some(change) = change else { break };
                    registered = change.new.is_some() && self.item.register().await.is_ok();
                }
                Some(choice) = self.choices.recv() => {
                    if let Some(EventSink(sink)) = &self.sink {
                        sink(LinuxShellEvent::TrayChoice(choice));
                    }
                }
            }
        }
    }
}
