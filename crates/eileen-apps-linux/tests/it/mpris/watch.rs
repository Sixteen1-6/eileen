//! `MprisSessions::subscribe`'s `MprisWatch` against fake players: the first states, changes,
//! arrivals, departures and the end, the order of its reads, and the races a listing meets.
//!
//! No test waits on the wall clock. Where a test shows that something was not reported, it
//! makes the bus deliver what came before (`delivered`, a round trip), then makes a change of
//! its own and checks that the watch reports that change next. The bus keeps each sender's
//! messages in order, so a wrong report would have come first.

use std::collections::HashMap;
use std::time::Duration;

use eileen_apps_linux::{MprisEvent, MprisSessions, MprisWatch};
use eileen_contracts::desktop::MediaState;
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::METHOD_TIMEOUT;
use eileen_dbus_linux::testing::{PrivateBus, next_message, nothing_ready, round_trip};
use tokio::time::Instant;
use zbus::message::Type;
use zbus::zvariant::Value;
use zbus::{MatchRule, MessageStream};

use super::fake::{
    FULL, Gate, OBJECT, ODD, Player, Root, full_player, full_root, full_state, interface,
    odd_player, odd_root, odd_state, serve, serve_with, track_path,
};

/// The playback interface.
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// The interface `PropertiesChanged` belongs to.
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// A player whose reads never answer; first by name, ahead of the full and the odd players.
const ASLEEP: &str = "org.mpris.MediaPlayer2.asleep";

/// The watch's next change; the watch stays open.
async fn next(watch: &mut MprisWatch) -> MprisEvent {
    watch.next().await.expect("the watch stays open")
}

/// Returns once every signal `sender` has sent so far is in `conn`'s streams. The bus handles
/// each connection's messages in order: once it has answered `sender`, it has passed on what
/// `sender` sent before, and its answer to `conn` after that comes behind those signals.
async fn delivered(sender: &zbus::Connection, conn: &zbus::Connection) {
    round_trip(sender, None).await;
    round_trip(conn, None).await;
}

/// Returns once each of `players` has answered both reads of its first state, its `Player`
/// properties and its root interface (`replies` is every message the watch's connection
/// receives). A connection hands each reply to the call that waits for it before any stream sees
/// it, so from then on those reads need nothing more from the bus.
async fn first_reads_answered(replies: &mut MessageStream, players: &[&zbus::Connection]) {
    let mut left: Vec<(String, usize)> =
        players.iter().map(|player| (player.unique_name().unwrap().to_string(), 2)).collect();
    while left.iter().any(|(_, answers)| *answers > 0) {
        let message = next_message(replies).await;
        let header = message.header();
        let Some(sender) = header.sender().filter(|_| message.message_type() == Type::MethodReturn)
        else {
            continue;
        };
        if let Some((_, answers)) = left.iter_mut().find(|(name, _)| name == sender.as_str()) {
            *answers = answers.saturating_sub(1);
        }
    }
}

#[tokio::test]
async fn the_watch_reports_each_player_then_each_change() {
    let bus = PrivateBus::start();
    let full = serve(&bus, FULL, full_root(), full_player()).await;
    let _odd = serve(&bus, ODD, odd_root(), odd_player()).await;
    let mut watch = MprisSessions::new(&bus.connect().await).subscribe().await.unwrap();
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(full_state()), "the first player");
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(odd_state()), "then the second");

    let player = interface(&full).await;
    player.get_mut().await.status = "Paused";
    player.get().await.playback_status_changed(player.signal_emitter()).await.unwrap();
    let paused = MediaState { playing: false, ..full_state() };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(paused.clone()), "paused");

    player.get_mut().await.position = Some(10_000_000);
    Player::seeked(player.signal_emitter(), 10_000_000).await.unwrap();
    let sought = MediaState { position_s: Some(10.0), ..paused };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(sought.clone()), "sought");

    player.get_mut().await.metadata = vec![
        ("mpris:trackid", track_path("/org/eileen/Track/2")),
        ("xesam:title", Value::from("Song Two")),
    ];
    player.get().await.metadata_changed(player.signal_emitter()).await.unwrap();
    let second = MediaState {
        title: Untrusted::new("Song Two".to_owned()),
        artist: None,
        duration_s: None,
        ..sought
    };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(second), "the next track");
}

#[tokio::test]
async fn a_player_that_never_answers_holds_back_no_other_players_read() {
    let bus = PrivateBus::start();
    let gate = Gate::default();
    let asleep = Player { gate: Some(gate.clone()), ..full_player() };
    let _asleep = serve(&bus, ASLEEP, full_root(), asleep).await;
    let full = serve(&bus, FULL, full_root(), full_player()).await;
    let odd = serve(&bus, ODD, odd_root(), odd_player()).await;
    let conn = bus.connect_within(METHOD_TIMEOUT).await;
    let mut replies = MessageStream::from(&conn);
    let mut watch = MprisSessions::new(&conn).subscribe().await.unwrap();

    // In real time, the two other players answer while the first one has not: read one after
    // the other, they would be asked only once its read had passed the method timeout, and the
    // first of them would be reported before the second was asked.
    let others = [&full, &odd];
    tokio::select! {
        () = first_reads_answered(&mut replies, &others) => {}
        event = watch.next() => panic!("a player was reported before the others were read: {event:?}"),
    }
    // Unpolled, the stream would hold up the connection once its queue is full.
    drop(replies);
    // Only the stalled read's timeouts are left to fire, and tokio's paused clock reaches them as
    // soon as the runtime is idle.
    tokio::time::pause();
    let paused = Instant::now();
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(full_state()), "the full player");
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(odd_state()), "then the odd one");
    let waited = paused.elapsed();
    assert!(
        waited <= METHOD_TIMEOUT,
        "in bus name order, once the stalled read has passed its one method timeout: {waited:?}"
    );
}

#[tokio::test]
async fn the_watch_skips_a_name_without_the_player_interface() {
    let bus = PrivateBus::start();
    let broken = "org.mpris.MediaPlayer2.broken";
    let _broken = eileen_dbus_linux::testing::serve(&bus, broken, OBJECT, full_root()).await;
    let _full = serve(&bus, FULL, full_root(), full_player()).await;
    let mut watch = MprisSessions::new(&bus.connect().await).subscribe().await.unwrap();
    assert_eq!(
        next(&mut watch).await,
        MprisEvent::Changed(full_state()),
        "the broken player, first by name, is skipped"
    );
}

#[tokio::test]
async fn only_a_players_own_signals_read_it_and_an_unchanged_state_reports_nothing() {
    let bus = PrivateBus::start();
    let full = serve(&bus, FULL, full_root(), full_player()).await;
    let odd = serve(&bus, ODD, odd_root(), odd_player()).await;
    let stranger = bus.connect().await;
    let conn = bus.connect().await;
    let mut watch = MprisSessions::new(&conn).subscribe().await.unwrap();
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(full_state()), "the first player");
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(odd_state()), "then the second");

    // The first player stops without saying so, and a stranger sends both kinds of signal for
    // the players' object. A read they caused would report the stop before the second player's
    // seek, which comes after them.
    let first = interface(&full).await;
    first.get_mut().await.status = "Stopped";
    let nothing: (&str, HashMap<&str, Value<'_>>, Vec<&str>) = (PLAYER, HashMap::new(), Vec::new());
    stranger
        .emit_signal(None::<&str>, OBJECT, PROPERTIES, "PropertiesChanged", &nothing)
        .await
        .unwrap();
    stranger.emit_signal(None::<&str>, OBJECT, PLAYER, "Seeked", &(5_i64,)).await.unwrap();
    delivered(&stranger, &conn).await;
    let second = interface(&odd).await;
    second.get_mut().await.position = Some(7_000_000);
    Player::seeked(second.signal_emitter(), 7_000_000).await.unwrap();
    let sought = MediaState { position_s: Some(7.0), ..odd_state() };
    assert_eq!(
        next(&mut watch).await,
        MprisEvent::Changed(sought.clone()),
        "a stranger's signals are ignored"
    );

    // Any change on the player's own Player interface reads it again.
    first.get_mut().await.volume = 0.5;
    first.get().await.volume_changed(first.signal_emitter()).await.unwrap();
    let stopped = MediaState { playing: false, ..full_state() };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(stopped), "read on its own signal");

    // A read that finds the state as last reported reports nothing, so the next change is the
    // second player's, which comes after it.
    first.get_mut().await.volume = 0.25;
    first.get().await.volume_changed(first.signal_emitter()).await.unwrap();
    delivered(&full, &conn).await;
    second.get_mut().await.position = Some(8_000_000);
    Player::seeked(second.signal_emitter(), 8_000_000).await.unwrap();
    let later = MediaState { position_s: Some(8.0), ..sought };
    assert_eq!(
        next(&mut watch).await,
        MprisEvent::Changed(later),
        "the same state again is not reported"
    );
}

#[tokio::test]
async fn a_flood_of_signals_is_folded_and_never_stalls_the_watch() {
    let bus = PrivateBus::start();
    let full = serve(&bus, FULL, full_root(), full_player()).await;
    let odd = serve(&bus, ODD, odd_root(), odd_player()).await;
    let conn = bus.connect().await;
    let mpris = MprisSessions::new(&conn);
    let mut watch = mpris.subscribe().await.unwrap();
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(full_state()), "the first player");
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(odd_state()), "then the second");

    // More signals than the watch's queue holds, sent while nothing polls it.
    let player = interface(&full).await;
    player.get_mut().await.position = Some(5_000_000);
    for _ in 0..600 {
        Player::seeked(player.signal_emitter(), 5_000_000).await.unwrap();
    }
    let sought = MediaState { position_s: Some(5.0), ..full_state() };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(sought.clone()), "one change");

    // The rest of the flood reports nothing. Its signals are ahead of the second player's seek,
    // so a read they caused that reported the first player again would come first.
    delivered(&full, &conn).await;
    let second = interface(&odd).await;
    second.get_mut().await.position = Some(7_000_000);
    Player::seeked(second.signal_emitter(), 7_000_000).await.unwrap();
    let second_sought = MediaState { position_s: Some(7.0), ..odd_state() };
    assert_eq!(
        next(&mut watch).await,
        MprisEvent::Changed(second_sought.clone()),
        "no more for the rest of the flood"
    );

    // The first player's own signals still count, and the connection still answers.
    player.get_mut().await.status = "Paused";
    player.get().await.playback_status_changed(player.signal_emitter()).await.unwrap();
    let paused = MediaState { playing: false, ..sought };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(paused.clone()), "then the pause");
    assert_eq!(mpris.sessions().await.unwrap(), [paused, second_sought], "the listing");
}

#[tokio::test]
async fn the_watch_reports_players_that_arrive_leave_or_are_replaced() {
    let bus = PrivateBus::start();
    let mpris = MprisSessions::new(&bus.connect().await);
    let mut watch = mpris.subscribe().await.unwrap();
    let full = serve(&bus, FULL, full_root(), full_player()).await;
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(full_state()), "arrived");
    full.release_name(FULL).await.unwrap();
    let gone = MediaState { playing: false, ..full_state() };
    assert_eq!(next(&mut watch).await, MprisEvent::Gone(gone), "left, and no longer playing");
    assert_eq!(mpris.sessions().await.unwrap(), [], "and no longer listed");

    let _first = serve_with(&bus, ODD, odd_root(), odd_player(), |builder| {
        builder.allow_name_replacements(true)
    })
    .await;
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(odd_state()), "the first owner");
    let playing = Player { status: "Playing", ..odd_player() };
    let _second =
        serve_with(&bus, ODD, odd_root(), playing, |builder| builder.replace_existing_names(true))
            .await;
    assert_eq!(next(&mut watch).await, MprisEvent::Gone(odd_state()), "the first owner left");
    let replaced = MediaState { playing: true, ..odd_state() };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(replaced), "the second owner");
}

#[tokio::test]
async fn a_late_answer_from_an_owner_that_lost_the_name_is_dropped() {
    let bus = PrivateBus::start();
    let first = serve_with(&bus, ODD, odd_root(), odd_player(), |builder| {
        builder.allow_name_replacements(true)
    })
    .await;
    let mut watch = MprisSessions::new(&bus.connect().await).subscribe().await.unwrap();
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(odd_state()), "the first owner");

    // The first owner's next read waits at the gate while another connection takes the name.
    let gate = Gate::default();
    let player = interface(&first).await;
    player.get_mut().await.gate = Some(gate.clone());
    player.get().await.volume_changed(player.signal_emitter()).await.unwrap();
    tokio::select! {
        () = gate.entered.notified() => {}
        event = watch.next() => panic!("nothing to report while the read waits: {event:?}"),
    }
    let playing = Player { status: "Playing", ..odd_player() };
    let _second =
        serve_with(&bus, ODD, odd_root(), playing, |builder| builder.replace_existing_names(true))
            .await;
    gate.open.notify_one();
    assert_eq!(next(&mut watch).await, MprisEvent::Gone(odd_state()), "the first owner left");
    let replaced = MediaState { playing: true, ..odd_state() };
    assert_eq!(
        next(&mut watch).await,
        MprisEvent::Changed(replaced),
        "the second owner's state, not the first owner's late answer"
    );
}

#[tokio::test]
async fn the_watch_ends_when_the_bus_does() {
    let bus = PrivateBus::start();
    let mut watch = MprisSessions::new(&bus.connect().await).subscribe().await.unwrap();
    drop(bus);
    assert_eq!(watch.next().await, None, "no more changes once the connection has ended");
}

#[tokio::test]
async fn a_name_owner_change_the_bus_did_not_send_is_ignored() {
    let bus = PrivateBus::start();
    let full = serve(&bus, FULL, full_root(), full_player()).await;
    // A peer that serves a player of its own but owns no player's name.
    let forged = Player::new("Playing", vec![("xesam:title", Value::from("Forged"))], None);
    let stranger = zbus::connection::Builder::address(bus.address())
        .unwrap()
        .serve_at(OBJECT, Root { identity: Some("Stranger"), desktop_entry: None })
        .unwrap()
        .serve_at(OBJECT, forged)
        .unwrap()
        .build()
        .await
        .unwrap();
    let conn = bus.connect().await;
    let mut watch = MprisSessions::new(&conn).subscribe().await.unwrap();
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(full_state()), "the player");

    // The stranger sends NameOwnerChanged straight to the watch's connection, once taking the
    // player's name and once dropping it. The bus delivers a signal sent to one connection
    // whatever its match rules say; only the bus's own NameOwnerChanged counts.
    let watcher = conn.unique_name().unwrap().to_string();
    let owner = full.unique_name().unwrap().to_string();
    let thief = stranger.unique_name().unwrap().to_string();
    for new in [thief.as_str(), ""] {
        stranger
            .emit_signal(
                Some(watcher.as_str()),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "NameOwnerChanged",
                &(FULL, owner.as_str(), new),
            )
            .await
            .unwrap();
    }
    delivered(&stranger, &conn).await;

    // The real player is still followed: its pause is the next change, with nothing before it.
    let player = interface(&full).await;
    player.get_mut().await.status = "Paused";
    player.get().await.playback_status_changed(player.signal_emitter()).await.unwrap();
    let paused = MediaState { playing: false, ..full_state() };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(paused), "its own signals still count");
}

#[tokio::test]
async fn a_player_that_leaves_while_the_watch_lists_is_never_reported() {
    let bus = PrivateBus::start();
    let full = serve(&bus, FULL, full_root(), full_player()).await;
    let odd = serve(&bus, ODD, odd_root(), odd_player()).await;
    let conn = bus.connect().await;
    let mut watch = MprisSessions::new(&conn).subscribe().await.unwrap();

    // Step the listing by hand: each poll sends its next call, and a round trip to the bus
    // brings the answer in. The listing asks for the names, then who owns the first player.
    assert!(nothing_ready(&mut watch), "the listing asks for the names");
    round_trip(&conn, None).await;
    assert!(nothing_ready(&mut watch), "then who owns the first player");
    round_trip(&conn, None).await;
    // The first player leaves the bus while the listing still runs, though its connection
    // still serves it, and the bus's NameOwnerChanged reaches the watch before its next poll.
    full.release_name(FULL).await.unwrap();
    round_trip(&conn, None).await;
    assert_eq!(
        next(&mut watch).await,
        MprisEvent::Changed(odd_state()),
        "only the player still on the bus"
    );

    // Nor later: the second player's next change is the next one reported.
    let second = interface(&odd).await;
    second.get_mut().await.status = "Playing";
    second.get().await.playback_status_changed(second.signal_emitter()).await.unwrap();
    let playing = MediaState { playing: true, ..odd_state() };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(playing), "never the one that left");
}

#[tokio::test]
async fn a_player_that_changes_while_it_is_read_waits_behind_the_others() {
    let bus = PrivateBus::start();
    let full = serve(&bus, FULL, full_root(), full_player()).await;
    let odd = serve(&bus, ODD, odd_root(), odd_player()).await;
    let conn = bus.connect().await;
    let mut watch = MprisSessions::new(&conn).subscribe().await.unwrap();
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(full_state()), "the first player");
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(odd_state()), "then the second");

    // The first player, first by name, pauses, and its read waits at the gate.
    let gate = Gate::default();
    let busy = interface(&full).await;
    let mut player = busy.get_mut().await;
    player.status = "Paused";
    player.gate = Some(gate.clone());
    drop(player);
    busy.get().await.volume_changed(busy.signal_emitter()).await.unwrap();
    tokio::select! {
        () = gate.entered.notified() => {}
        event = watch.next() => panic!("nothing to report while the read waits: {event:?}"),
    }

    // Meanwhile the second player starts playing, and then the first changes again.
    let other = interface(&odd).await;
    other.get_mut().await.status = "Playing";
    other.get().await.playback_status_changed(other.signal_emitter()).await.unwrap();
    delivered(&odd, &conn).await;
    busy.get().await.volume_changed(busy.signal_emitter()).await.unwrap();
    delivered(&full, &conn).await;
    gate.open.notify_one();
    let paused = MediaState { playing: false, ..full_state() };
    assert_eq!(next(&mut watch).await, MprisEvent::Changed(paused), "the read at the gate");

    // The first player's next read would wait at the gate for good: the second goes first.
    let playing = MediaState { playing: true, ..odd_state() };
    tokio::select! {
        event = watch.next() => assert_eq!(
            event,
            Some(MprisEvent::Changed(playing)),
            "the player that changed first is read first"
        ),
        () = gate.entered.notified() => panic!("the player marked again was read first"),
    }
}

#[tokio::test]
async fn a_listing_that_fails_leaves_the_watch_open_and_the_next_signal_lists_again() {
    let bus = PrivateBus::start();
    let full = serve(&bus, FULL, full_root(), full_player()).await;
    let conn = bus.connect_within(Duration::from_secs(5)).await;
    let mut watch = MprisSessions::new(&conn).subscribe().await.unwrap();

    // Jam the connection: a stream that holds one signal and is never polled stops the
    // connection reading at the second, so the listing's call gets no answer. Once the bus has
    // answered the jammer, both signals are on their way, ahead of anything the listing asks.
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.eileen.Test")
        .unwrap()
        .member("Jam")
        .unwrap()
        .build();
    let jam = MessageStream::for_match_rule(rule, &conn, Some(1)).await.unwrap();
    let jammer = bus.connect().await;
    for _ in 0..2 {
        jammer
            .emit_signal(None::<&str>, "/org/eileen/Test", "org.eileen.Test", "Jam", &())
            .await
            .unwrap();
    }
    round_trip(&jammer, None).await;

    // The listing's method timeout is the only timer left, and tokio's paused clock reaches it
    // as soon as the runtime is idle: the listing fails, and the watch stays open.
    tokio::time::pause();
    let listing = tokio::time::timeout(Duration::from_secs(10), watch.next()).await;
    assert!(listing.is_err(), "the watch stays open when its listing fails: {listing:?}");
    tokio::time::resume();

    // Unjammed, the player that was there all along signals, and the watch lists again.
    drop(jam);
    let player = interface(&full).await;
    player.get().await.volume_changed(player.signal_emitter()).await.unwrap();
    assert_eq!(
        next(&mut watch).await,
        MprisEvent::Changed(full_state()),
        "the player, found by the second listing"
    );
}
