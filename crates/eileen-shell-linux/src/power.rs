//! The power source from `/sys/class/power_supply`, and whether a power-saving profile is on.
//!
//! A system battery is a supply of type `Battery` whose scope is not `Device` (a mouse's or a
//! headset's battery has scope `Device`) and that is present. The PC runs on it when no
//! external supply (`Mains`, `USB`, `Wireless`) is online; on a PC that lists no external
//! supply, when the battery says it is discharging.
//!
//! Battery saver has no single Linux switch. Its closest forms are power-profiles-daemon's
//! `power-saver` profile and the ACPI platform profile `low-power` that profile usually sets;
//! either counts. The daemon's saved state (`[State]`, `Profile=`) records what the user or the
//! daemon chose, not a program's hold, so GNOME's low-battery power saver shows only where it
//! reaches the platform profile. Phase L1 reads the active profile over D-Bus instead
//! (`org.freedesktop.UPower.PowerProfiles`, which tuned-ppd serves too).

use crate::SysRoot;
use crate::facts::PowerState;

/// power-profiles-daemon's saved state, readable by every user.
const PPD_STATE: &str = "var/lib/power-profiles-daemon/state.ini";

/// The ACPI platform profile, where the firmware has one.
const PLATFORM_PROFILE: &str = "sys/firmware/acpi/platform_profile";

/// Reads the power state; what cannot be read is named in `degraded`.
pub(crate) fn read_power(root: &SysRoot, wsl: bool, degraded: &mut Vec<String>) -> PowerState {
    let mut has_battery = false;
    let mut discharging = false;
    let mut external = false;
    let mut external_online = false;
    for name in root.entries("sys/class/power_supply") {
        let read = |file: &str| root.read_trimmed(&format!("sys/class/power_supply/{name}/{file}"));
        if read("scope").as_deref() == Some("Device") {
            continue;
        }
        match read("type").as_deref() {
            Some("Battery") if read("present").as_deref() != Some("0") => {
                has_battery = true;
                discharging |= read("status").as_deref() == Some("Discharging");
            }
            Some("Mains" | "USB" | "Wireless") => {
                external = true;
                external_online |= read("online").as_deref() == Some("1");
            }
            _ => {}
        }
    }
    if wsl && !has_battery {
        degraded.push("power: under WSL the host's battery is not visible".to_owned());
    }
    let on_battery = has_battery && if external { !external_online } else { discharging };
    let platform = root.read_trimmed(PLATFORM_PROFILE);
    let daemon = root.read(PPD_STATE).ok().and_then(|text| ppd_profile(&text));
    if has_battery && platform.is_none() && daemon.is_none() {
        degraded.push(
            "battery saver: no platform profile and no power-profiles-daemon state; taken as off"
                .to_owned(),
        );
    }
    let battery_saver =
        platform.as_deref() == Some("low-power") || daemon.as_deref() == Some("power-saver");
    PowerState { has_battery, on_battery, battery_saver }
}

/// The `Profile` key of the `[State]` group in power-profiles-daemon's `state.ini`.
pub(crate) fn ppd_profile(text: &str) -> Option<String> {
    let mut in_state = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_state = line == "[State]";
        } else if in_state
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "Profile"
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::ppd_profile;

    #[test]
    fn the_profile_comes_from_the_state_group() {
        let text = "[Other]\nProfile=performance\n\n[State]\nCpuDriver=amd_pstate\n\
                    PlatformDriver=platform_profile\nProfile=power-saver\n";
        assert_eq!(ppd_profile(text).as_deref(), Some("power-saver"), "the [State] group's");
        assert_eq!(ppd_profile("[State]\nDriver=x\n"), None, "no profile");
        assert_eq!(ppd_profile(""), None, "empty");
    }
}
