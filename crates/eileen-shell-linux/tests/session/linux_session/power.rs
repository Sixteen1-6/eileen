//! The power facts over D-Bus, from `UPower` and power-profiles-daemon on the runner's system
//! bus: one reading, and the stream of changes while the test switches the profile to
//! `power-saver` and back to the one it found, as a user does. polkit lets only the active local
//! session switch it, so the lane's session can and a process outside every session cannot. The
//! runner is a VM on mains power.

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
