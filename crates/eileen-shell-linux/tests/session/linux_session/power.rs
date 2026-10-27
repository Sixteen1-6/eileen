//! The power facts over D-Bus, from `UPower` and power-profiles-daemon on the runner's system
//! bus: one reading, and the stream of changes while the test switches the profile to
//! `power-saver` and back to the one it found, as a user does. polkit lets a caller switch it
//! when the session it judges the caller by is active and local: the caller's logind session or,
//! for a process in none, the one logind names as its user's display, a graphical one first
//! (polkit's `polkit_backend_session_monitor_get_session_for_subject`). The tests run in the
//! lane's session, which is; the runner's own steps run in system services, outside the user's
//! slice, and so have neither. The runner is a VM on mains power.

use eileen_shell_linux::{LinuxPower, PowerChanges, PowerDaemon};

use super::{ProfileBack, Session, active_profile, set_profile};

/// The profile that is battery saver.
const POWER_SAVER: &str = "power-saver";

#[test]
fn the_daemons_answer_with_nothing_degraded() {
    let session = Session::new();
    let system = session.system();
    let reading = session.block_on(async { LinuxPower::new(system).read().await });
    assert_eq!(
        reading.answered,
        [PowerDaemon::UPower, PowerDaemon::PowerProfiles],
        "UPower, and the profiles daemon under its current name"
    );
    assert!(reading.degraded.is_empty(), "every fact was read: {:?}", reading.degraded);
    assert!(!reading.state.on_battery, "the runner's VM is on mains power: {reading:?}");
}

#[test]
fn a_profile_switch_reaches_the_changes_stream() {
    let session = Session::new();
    let back = ProfileBack::new(&session);
    let system = session.system();
    session.block_on(async {
        let mut changes = LinuxPower::new(system).changes().await.unwrap();
        assert!(!changes.state().battery_saver, "battery saver starts off: {changes:?}");
        let found = active_profile(system).await.unwrap();
        set_profile(system, POWER_SAVER).await.unwrap();
        back.arm(&found);
        saver_turns(&mut changes, true).await;
        set_profile(system, &found).await.unwrap();
        back.disarm();
        saver_turns(&mut changes, false).await;
    });
}

/// Waits for the stream's next state whose battery saver is `on`, passing over a state that
/// changed in the other facts alone.
async fn saver_turns(changes: &mut PowerChanges, on: bool) {
    loop {
        let state = changes.next().await.unwrap();
        if state.battery_saver == on {
            return;
        }
    }
}
