//! [`PowerChanges`], the stream of new power states that
//! [`LinuxPower::changes`](super::LinuxPower::changes) starts. It spawns nothing: the caller
//! drives it, and each poll takes the signals waiting and reads again the daemons they concern.
//!
//! # Rules
//!
//! - It subscribes before it reads, so no change falls between the two, and yields only a state
//!   that differs from the one before. A change to `UPower`'s `OnBattery`, to a system battery's
//!   `Type`, `PowerSupply`, `IsPresent` or `State`, a device added or removed, a change to the
//!   profiles object's `ActiveProfile` or `ActiveProfileHolds`, and a daemon starting or being
//!   replaced each make it read that daemon again. A daemon stopping does not.
//! - After the first reading it calls the unique name that owns the daemon's name, and the bus
//!   starts a daemon only for a well-known name
//!   ([activation](https://dbus.freedesktop.org/doc/dbus-specification.html#message-bus-starting-services)).
//!   So a daemon that was stopped, by the user, a package upgrade or a crash, stays stopped:
//!   power-profiles-daemon's unit conflicts with TLP's, and starting it again would stop TLP.
//! - Its values come from those readings, never from the signals, so a signal from another
//!   sender can cost a reading but never set a fact. When a daemon stops, its facts keep their
//!   last values until it answers again.
//!
//! # Traps
//!
//! - It follows `eileen-dbus-linux`'s stream conventions (`QUEUE_SIZE`, `DRAIN_CAP`) and has the
//!   traps written there: poll it steadily or drop it, and drop it inside the tokio runtime.
//! - power-profiles-daemon 0.20 and later serves both names from one process and signals each
//!   change on both objects; signals already waiting are read as one.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use eileen_dbus_linux::{DRAIN_CAP, Names, OwnerChange, OwnerChanges, QUEUE_SIZE, Stream};
use zbus::message::Type as MessageType;
use zbus::zvariant::{OwnedObjectPath, Value};
use zbus::{Connection, MatchRule, Message, MessageStream};

use super::profiles::{ProfilesAnswer, read_profiles};
use super::{DEVICE, LEGACY_PROFILES, PROFILES, PROPERTIES, UPOWER, UPOWER_PATH};
use super::{PowerDaemon, Reach, UPowerAnswer, read_upower, state_of};
use crate::DbusError;
use crate::facts::PowerState;

/// The device properties whose change can change `has_battery`, and `State`, which changes with
/// the power source.
const BATTERY_PROPERTIES: [&str; 4] = ["Type", "PowerSupply", "IsPresent", "State"];

/// The profiles object's properties the stream follows.
const PROFILE_PROPERTIES: [&str; 2] = ["ActiveProfile", "ActiveProfileHolds"];

/// What the subscriptions' errors name, as a bare noun.
const WATCH: &str = "power-change subscription";

/// Subscribes to every change of the power facts on `bus`, then reads them.
pub(super) async fn follow(bus: &Connection) -> Result<PowerChanges, DbusError> {
    let daemons =
        [PowerDaemon::UPower, PowerDaemon::PowerProfiles, PowerDaemon::LegacyPowerProfiles];
    let mut owners = Vec::new();
    for daemon in daemons {
        owners.push(eileen_dbus_linux::owner_changes(bus, Names::Name(daemon.name())).await?);
    }
    let mut objects = vec![(Source::UPowerRoot, subscribe(bus, root_rule()).await?)];
    for daemon in [PowerDaemon::PowerProfiles, PowerDaemon::LegacyPowerProfiles] {
        let rule = properties_rule(daemon.name(), daemon.path(), daemon.name());
        objects.push((Source::Profiles(daemon), subscribe(bus, rule).await?));
    }
    let mut watch = Watch {
        bus: bus.clone(),
        upower: None,
        profiles: None,
        last: PowerState::default(),
        owners,
        objects,
        batteries: Vec::new(),
    };
    watch.refresh(Outdated { upower: true, profiles: true }, Reach::Start).await;
    watch.last = watch.state();
    Ok(PowerChanges { state: watch.last, next: Some(Box::pin(watch.advance())) })
}

/// The power facts' changes: a stream of each [`PowerState`] that differs from the one before,
/// which the caller drives (with [`PowerChanges::next`] or as a `Stream`); it spawns nothing.
/// It ends when the connection closes.
pub struct PowerChanges {
    /// The state last yielded, or the one the stream started from.
    state: PowerState,
    /// The wait for the next state; `None` once the stream has ended.
    next: Option<Advance>,
}

/// The future that waits for the next state, owning the watch until it yields it.
type Advance = Pin<Box<dyn Future<Output = Option<(PowerState, Watch)>> + Send>>;

impl PowerChanges {
    /// The state the stream last yielded, or the reading it started from, taken after it
    /// subscribed.
    #[must_use]
    pub const fn state(&self) -> PowerState {
        self.state
    }

    /// The next state that differs from the one before; `None` once the connection has closed.
    pub async fn next(&mut self) -> Option<PowerState> {
        std::future::poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }
}

impl Stream for PowerChanges {
    type Item = PowerState;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<PowerState>> {
        let Some(advance) = self.next.as_mut() else {
            return Poll::Ready(None);
        };
        match advance.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some((state, watch))) => {
                self.state = state;
                self.next = Some(Box::pin(watch.advance()));
                Poll::Ready(Some(state))
            }
            Poll::Ready(None) => {
                self.next = None;
                Poll::Ready(None)
            }
        }
    }
}

impl fmt::Debug for PowerChanges {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PowerChanges")
            .field("state", &self.state)
            .field("ended", &self.next.is_none())
            .finish()
    }
}

/// Which daemon's facts the signals waiting make outdated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Outdated {
    /// `UPower`'s.
    upower: bool,
    /// The profiles daemon's.
    profiles: bool,
}

impl Outdated {
    /// Either is outdated.
    const fn any(self) -> bool {
        self.upower || self.profiles
    }

    /// Adds what `other` makes outdated.
    const fn merge(&mut self, other: Self) {
        self.upower |= other.upower;
        self.profiles |= other.profiles;
    }
}

/// What an object subscription's signals are about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// `UPower`'s root object: `PropertiesChanged`, `DeviceAdded` and `DeviceRemoved`.
    UPowerRoot,
    /// One system battery's `PropertiesChanged`.
    Battery,
    /// The profiles object under one name: its `PropertiesChanged`.
    Profiles(PowerDaemon),
}

/// What one subscription had waiting, the strongest last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Drained {
    /// Nothing more for now.
    Waiting,
    /// More than one poll takes: poll again.
    More,
    /// The subscription has ended: the connection closed.
    Ended,
}

/// The subscriptions behind [`PowerChanges`], and the facts last read.
struct Watch {
    /// The system bus.
    bus: Connection,
    /// `UPower`'s last answer; `None` until it has answered.
    upower: Option<UPowerAnswer>,
    /// The profiles daemon's last answer; `None` until one has answered.
    profiles: Option<ProfilesAnswer>,
    /// The state last yielded, or the first reading's.
    last: PowerState,
    /// Each daemon's name's owner changes.
    owners: Vec<OwnerChanges>,
    /// `UPower`'s root object and both profiles objects.
    objects: Vec<(Source, MessageStream)>,
    /// One subscription per system battery.
    batteries: Vec<(OwnedObjectPath, MessageStream)>,
}

impl Watch {
    /// The state the last answers give.
    fn state(&self) -> PowerState {
        state_of(self.upower.as_ref(), self.profiles.as_ref())
    }

    /// Waits for the next state that differs from the last; `None` once the connection closed.
    async fn advance(mut self) -> Option<(PowerState, Self)> {
        loop {
            let outdated = std::future::poll_fn(|cx| self.poll_outdated(cx)).await?;
            self.refresh(outdated, Reach::Running).await;
            let state = self.state();
            if state != self.last {
                self.last = state;
                return Some((state, self));
            }
        }
    }

    /// Reads the outdated daemons again, reaching them as `reach` says. A daemon that does not
    /// answer keeps its last answer, so one that stops never turns its facts false.
    async fn refresh(&mut self, outdated: Outdated, reach: Reach) {
        if outdated.upower {
            // A battery seen for the first time is read again once its subscription is in
            // place, so a change between the two readings is not lost.
            for _ in 0..2 {
                let Ok(answer) = read_upower(&self.bus, reach).await else {
                    break;
                };
                let added = self.watch_batteries(&answer).await;
                self.upower = Some(answer);
                if !added {
                    break;
                }
            }
        }
        if outdated.profiles
            && let Ok(answer) = read_profiles(&self.bus, reach).await
        {
            self.profiles = Some(answer);
        }
    }

    /// Subscribes to each system battery in `answer` not yet followed and drops the
    /// subscriptions of devices it no longer lists; true when one was added.
    async fn watch_batteries(&mut self, answer: &UPowerAnswer) -> bool {
        self.batteries
            .retain(|(path, _)| answer.batteries.iter().any(|(battery, _)| battery == path));
        let mut added = false;
        for (battery, _) in &answer.batteries {
            if self.batteries.iter().any(|(path, _)| path == battery) {
                continue;
            }
            // A battery the bus will not follow is read again with UPower's other changes.
            if let Ok(stream) =
                subscribe(&self.bus, properties_rule(UPOWER, battery.as_str(), DEVICE)).await
            {
                self.batteries.push((battery.clone(), stream));
                added = true;
            }
        }
        added
    }

    /// Takes every signal waiting: what they make outdated, or `None` once the connection closed.
    fn poll_outdated(&mut self, cx: &mut Context<'_>) -> Poll<Option<Outdated>> {
        let mut outdated = Outdated::default();
        let mut drained = Drained::Waiting;
        for owners in &mut self.owners {
            drained = drained.max(drain(owners, cx, |change| outdated.merge(started(&change))));
        }
        let objects = self.objects.iter_mut().map(|(source, stream)| (*source, stream));
        let batteries = self.batteries.iter_mut().map(|(_, stream)| (Source::Battery, stream));
        for (source, stream) in objects.chain(batteries) {
            // A read error is passed over; the subscription's end follows it.
            let note = |item: zbus::Result<Message>| {
                if let Ok(message) = item {
                    outdated.merge(outdated_by(source, &message));
                }
            };
            drained = drained.max(drain(stream, cx, note));
        }
        if drained == Drained::Ended {
            Poll::Ready(None)
        } else if outdated.any() {
            Poll::Ready(Some(outdated))
        } else {
            if drained == Drained::More {
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        }
    }
}

/// Takes up to [`DRAIN_CAP`] items from `stream`, handing each to `note`.
fn drain<S: Stream + Unpin>(
    stream: &mut S,
    cx: &mut Context<'_>,
    mut note: impl FnMut(S::Item),
) -> Drained {
    for _ in 0..DRAIN_CAP {
        match Pin::new(&mut *stream).poll_next(cx) {
            Poll::Ready(Some(item)) => note(item),
            Poll::Ready(None) => return Drained::Ended,
            Poll::Pending => return Drained::Waiting,
        }
    }
    Drained::More
}

/// What an owner change makes outdated. A daemon that starts or is replaced has facts to read;
/// one that stops has none, and keeps its last ones.
fn started(change: &OwnerChange) -> Outdated {
    let name = change.name.as_str();
    let started = change.new.is_some();
    Outdated {
        upower: started && name == UPOWER,
        profiles: started && (name == PROFILES || name == LEGACY_PROFILES),
    }
}

/// What one signal from an object subscription for `source` makes outdated.
fn outdated_by(source: Source, message: &Message) -> Outdated {
    match source {
        Source::UPowerRoot => {
            let header = message.header();
            let device_listing = header.interface().is_some_and(|interface| interface == UPOWER)
                && header
                    .member()
                    .is_some_and(|member| member == "DeviceAdded" || member == "DeviceRemoved");
            Outdated {
                upower: device_listing || changes_any(message, UPOWER, &["OnBattery"]),
                profiles: false,
            }
        }
        Source::Battery => {
            Outdated { upower: changes_any(message, DEVICE, &BATTERY_PROPERTIES), profiles: false }
        }
        Source::Profiles(daemon) => Outdated {
            upower: false,
            profiles: changes_any(message, daemon.name(), &PROFILE_PROPERTIES),
        },
    }
}

/// Whether `message` is a `PropertiesChanged` of `interface` that changes or invalidates one of
/// `names`. One whose body does not parse counts as a change: reading again costs little and
/// the reading decides.
fn changes_any(message: &Message, interface: &str, names: &[&str]) -> bool {
    if message.header().member().is_none_or(|member| member != "PropertiesChanged") {
        return false;
    }
    let body = message.body();
    match body.deserialize::<(&str, HashMap<&str, Value<'_>>, Vec<&str>)>() {
        Ok((changed_interface, changed, invalidated)) => {
            changed_interface == interface
                && names.iter().any(|name| changed.contains_key(name) || invalidated.contains(name))
        }
        Err(_) => true,
    }
}

/// A subscription to the signals `rule` matches, queueing `QUEUE_SIZE` of them.
async fn subscribe(
    bus: &Connection,
    rule: zbus::Result<MatchRule<'_>>,
) -> Result<MessageStream, DbusError> {
    let failed = |error: zbus::Error| DbusError::from_zbus(WATCH, &error);
    MessageStream::for_match_rule(rule.map_err(failed)?, bus, Some(QUEUE_SIZE))
        .await
        .map_err(failed)
}

/// Every signal of `UPower`'s root object: `PropertiesChanged`, `DeviceAdded`, `DeviceRemoved`.
fn root_rule() -> zbus::Result<MatchRule<'static>> {
    Ok(MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(UPOWER)?
        .path(UPOWER_PATH)?
        .build())
}

/// `PropertiesChanged` of `interface` at `path`, from the owner of `sender`.
fn properties_rule<'p>(
    sender: &'static str,
    path: &'p str,
    interface: &'static str,
) -> zbus::Result<MatchRule<'p>> {
    Ok(MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(sender)?
        .path(path)?
        .interface(PROPERTIES)?
        .member("PropertiesChanged")?
        .arg(0, interface)?
        .build())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use eileen_dbus_linux::OwnerChange;
    use zbus::Message;
    use zbus::zvariant::{ObjectPath, Value};

    use super::{DEVICE, Outdated, PowerDaemon, Source, outdated_by, started};

    /// A `PropertiesChanged` signal at `path`.
    fn changed(path: &str, interface: &str, changed: &[&str], invalidated: &[&str]) -> Message {
        let values: HashMap<&str, Value<'_>> =
            changed.iter().map(|name| (*name, Value::from(true))).collect();
        Message::signal(path, "org.freedesktop.DBus.Properties", "PropertiesChanged")
            .unwrap()
            .build(&(interface, values, invalidated.to_vec()))
            .unwrap()
    }

    const UPOWER: Outdated = Outdated { upower: true, profiles: false };
    const PROFILES: Outdated = Outdated { upower: false, profiles: true };
    const NEITHER: Outdated = Outdated { upower: false, profiles: false };

    #[test]
    fn a_upower_signal_outdates_its_facts_only_for_what_they_read() {
        let (root, bat0) =
            ("/org/freedesktop/UPower", "/org/freedesktop/UPower/devices/battery_BAT0");
        let on_root =
            |names: &[&str], gone: &[&str]| changed(root, "org.freedesktop.UPower", names, gone);
        let on_bat0 = |interface: &str, names: &[&str]| changed(bat0, interface, names, &[]);
        let added = Message::signal(root, "org.freedesktop.UPower", "DeviceAdded")
            .unwrap()
            .build(&(ObjectPath::try_from(bat0).unwrap(),))
            .unwrap();
        let unparsed =
            Message::signal(bat0, "org.freedesktop.DBus.Properties", "PropertiesChanged")
                .unwrap()
                .build(&("not the properties' shape",))
                .unwrap();
        let cases = [
            (Source::UPowerRoot, on_root(&["OnBattery"], &[]), UPOWER, "the source"),
            (Source::UPowerRoot, on_root(&["LidIsClosed"], &[]), NEITHER, "the lid"),
            (Source::UPowerRoot, on_root(&[], &["OnBattery"]), UPOWER, "invalidated"),
            (Source::UPowerRoot, added, UPOWER, "a device added"),
            (Source::Battery, on_bat0(DEVICE, &["Percentage", "Energy"]), NEITHER, "charge"),
            (Source::Battery, on_bat0(DEVICE, &["Percentage", "State"]), UPOWER, "state"),
            (Source::Battery, on_bat0(DEVICE, &["IsPresent"]), UPOWER, "taken out"),
            (Source::Battery, on_bat0("org.example.Other", &["IsPresent"]), NEITHER, "other"),
            (Source::Battery, unparsed, UPOWER, "a body that does not parse is read again"),
        ];
        for (source, message, expected, case) in cases {
            assert_eq!(outdated_by(source, &message), expected, "{case}");
        }
    }

    #[test]
    fn a_profiles_signal_outdates_battery_saver_only_for_its_properties() {
        let path = "/org/freedesktop/UPower/PowerProfiles";
        let on = |interface: &str, name: &str| changed(path, interface, &[name], &[]);
        let current = "org.freedesktop.UPower.PowerProfiles";
        let source = Source::Profiles(PowerDaemon::PowerProfiles);
        let cases = [
            (on(current, "ActiveProfile"), PROFILES, "the profile"),
            (on(current, "ActiveProfileHolds"), PROFILES, "a hold"),
            (on(current, "PerformanceDegraded"), NEITHER, "degraded performance"),
            (on("net.hadess.PowerProfiles", "ActiveProfile"), NEITHER, "the other name"),
        ];
        for (message, expected, case) in cases {
            assert_eq!(outdated_by(source, &message), expected, "{case}");
        }
    }

    #[test]
    fn a_daemon_starting_or_replaced_outdates_its_facts_and_one_stopping_does_not() {
        let upower = "org.freedesktop.UPower";
        let (current, legacy) =
            ("org.freedesktop.UPower.PowerProfiles", "net.hadess.PowerProfiles");
        let cases = [
            (upower, "", ":1.42", UPOWER, "UPower started"),
            (upower, ":1.41", ":1.42", UPOWER, "UPower replaced"),
            (upower, ":1.41", "", NEITHER, "UPower stopped"),
            (current, "", ":1.42", PROFILES, "the profiles daemon started"),
            (current, ":1.41", "", NEITHER, "the profiles daemon stopped"),
            (legacy, "", ":1.42", PROFILES, "the legacy name taken"),
            (legacy, ":1.41", "", NEITHER, "the legacy name released"),
            ("org.example.Other", "", ":1.42", NEITHER, "another name"),
        ];
        let bus = ("/org/freedesktop/DBus", "org.freedesktop.DBus");
        for (name, old_owner, new_owner, expected, case) in cases {
            let signal = Message::signal(bus.0, bus.1, "NameOwnerChanged").unwrap();
            let change = OwnerChange::of(&signal.build(&(name, old_owner, new_owner)).unwrap());
            assert_eq!(change.map(|change| started(&change)), Some(expected), "{case}");
        }
    }
}
