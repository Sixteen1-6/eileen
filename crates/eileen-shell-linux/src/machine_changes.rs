//! [`MachineChanges`]: what `MachineProbe::watch` reports on Linux, and from where. BUILD.md
//! section 2.16 reads the facts again when the power source, battery saver or the display count
//! changes; `eileen-platform`'s `MachineChange` names those three.
//!
//! - `PowerSource` and `BatterySaver` come from [`PowerChanges`], the stream of new power states
//!   that `UPower`'s and the profiles daemon's signals on the system bus drive (the `upower`
//!   module). A new state whose `has_battery` or `on_battery` differs from the one before is
//!   `PowerSource`, one whose `battery_saver` differs is `BatterySaver`, and one that changes both
//!   is both, the power source first.
//! - `Displays` is not reported yet. The count comes from the kernel's DRM connectors, whose
//!   changes the kernel announces as `drm` uevents with `HOTPLUG=1` on a netlink socket this crate
//!   has no reader for; GNOME also signals `MonitorsChanged` on `org.gnome.Mutter.DisplayConfig`,
//!   which only GNOME serves.
//! - Memory pressure is no `MachineChange`: the 5 s memory sample serves it
//!   ([`LinuxSystemMemory`](crate::LinuxSystemMemory)), and a trigger on `/proc/pressure/memory`
//!   would add nothing the core reads.
//!
//! # Traps
//!
//! - It follows [`PowerChanges`]' rules and traps: poll it steadily or drop it, inside the tokio
//!   runtime; its first reading may have the bus start a daemon, its later ones never do.
//! - It ends when the system bus's connection closes, and nothing reports a change after that.

use std::pin::Pin;
use std::task::{Context, Poll, ready};

use eileen_contracts::machine::PowerState;
use eileen_dbus_linux::Stream;
use eileen_platform::MachineChange;

use crate::PowerChanges;

/// The machine's changes as `MachineProbe::watch` reports them, from the power states that
/// [`PowerChanges`] yields: a stream the caller drives, which spawns nothing. It ends when the
/// power stream does.
#[derive(Debug)]
pub struct MachineChanges {
    /// The power states.
    power: PowerChanges,
    /// The state the last change came from, or the one the power stream started from.
    last: PowerState,
    /// A `BatterySaver` that came with a `PowerSource`, yielded next.
    saver_next: bool,
}

impl MachineChanges {
    /// The changes of `power`'s states, from the state it last yielded.
    #[must_use]
    pub const fn new(power: PowerChanges) -> Self {
        Self { last: power.state(), power, saver_next: false }
    }

    /// The next change; `None` once the power stream has ended.
    pub async fn next(&mut self) -> Option<MachineChange> {
        std::future::poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }
}

impl Stream for MachineChanges {
    type Item = MachineChange;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<MachineChange>> {
        let this = &mut *self;
        loop {
            if std::mem::take(&mut this.saver_next) {
                return Poll::Ready(Some(MachineChange::BatterySaver));
            }
            let Some(state) = ready!(Pin::new(&mut this.power).poll_next(cx)) else {
                return Poll::Ready(None);
            };
            let before = std::mem::replace(&mut this.last, state);
            let (power, saver) = changed(before, state);
            if power {
                this.saver_next = saver;
                return Poll::Ready(Some(MachineChange::PowerSource));
            }
            if saver {
                return Poll::Ready(Some(MachineChange::BatterySaver));
            }
        }
    }
}

/// Whether the power source and whether battery saver changed from `before` to `after`.
const fn changed(before: PowerState, after: PowerState) -> (bool, bool) {
    let power = before.has_battery != after.has_battery || before.on_battery != after.on_battery;
    (power, before.battery_saver != after.battery_saver)
}

#[cfg(test)]
mod tests {
    use eileen_contracts::machine::PowerState;

    use super::changed;

    const fn state(has_battery: bool, on_battery: bool, battery_saver: bool) -> PowerState {
        PowerState { has_battery, on_battery, battery_saver }
    }

    #[test]
    fn the_battery_and_the_source_are_the_power_source_and_the_profile_battery_saver() {
        let mains = state(true, false, false);
        assert_eq!(changed(mains, state(true, true, false)), (true, false), "unplugged");
        assert_eq!(changed(mains, state(false, false, false)), (true, false), "battery removed");
        assert_eq!(changed(mains, state(true, false, true)), (false, true), "saver on");
        assert_eq!(changed(mains, state(true, true, true)), (true, true), "both at once");
        assert_eq!(changed(mains, mains), (false, false), "nothing");
        let ups = state(false, true, false);
        assert_eq!(changed(state(false, false, false), ups), (true, false), "a UPS discharging");
    }
}
