//! `LinuxMedia`, the `MediaSessions` trait over MPRIS, through a bridge on the private bus. The
//! test's own thread calls the trait as its caller does, blocking; the fake players run on a
//! runtime of their own, whose workers serve them meanwhile. What `MprisSessions` and
//! `MprisWatch` report is pinned by the `sessions` and `watch` tests: these pin that the trait
//! reaches them, and what it adds, the conversions and the bridge's deadline.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use eileen_apps_linux::LinuxMedia;
use eileen_contracts::desktop::MediaState;
use eileen_contracts::tools::apps::MediaAction;
use eileen_dbus_linux::Bridge;
use eileen_dbus_linux::testing::PrivateBus;
use eileen_platform::{Deadline, MediaSessions, PlatformError};
use tokio::sync::mpsc;

use super::fake::{
    Call, FULL, Gate, ODD, Player, Root, calls, full_player, full_root, full_state, interface,
    odd_player, odd_root, odd_state, serve,
};

/// The deadline of a listing that a player holds for the connection's whole method timeout: it
/// passes whatever its length below that timeout, and a short one keeps the test short.
const SHORT: Deadline = Deadline::after(Duration::from_millis(100));

/// Players whose reads wait at a gate that never opens; by name, they come between the full
/// player and the odd one.
const HUNG: [&str; 2] = ["org.mpris.MediaPlayer2.hung1", "org.mpris.MediaPlayer2.hung2"];

/// Fake players by bus name, on a multi-thread runtime of their own.
struct Fakes {
    runtime: tokio::runtime::Runtime,
    players: BTreeMap<&'static str, zbus::Connection>,
}

impl Fakes {
    fn start() -> Self {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build();
        Self { runtime: runtime.unwrap(), players: BTreeMap::new() }
    }

    /// Serves a fake player called `name`.
    fn serve(&mut self, bus: &PrivateBus, name: &'static str, root: Root, player: Player) {
        let service = self.run(serve(bus, name, root, player));
        self.players.insert(name, service);
    }

    /// Serves a player called `name` whose reads wait at `gate`.
    fn serve_hung(&mut self, bus: &PrivateBus, name: &'static str, gate: &Gate) {
        self.serve(bus, name, full_root(), Player { gate: Some(gate.clone()), ..full_player() });
    }

    /// The connection of the player `name`.
    fn player(&self, name: &str) -> &zbus::Connection {
        &self.players[name]
    }

    /// Runs `work` on the fakes' runtime.
    fn run<T>(&self, work: impl Future<Output = T>) -> T {
        self.runtime.block_on(work)
    }

    /// Sets the player `name`'s `PlaybackStatus` to `status`, and says so on the bus.
    fn set_status(&self, name: &str, status: &'static str) {
        self.run(async {
            let player = interface(self.player(name)).await;
            player.get_mut().await.status = status;
            player.get().await.playback_status_changed(player.signal_emitter()).await.unwrap();
        });
    }
}

impl Drop for Fakes {
    fn drop(&mut self) {
        // The connections end inside their runtime's context, a panicking test's included.
        let _context = self.runtime.enter();
        self.players.clear();
    }
}

/// The media sessions on a new bridge to `bus`.
fn media(bus: &PrivateBus) -> LinuxMedia {
    LinuxMedia::new(Bridge::on_private_bus(bus).unwrap())
}

/// A `subscribe` sink that sends each state to the receiver.
fn sink() -> (Box<dyn Fn(MediaState) + Send + Sync>, mpsc::Receiver<MediaState>) {
    let (sender, states) = mpsc::channel(8);
    (Box::new(move |state| sender.try_send(state).unwrap()), states)
}

#[test]
fn sessions_lists_the_players_on_the_session_bus() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    fakes.serve(&bus, FULL, full_root(), full_player());
    let media = media(&bus);
    assert_eq!(media.sessions().unwrap(), [full_state()], "the player, as MprisSessions reads it");
}

#[test]
fn control_reaches_the_player_and_answers_platform_errors() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    fakes.serve(&bus, FULL, full_root(), full_player());
    let media = media(&bus);
    media.control(FULL, &MediaAction::Pause).unwrap();
    assert_eq!(fakes.run(calls(fakes.player(FULL))), [Call::Pause], "the player's Pause");
    assert_eq!(
        media.control("org.mpris.MediaPlayer2.absent", &MediaAction::Pause),
        Err(PlatformError::Unavailable { what: "media player" }),
        "a DbusError, as its PlatformError twin"
    );
}

#[test]
fn subscribe_reports_changes_arrivals_and_departures_until_the_subscription_drops() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    fakes.serve(&bus, ODD, odd_root(), odd_player());
    let media = media(&bus);
    let (sink, mut states) = sink();
    let subscription = media.subscribe(sink).unwrap();
    assert_eq!(states.blocking_recv(), Some(odd_state()), "the player already there");

    fakes.set_status(ODD, "Playing");
    let playing = MediaState { playing: true, ..odd_state() };
    assert_eq!(states.blocking_recv(), Some(playing), "its change");

    fakes.serve(&bus, FULL, full_root(), full_player());
    assert_eq!(states.blocking_recv(), Some(full_state()), "a player that arrives");
    // The other player changes after one leaves: the bus sends its NameOwnerChanged first, so
    // a departure that did not reach the sink would show as that change coming first.
    assert!(fakes.run(fakes.player(FULL).release_name(FULL)).unwrap(), "released");
    fakes.set_status(ODD, "Paused");
    let gone = MediaState { playing: false, ..full_state() };
    assert_eq!(states.blocking_recv(), Some(gone), "one that leaves: its last state, not playing");
    assert_eq!(states.blocking_recv(), Some(odd_state()), "then the other's change");

    drop(subscription);
    assert_eq!(states.blocking_recv(), None, "the watch ended and dropped the sink");
}

#[test]
fn a_listing_that_stalls_passes_its_deadline_and_the_bridge_goes_on() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    fakes.serve(&bus, FULL, full_root(), full_player());
    // A player that never answers holds the listing for the connection's whole method timeout,
    // far past the short deadline.
    fakes.serve_hung(&bus, HUNG[0], &Gate::default());
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    assert_eq!(
        LinuxMedia::new_within(bridge.clone(), SHORT).sessions(),
        Err(PlatformError::DeadlineExceeded { call: "media sessions" }),
        "the adapter's deadline, not a listing without the hung player a method timeout later"
    );
    // Another adapter on the same bridge, with the default deadline.
    let media = LinuxMedia::new(bridge);
    media.control(FULL, &MediaAction::Pause).unwrap();
    assert_eq!(fakes.run(calls(fakes.player(FULL))), [Call::Pause], "the bridge still serves");
}
