//! `MprisSessions::sessions` and `MprisSessions::control` against fake players: the listing and
//! how leniently it reads odd players, each control call and each refusal, and a player that
//! never answers.

use std::pin::pin;
use std::time::Duration;

use eileen_apps_linux::{DbusError, MprisSessions};
use eileen_contracts::tools::apps::MediaAction;
use eileen_dbus_linux::next;
use eileen_dbus_linux::testing::PrivateBus;
use zbus::MessageStream;
use zbus::message::Type;
use zbus::zvariant::OwnedObjectPath;

use super::fake::{
    Call, FLOAT, FULL, Gate, OBJECT, ODD, Player, TRACK, calls, float_player, float_root,
    float_state, full_player, full_root, full_state, interface, named, odd_player, odd_root,
    odd_state, serve,
};

#[tokio::test]
async fn sessions_lists_every_player_with_what_it_reports() {
    let bus = PrivateBus::start();
    let _full = serve(&bus, FULL, full_root(), full_player()).await;
    let _odd = serve(&bus, ODD, odd_root(), odd_player()).await;
    let _float = serve(&bus, FLOAT, float_root(), float_player()).await;
    let _not_a_player = named(&bus, "org.eileen.Test.NotAPlayer").await;
    let _the_prefix_alone = named(&bus, "org.mpris.MediaPlayer2").await;
    let mpris = MprisSessions::new(&bus.connect().await);
    assert_eq!(
        mpris.sessions().await.unwrap(),
        [float_state(), full_state(), odd_state()],
        "the three players, by bus name, each as leniently as it needs"
    );
}

#[tokio::test]
async fn a_bus_without_players_has_no_sessions_and_a_missing_player_is_unavailable() {
    let bus = PrivateBus::start();
    let mpris = MprisSessions::new(&bus.connect().await);
    assert_eq!(mpris.sessions().await.unwrap(), [], "no players");
    assert_eq!(
        mpris.control("org.mpris.MediaPlayer2.absent", &MediaAction::Pause).await,
        Err(DbusError::Unavailable { what: "media player" }),
        "nothing owns the name"
    );
    let not_players = [
        "org.eileen.Test.NotAPlayer",
        "org.mpris.MediaPlayer2",
        "org.mpris.MediaPlayer2.not a name",
        "not a bus name",
        "",
    ];
    for session in not_players {
        assert!(
            matches!(
                mpris.control(session, &MediaAction::Play).await,
                Err(DbusError::Invalid { what: "media session", .. })
            ),
            "{session:?} is not an MPRIS player's name"
        );
    }
}

#[tokio::test]
async fn a_name_without_the_player_interface_is_not_a_session() {
    let bus = PrivateBus::start();
    let broken = "org.mpris.MediaPlayer2.broken";
    let _broken = eileen_dbus_linux::testing::serve(&bus, broken, OBJECT, full_root()).await;
    let _full = serve(&bus, FULL, full_root(), full_player()).await;
    let mpris = MprisSessions::new(&bus.connect().await);
    assert_eq!(mpris.sessions().await.unwrap(), [full_state()], "only the real player");
    assert_eq!(
        mpris.control(broken, &MediaAction::Play).await,
        Err(DbusError::NotFound { what: "media player" }),
        "no Player interface"
    );
}

#[tokio::test]
async fn a_reply_that_is_not_a_property_map_is_left_out() {
    let bus = PrivateBus::start();
    let junk = named(&bus, "org.mpris.MediaPlayer2.junk").await;
    let mut calls = MessageStream::from(&junk);
    let answer = async {
        // Every call gets a string where a property map belongs.
        while let Some(call) = next(&mut calls).await {
            if call.message_type() == Type::MethodCall {
                junk.reply(&call.header(), &("not a property map",)).await.unwrap();
            }
        }
    };
    let mpris = MprisSessions::new(&bus.connect().await);
    let client = async {
        let sessions = mpris.sessions().await;
        let control = mpris.control("org.mpris.MediaPlayer2.junk", &MediaAction::Play).await;
        (sessions, control)
    };
    let (sessions, control) = tokio::select! {
        () = answer => panic!("the junk player stopped answering"),
        results = client => results,
    };
    assert_eq!(sessions.unwrap(), [], "the junk player is left out");
    assert!(
        matches!(control, Err(DbusError::Invalid { what: "media player", .. })),
        "and cannot be controlled: {control:?}"
    );
}

#[tokio::test]
async fn a_player_that_never_answers_is_left_out_and_passes_its_deadline() {
    let bus = PrivateBus::start();
    let _full = serve(&bus, FULL, full_root(), full_player()).await;
    // After the full player by name, so it is read last.
    let hung = "org.mpris.MediaPlayer2.hung";
    let gate = Gate::default();
    let _hung =
        serve(&bus, hung, full_root(), Player { gate: Some(gate.clone()), ..full_player() }).await;
    let mpris = MprisSessions::new(&bus.connect_within(Duration::from_secs(5)).await);

    // Each call runs in real time until the hung player is asked; then only the method timeout
    // is left to fire, and tokio's paused clock reaches it as soon as the runtime is idle.
    let mut sessions = pin!(mpris.sessions());
    tokio::select! {
        () = gate.entered.notified() => {}
        early = &mut sessions => panic!("the listing ended before it asked the hung player: {early:?}"),
    }
    tokio::time::pause();
    assert_eq!(
        sessions.await.unwrap(),
        [full_state()],
        "the player that never answers is left out"
    );
    tokio::time::resume();

    let mut control = pin!(mpris.control(hung, &MediaAction::Play));
    tokio::select! {
        () = gate.entered.notified() => {}
        early = &mut control => panic!("the control ended before it asked the player: {early:?}"),
    }
    tokio::time::pause();
    assert_eq!(
        control.await,
        Err(DbusError::DeadlineExceeded { call: "media player" }),
        "a call past the connection's method timeout passes its deadline"
    );
}

#[tokio::test]
async fn control_sends_each_action_to_the_player() {
    let bus = PrivateBus::start();
    let service = serve(&bus, FULL, full_root(), full_player()).await;
    let mpris = MprisSessions::new(&bus.connect().await);
    let actions = [
        MediaAction::Status,
        MediaAction::Play,
        MediaAction::Pause,
        MediaAction::Toggle,
        MediaAction::Next,
        MediaAction::Previous,
        MediaAction::SeekBy { seconds: -5.0 },
        MediaAction::Seek { seconds: 60.0 },
    ];
    for action in actions {
        mpris.control(FULL, &action).await.unwrap();
    }
    assert_eq!(
        calls(&service).await,
        [
            Call::Play,
            Call::Pause,
            Call::PlayPause,
            Call::Next,
            Call::Previous,
            Call::Seek(-5_000_000),
            Call::SetPosition(OwnedObjectPath::try_from(TRACK).unwrap(), 60_000_000),
        ],
        "one call each, none for status"
    );
}

/// A `Can` property set false on a player that can do everything else, the actions that
/// property refuses and the reason given.
struct Refusal {
    property: &'static str,
    set_false: fn(&mut Player),
    refused: Vec<MediaAction>,
    reason: &'static str,
}

#[tokio::test]
async fn each_can_property_that_is_false_refuses_its_actions() {
    use MediaAction::{Next, Pause, Play, Previous, Seek, SeekBy, Toggle};
    let bus = PrivateBus::start();
    let service = serve(&bus, FULL, full_root(), full_player()).await;
    let player = interface(&service).await;
    let mpris = MprisSessions::new(&bus.connect().await);
    let (seek, seek_by) = (Seek { seconds: 1.0 }, SeekBy { seconds: 1.0 });
    let cases = [
        Refusal {
            property: "CanControl",
            set_false: |player| player.can_control = Some(false),
            refused: vec![Play, Pause, Toggle, Next, Previous, seek, seek_by],
            reason: "the player cannot be controlled",
        },
        Refusal {
            property: "CanPlay",
            set_false: |player| {
                player.can_play = Some(false);
                player.status = "Paused";
            },
            refused: vec![Play, Toggle],
            reason: "the player cannot play",
        },
        Refusal {
            property: "CanPause",
            set_false: |player| player.can_pause = Some(false),
            refused: vec![Pause, Toggle],
            reason: "the player cannot pause",
        },
        Refusal {
            property: "CanGoNext",
            set_false: |player| player.can_go_next = Some(false),
            refused: vec![Next],
            reason: "the player has no next track",
        },
        Refusal {
            property: "CanGoPrevious",
            set_false: |player| player.can_go_previous = Some(false),
            refused: vec![Previous],
            reason: "the player has no previous track",
        },
        Refusal {
            property: "CanSeek",
            set_false: |player| player.can_seek = Some(false),
            refused: vec![seek, seek_by],
            reason: "the player cannot seek",
        },
    ];
    for case in cases {
        let mut fresh = full_player();
        (case.set_false)(&mut fresh);
        *player.get_mut().await = fresh;
        for action in &case.refused {
            let refused = mpris.control(FULL, action).await;
            assert!(
                matches!(refused, Err(DbusError::Refused { reason, .. }) if reason == case.reason),
                "{} false refuses {action:?}: {refused:?}",
                case.property
            );
        }
        assert_eq!(mpris.control(FULL, &MediaAction::Status).await, Ok(()), "status still reads");
        assert_eq!(calls(&service).await, [], "nothing refused reached the player");
    }
}

#[tokio::test]
async fn a_player_that_leaves_out_its_can_properties_is_still_controlled() {
    let bus = PrivateBus::start();
    let service = serve(&bus, ODD, odd_root(), odd_player()).await;
    let mpris = MprisSessions::new(&bus.connect().await);
    for action in [MediaAction::Play, MediaAction::Toggle, MediaAction::Next] {
        mpris.control(ODD, &action).await.unwrap();
    }
    assert_eq!(
        mpris.control(ODD, &MediaAction::Seek { seconds: 50.0 }).await,
        Err(DbusError::Refused {
            what: "seeking",
            reason: "the player gives no track id and no position"
        }),
        "no track id and no position to seek from"
    );
    interface(&service).await.get_mut().await.position = Some(20_000_000);
    mpris.control(ODD, &MediaAction::Seek { seconds: 50.0 }).await.unwrap();
    assert_eq!(
        calls(&service).await,
        [Call::Play, Call::PlayPause, Call::Next, Call::Seek(30_000_000)],
        "each call made, and the seek by the distance from the position"
    );
}

#[tokio::test]
async fn a_seek_takes_a_track_id_sent_as_a_string_and_stays_in_the_track() {
    let bus = PrivateBus::start();
    let service = serve(&bus, FLOAT, float_root(), float_player()).await;
    let mpris = MprisSessions::new(&bus.connect().await);
    mpris.control(FLOAT, &MediaAction::Seek { seconds: 10.0 }).await.unwrap();
    for seconds in [30.6, -1.0, f64::NAN] {
        assert!(
            matches!(
                mpris.control(FLOAT, &MediaAction::Seek { seconds }).await,
                Err(DbusError::Invalid { what: "seek position", .. })
            ),
            "{seconds} is outside the 30.5 s track"
        );
    }
    assert_eq!(
        calls(&service).await,
        [Call::SetPosition(OwnedObjectPath::try_from("/org/eileen/Track/9").unwrap(), 10_000_000)],
        "only the seek inside the track"
    );
}
