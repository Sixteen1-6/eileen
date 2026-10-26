//! `ShellHost`'s shortcut methods on Linux, `register_hotkey` and `watch_input`'s dictation key:
//! `LinuxShell` through a bridge on the private bus, against the `GlobalShortcuts` portal fake of
//! the client's own tests, with `shell_host`'s fakes, settings and shells. The test's thread calls
//! the shell as the core does, blocking, and a sink of the test's own takes the shell's events.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use eileen_contracts::core_io::KeyEdge;
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::testing::PrivateBus;
use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE, Names, owner_changes};
use eileen_platform::{PlatformError, ShellHost, ShellInputEvent, VirtualKey};
use eileen_shell_linux::{BoundHotkey, DbusError, LinuxShell, LinuxShellEvent, ShortcutId};
use tokio::sync::mpsc;

use crate::keep_awake::left;
use crate::shell_host::{DESKTOP_ENTRY, Fakes, settings, shell_on};
use crate::shortcuts::{
    Bind, FakeRegistry, FakeShortcuts, Mode, Modes, PORTAL as SHORTCUTS_PORTAL,
    PORTAL_PATH as SHORTCUTS_PATH, Report, end_session, listed, press, respond as answer, triggers,
};

/// A sink for the shell's events, and what it receives.
fn events_sink() -> (Arc<dyn Fn(LinuxShellEvent) + Send + Sync>, mpsc::Receiver<LinuxShellEvent>) {
    let (events, received) = mpsc::channel(64);
    let sink = move |event| {
        let _ignored = events.try_send(event);
    };
    (Arc::new(sink), received)
}

/// The next bind the fake reports that lists `count` shortcuts, passing over its other reports.
fn bind_of(reports: &mut mpsc::Receiver<Report>, count: usize) -> Bind {
    loop {
        if let Report::Bound(bind) = reports.blocking_recv().unwrap()
            && bind.shortcuts.len() == count
        {
            return bind;
        }
    }
}

/// The IDs a bind lists.
fn ids(bind: &Bind) -> Vec<&str> {
    bind.shortcuts.iter().map(|(id, _, _)| id.as_str()).collect()
}

#[test]
fn hotkeys_and_the_dictation_key_are_bound_together_and_report_presses() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes::GRANT);
    let portal = fakes.shortcuts(&bus, fake);
    let mut settings = settings(DEFAULT_DEADLINE, None);
    settings.shortcut_names.hotkeys = BTreeMap::from([(1, "Mute or unmute".to_owned())]);
    let (sink, mut events) = events_sink();
    let shell = LinuxShell::new(Bridge::on_private_bus(&bus).unwrap(), settings).with_events(sink);
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    shell.register_hotkey("Ctrl+Alt+Shift+B", 2).unwrap();
    let (edges, mut input) = mpsc::channel(8);
    let edge = move |event| {
        let _ignored = edges.try_send(event);
    };
    let _watch = shell.watch_input(VirtualKey(0x14), Box::new(edge)).unwrap();
    let bind = bind_of(&mut reports, 3);
    let expected = listed(&[
        ("dictation", Some("Hold to dictate"), Some("Caps_Lock")),
        ("hotkey-1", Some("Mute or unmute"), Some("CTRL+ALT+m")),
        ("hotkey-2", Some("Eileen shortcut 2"), Some("CTRL+ALT+SHIFT+b")),
    ]);
    assert_eq!(bind.shortcuts, expected, "one bind of the whole set, described and triggered");
    let bound = loop {
        if let LinuxShellEvent::ShortcutsBound(bound) = events.blocking_recv().unwrap()
            && bound.len() == 3
        {
            break bound;
        }
    };
    let hotkey = BoundHotkey {
        id: ShortcutId::Hotkey(1),
        trigger: Untrusted::new("Press CTRL+ALT+m".to_owned()),
    };
    assert_eq!(bound.get(1), Some(&hotkey), "the desktop's answer reaches the sink");
    let pressed = |member: &str, id: &str| {
        fakes.runtime.block_on(press(&portal, &bind.caller, member, &bind.session, id));
    };
    pressed("Activated", "hotkey-1");
    assert_eq!(events.blocking_recv(), Some(LinuxShellEvent::Hotkey(1)), "the hotkey's press");
    pressed("Activated", "dictation");
    pressed("Deactivated", "dictation");
    let down = Some(ShellInputEvent::DictationKey(KeyEdge::Down));
    assert_eq!(input.blocking_recv(), down, "the dictation key's press");
    let up = Some(ShellInputEvent::DictationKey(KeyEdge::Up));
    assert_eq!(input.blocking_recv(), up, "and its release");
}

/// The next `count` hotkey presses `events` reports, passing over the desktop's answers.
fn presses(events: &mut mpsc::Receiver<LinuxShellEvent>, count: usize) -> Vec<LinuxShellEvent> {
    let mut presses = Vec::new();
    while presses.len() < count {
        let event = events.blocking_recv().unwrap();
        if let LinuxShellEvent::Hotkey(_) = event {
            presses.push(event);
        }
    }
    presses
}

#[test]
fn the_holder_first_registers_the_desktop_entry_as_the_apps_id() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes::GRANT);
    let _portal = fakes.shortcuts(&bus, fake);
    let mut settings = settings(DEFAULT_DEADLINE, None);
    settings.notifier.desktop_entry = format!("{DESKTOP_ENTRY}.desktop");
    let shell = LinuxShell::new(Bridge::on_private_bus(&bus).unwrap(), settings);
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    let registered =
        Report::Registered { app_id: DESKTOP_ENTRY.to_owned(), options: vec![], first: true };
    assert_eq!(
        reports.blocking_recv(),
        Some(registered),
        "the desktop file's ID, without .desktop, in the first portal call of the connection"
    );
}

#[test]
fn a_held_key_is_one_press_and_one_release_though_the_desktop_repeats_it() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes::GRANT);
    let portal = fakes.shortcuts(&bus, fake);
    let (sink, mut events) = events_sink();
    let shell = shell_on(&bus, DEFAULT_DEADLINE).with_events(sink);
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    shell.register_hotkey("ctrl+alt+shift+b", 2).unwrap();
    let (edges, mut input) = mpsc::channel(8);
    let edge = move |event| {
        let _ignored = edges.try_send(event);
    };
    let _watch = shell.watch_input(VirtualKey(0x14), Box::new(edge)).unwrap();
    let bind = bind_of(&mut reports, 3);
    let send = |member: &str, id: &str| {
        fakes.runtime.block_on(press(&portal, &bind.caller, member, &bind.session, id));
    };
    // GNOME sends `Activated` again for each repeat of a held key, then `Deactivated` once.
    for member in ["Activated", "Activated", "Activated", "Deactivated", "Activated", "Deactivated"]
    {
        send(member, "dictation");
    }
    let down = ShellInputEvent::DictationKey(KeyEdge::Down);
    let up = ShellInputEvent::DictationKey(KeyEdge::Up);
    let edges: Vec<_> = (0..4).map(|_| input.blocking_recv().unwrap()).collect();
    assert_eq!(edges, [down, up, down, up], "a press and a release for each of two holds");
    let held = [("Activated", "hotkey-1"), ("Activated", "hotkey-1"), ("Activated", "hotkey-1")];
    let then =
        [("Activated", "hotkey-2"), ("Deactivated", "hotkey-2"), ("Deactivated", "hotkey-1")];
    for (member, id) in held.into_iter().chain(then).chain([("Activated", "hotkey-1")]) {
        send(member, id);
    }
    assert_eq!(
        presses(&mut events, 3),
        [LinuxShellEvent::Hotkey(1), LinuxShellEvent::Hotkey(2), LinuxShellEvent::Hotkey(1)],
        "one press for a hotkey held down, none for a release, and one for the next press"
    );
}

#[test]
fn a_dictation_key_held_as_its_session_ends_is_released() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes::GRANT);
    let portal = fakes.shortcuts(&bus, fake);
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    let (edges, mut input) = mpsc::channel(8);
    let edge = move |event| {
        let _ignored = edges.try_send(event);
    };
    let _watch = shell.watch_input(VirtualKey(0x14), Box::new(edge)).unwrap();
    let first = bind_of(&mut reports, 2);
    let send = |bind: &Bind, member: &str| {
        fakes.runtime.block_on(press(&portal, &bind.caller, member, &bind.session, "dictation"));
    };
    send(&first, "Activated");
    let down = Some(ShellInputEvent::DictationKey(KeyEdge::Down));
    assert_eq!(input.blocking_recv(), down, "the key went down");
    // A changed set: the first session closes, and its Deactivated can no longer come.
    shell.register_hotkey("ctrl+alt+shift+b", 2).unwrap();
    let up = Some(ShellInputEvent::DictationKey(KeyEdge::Up));
    assert_eq!(input.blocking_recv(), up, "released as its session ended");
    let second = bind_of(&mut reports, 3);
    send(&second, "Activated");
    assert_eq!(input.blocking_recv(), down, "and down again, a new press in the new session");
}

#[test]
fn a_hotkey_added_while_a_bind_waits_rebinds_everything_in_a_new_session() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes { bind: Mode::Later, ..Modes::GRANT });
    let portal = fakes.shortcuts(&bus, fake);
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    let first = bind_of(&mut reports, 1);
    // Answered at once, since the holder has a session, while the first bind's dialog waits.
    shell.register_hotkey("ctrl+alt+shift+b", 2).unwrap();
    let granted = HashMap::from([("shortcuts", triggers(&first.shortcuts))]);
    fakes.runtime.block_on(answer(&portal, &first.caller, &first.request, 0, granted));
    let mut closed = false;
    let second = loop {
        match reports.blocking_recv().unwrap() {
            Report::SessionClosed(session) => closed |= session == first.session,
            Report::Bound(bind) => break bind,
            Report::Registered { .. } | Report::Created(_) | Report::RequestClosed(_) => {}
        }
    };
    assert!(closed, "the first session is closed before the next bind");
    assert_ne!(second.session, first.session, "which is in a new session");
    assert_eq!(ids(&second), ["hotkey-1", "hotkey-2"], "and binds the whole set");
}

#[test]
fn a_refused_rebind_binds_what_the_desktop_had_again_and_the_next_call_asks_again() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes { bind: Mode::Later, ..Modes::GRANT });
    let portal = fakes.shortcuts(&bus, fake);
    let (sink, mut events) = events_sink();
    let shell = shell_on(&bus, DEFAULT_DEADLINE).with_events(sink);
    let reply = |bind: &Bind, code: u32| {
        let granted = HashMap::from([("shortcuts", triggers(&bind.shortcuts))]);
        let results = if code == 0 { granted } else { HashMap::new() };
        fakes.runtime.block_on(answer(&portal, &bind.caller, &bind.request, code, results));
    };
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    let first = bind_of(&mut reports, 1);
    reply(&first, 0);
    let bound = |events: &mut mpsc::Receiver<LinuxShellEvent>| match events.blocking_recv() {
        Some(LinuxShellEvent::ShortcutsBound(bound)) => bound.into_iter().map(|hotkey| hotkey.id),
        other => panic!("not a bind's answer: {other:?}"),
    };
    assert_eq!(bound(&mut events).collect::<Vec<_>>(), [ShortcutId::Hotkey(1)], "mute is bound");
    shell.register_hotkey("ctrl+alt+shift+b", 2).unwrap();
    let changed = bind_of(&mut reports, 2);
    // 2, as GNOME answers a dialog the user dismissed: the first session is closed by now.
    reply(&changed, 2);
    let refused =
        DbusError::Refused { what: "GlobalShortcuts binding", reason: "refused or cancelled" };
    let refusal = Some(LinuxShellEvent::ShortcutsRefused(refused));
    assert_eq!(events.blocking_recv(), refusal, "the changed set is refused");
    let again = bind_of(&mut reports, 1);
    assert_eq!(ids(&again), ["hotkey-1"], "what the desktop had is bound again, the new one not");
    assert_ne!(again.session, changed.session, "in a session of its own");
    reply(&again, 0);
    assert_eq!(bound(&mut events).collect::<Vec<_>>(), [ShortcutId::Hotkey(1)], "and bound");
    fakes.runtime.block_on(press(&portal, &again.caller, "Activated", &again.session, "hotkey-1"));
    assert_eq!(events.blocking_recv(), Some(LinuxShellEvent::Hotkey(1)), "so mute works again");
    shell.register_hotkey("ctrl+alt+shift+b", 2).unwrap();
    let asked = bind_of(&mut reports, 2);
    assert_ne!(asked.session, again.session, "the next call asks for the new one again");
}

#[test]
fn a_session_the_portal_closes_ends_the_holder_and_the_next_call_starts_another() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes::GRANT);
    let portal = fakes.shortcuts(&bus, fake);
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    let (edges, mut input) = mpsc::channel(8);
    let edge = move |event| {
        let _ignored = edges.try_send(event);
    };
    let _watch = shell.watch_input(VirtualKey(0x14), Box::new(edge)).unwrap();
    let bind = bind_of(&mut reports, 2);
    fakes.runtime.block_on(press(&portal, &bind.caller, "Activated", &bind.session, "dictation"));
    let down = Some(ShellInputEvent::DictationKey(KeyEdge::Down));
    assert_eq!(input.blocking_recv(), down, "the dictation key is held");
    let left_the_bus = fakes.runtime.block_on(async {
        let observer = bus.connect().await;
        let mut leaving = owner_changes(&observer, Names::Name(&bind.caller)).await.unwrap();
        end_session(&portal, &bind.caller, &bind.session).await;
        // The barrier: a call made before the holder has ended would race it.
        left(&mut leaving).await
    });
    assert!(left_the_bus, "the holder ended, and its connection left the bus");
    let up = Some(ShellInputEvent::DictationKey(KeyEdge::Up));
    assert_eq!(input.blocking_recv(), up, "the held key is released as the session ends");
    assert_eq!(input.blocking_recv(), None, "and the watch ends with the holder");
    shell.register_hotkey("ctrl+alt+shift+b", 2).unwrap();
    let next = bind_of(&mut reports, 1);
    assert_ne!(next.caller, bind.caller, "the next call starts a holder on a new connection");
    assert_eq!(ids(&next), ["hotkey-2"], "which binds that call's shortcut alone");
}

#[test]
fn the_holder_closes_its_session_once_the_shell_drops() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes::GRANT);
    let _portal = fakes.shortcuts(&bus, fake);
    // A handle of the test's own keeps the bridge, and so the holder, past the shell's drop.
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let shell = LinuxShell::new(bridge.clone(), settings(DEFAULT_DEADLINE, None));
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    let bind = bind_of(&mut reports, 1);
    drop(shell);
    let closed = loop {
        if let Report::SessionClosed(session) = reports.blocking_recv().unwrap() {
            break session;
        }
    };
    assert_eq!(closed, bind.session, "the holder ends with the shell, and closes its session");
    drop(bridge);
}

#[test]
fn a_rebind_whose_session_cannot_open_reaches_the_sink_as_a_refusal() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes::GRANT);
    let portal = fakes.shortcuts(&bus, fake);
    let (sink, mut events) = events_sink();
    let shell = shell_on(&bus, DEFAULT_DEADLINE).with_events(sink);
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    let _first = bind_of(&mut reports, 1);
    let answered = events.blocking_recv();
    assert!(matches!(answered, Some(LinuxShellEvent::ShortcutsBound(_))), "{answered:?}");
    let released = fakes.runtime.block_on(portal.release_name(SHORTCUTS_PORTAL)).unwrap();
    assert!(released, "the portal has left the bus, so no new session can open");
    shell.register_hotkey("ctrl+alt+shift+b", 2).unwrap();
    let unavailable = DbusError::Unavailable { what: "GlobalShortcuts session" };
    assert_eq!(
        events.blocking_recv(),
        Some(LinuxShellEvent::ShortcutsRefused(unavailable)),
        "the changed set is bound nowhere, and the sink hears it"
    );
}

#[test]
fn without_the_portal_hotkeys_and_input_are_unavailable() {
    let bus = PrivateBus::start();
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    let hotkey = PlatformError::Unavailable { what: "global hotkey" };
    assert_eq!(shell.register_hotkey("ctrl+alt+m", 1), Err(hotkey.clone()), "no portal");
    let watch = shell.watch_input(VirtualKey(0x14), Box::new(|_event| {}));
    assert_eq!(
        watch.unwrap_err(),
        PlatformError::Unavailable { what: "input watch" },
        "nor for the dictation key"
    );
    let mut fakes = Fakes::new();
    let (fake, _reports) = FakeShortcuts::new(Modes::GRANT);
    fakes.serve(&bus, SHORTCUTS_PORTAL, SHORTCUTS_PATH, FakeRegistry(fake));
    assert_eq!(
        shell.register_hotkey("ctrl+alt+m", 1),
        Err(hotkey),
        "a portal without GlobalShortcuts, which a new holder asks; 24.04's lacks the registry too"
    );
}

#[test]
fn a_chord_the_portal_cannot_name_is_invalid() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (fake, mut reports) = FakeShortcuts::new(Modes::GRANT);
    let _portal = fakes.shortcuts(&bus, fake);
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    let detail = "a key no trigger can name".to_owned();
    assert_eq!(
        shell.register_hotkey("ctrl+hyper+m", 1),
        Err(PlatformError::Invalid { what: "hotkey chord", detail }),
        "no keysym is called hyper"
    );
    shell.register_hotkey("ctrl+alt+m", 2).unwrap();
    assert_eq!(
        ids(&bind_of(&mut reports, 1)),
        ["hotkey-2"],
        "the first bind holds the valid chord alone: the other never reached the holder"
    );
}

#[test]
fn a_cancelled_dialog_reaches_the_sink_as_a_refusal() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    // 2, as GNOME answers a dialog the user dismissed.
    let (fake, mut reports) = FakeShortcuts::new(Modes { bind: Mode::Code(2), ..Modes::GRANT });
    let _portal = fakes.shortcuts(&bus, fake);
    let (sink, mut events) = events_sink();
    let shell = shell_on(&bus, DEFAULT_DEADLINE).with_events(sink);
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    let refused =
        DbusError::Refused { what: "GlobalShortcuts binding", reason: "refused or cancelled" };
    let refusal = Some(LinuxShellEvent::ShortcutsRefused(refused));
    assert_eq!(events.blocking_recv(), refusal, "the user closed the dialog");
    let first = bind_of(&mut reports, 1);
    shell.register_hotkey("ctrl+alt+m", 1).unwrap();
    let again = bind_of(&mut reports, 1);
    assert_ne!(again.session, first.session, "the same call again shows the dialog again");
    assert_eq!(events.blocking_recv(), refusal, "which the user closed again");
}
