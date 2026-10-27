//! [`LinuxMedia`]: `eileen-platform`'s `MediaSessions` over MPRIS, through the D-Bus bridge.
//! Each method runs [`MprisSessions`] on the bridge's session bus and converts what it answers:
//! a `DbusError` becomes its `PlatformError` twin, and each [`MprisEvent`] the state the sink
//! takes. What a player reports, and how, is `MprisSessions`' and `MprisWatch`'s to decide.

use eileen_contracts::desktop::MediaState;
use eileen_contracts::tools::apps::MediaAction;
use eileen_dbus_linux::{Bridge, Buses, DEFAULT_DEADLINE};
use eileen_platform::{Deadline, MediaSessions, PlatformError, Subscription};

use crate::{MprisEvent, MprisSessions};

/// What `sessions` names when it passes its deadline.
const SESSIONS: &str = "media sessions";

/// What `control` names when it passes its deadline.
const CONTROL: &str = "media control";

/// What `subscribe` names when its watch does not open in time.
const SUBSCRIBE: &str = "media subscription";

/// The media sessions of the user's session bus: `MediaSessions` on Linux, served by
/// [`MprisSessions`] on a [`Bridge`].
///
/// - `sessions` and `control` are [`MprisSessions::sessions`] and [`MprisSessions::control`],
///   whose docs give the rules. The trait takes no `Deadline`, so each waits at most
///   [`DEFAULT_DEADLINE`], the connection's method timeout and half a second, unless
///   [`Self::new_within`] gives another; past it the call is `DeadlineExceeded`, naming
///   "media sessions" or "media control".
/// - The listing reads the players at the same time, each player's read, its `Player`
///   properties and then its root interface, within one method timeout as a whole, so players
///   that never answer, or answer late and never name themselves, cost it one method timeout
///   together: within [`DEFAULT_DEADLINE`], it answers without them with about half a second to
///   spare. The bus itself can still hold it past its deadline: connecting has no timeout, and
///   the listing asks for the players' names and owners one call at a time before it reads them.
/// - `subscribe` runs a [`MprisWatch`](crate::MprisWatch) on the bridge, and calls the sink
///   with each player's state when it opens, then with each change. A player that arrives is
///   its state; one that leaves is its last state with `playing` false
///   ([`MprisEvent::into_state`]). The watch ends, and the sink with it, when the
///   [`Subscription`] drops or the session bus connection ends.
///
/// The sink runs on the bridge's one thread. It must not block it, nor wait on the bridge, as
/// a call to this adapter does: every bridge call would wait behind it until its deadline.
#[derive(Debug, Clone)]
pub struct LinuxMedia {
    bridge: Bridge,
    /// How long each call waits for its answer.
    deadline: Deadline,
}

impl LinuxMedia {
    /// The media sessions on `bridge`'s session bus, each call waiting at most
    /// [`DEFAULT_DEADLINE`].
    #[must_use]
    pub const fn new(bridge: Bridge) -> Self {
        Self::new_within(bridge, DEFAULT_DEADLINE)
    }

    /// The media sessions on `bridge`'s session bus, each call waiting at most `deadline`
    /// instead: for a caller that needs an answer sooner, such as a test that holds a call past
    /// its deadline. Below the connection's method timeout, one player that never answers makes
    /// the whole listing pass its deadline, where [`Self::new`] would leave that player out.
    #[must_use]
    pub const fn new_within(bridge: Bridge, deadline: Deadline) -> Self {
        Self { bridge, deadline }
    }
}

impl MediaSessions for LinuxMedia {
    fn sessions(&self) -> Result<Vec<MediaState>, PlatformError> {
        self.bridge.call(SESSIONS, self.deadline, |buses| async move {
            let bus = buses.session().await?;
            Ok(MprisSessions::new(&bus).sessions().await?)
        })
    }

    fn control(&self, session: &str, action: &MediaAction) -> Result<(), PlatformError> {
        let (session, action) = (session.to_owned(), *action);
        self.bridge.call(CONTROL, self.deadline, move |buses| async move {
            let bus = buses.session().await?;
            Ok(MprisSessions::new(&bus).control(&session, &action).await?)
        })
    }

    fn subscribe(
        &self,
        sink: Box<dyn Fn(MediaState) + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        let open = |buses: Buses| async move {
            let bus = buses.session().await?;
            Ok(MprisSessions::new(&bus).subscribe().await?)
        };
        let each = move |event: MprisEvent| sink(event.into_state());
        self.bridge.watch(SUBSCRIBE, self.deadline, open, each)
    }
}

#[cfg(test)]
mod tests {
    use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE};

    use super::LinuxMedia;

    // No fake can hold a listing past the default deadline without a timer, since the listing
    // reads the players at once: what `new` gives is checked here, and the integration tests
    // check that a listing waits the deadline it is given.
    #[test]
    fn new_waits_the_default_deadline() {
        // The bridge connects nothing until a call needs a bus.
        let media = LinuxMedia::new(Bridge::start().unwrap());
        assert_eq!(media.deadline, DEFAULT_DEADLINE, "as for any trait method that takes none");
    }
}
