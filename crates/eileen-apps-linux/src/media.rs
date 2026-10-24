//! [`LinuxMedia`]: `eileen-platform`'s `MediaSessions` over MPRIS, through the D-Bus bridge.
//! Each method runs [`MprisSessions`] on the bridge's session bus and converts what it answers:
//! a `DbusError` becomes its `PlatformError` twin, and each [`MprisEvent`] the state the sink
//! takes. What a player reports, and how, is `MprisSessions`' and `MprisWatch`'s to decide.

use eileen_contracts::desktop::MediaState;
use eileen_contracts::tools::apps::MediaAction;
use eileen_dbus_linux::{Bridge, Buses, DEFAULT_DEADLINE};
use eileen_platform::{MediaSessions, PlatformError, Subscription};

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
///   [`DEFAULT_DEADLINE`]: the connection's method timeout and half a second. A player that
///   never answers holds a call for that whole timeout: `sessions` leaves one such player out
///   with half a second left for the others, and passes its deadline when there are two.
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
}

impl LinuxMedia {
    /// The media sessions on `bridge`'s session bus.
    #[must_use]
    pub const fn new(bridge: Bridge) -> Self {
        Self { bridge }
    }
}

impl MediaSessions for LinuxMedia {
    fn sessions(&self) -> Result<Vec<MediaState>, PlatformError> {
        self.bridge.call(SESSIONS, DEFAULT_DEADLINE, |buses| async move {
            let bus = buses.session().await?;
            Ok(MprisSessions::new(&bus).sessions().await?)
        })
    }

    fn control(&self, session: &str, action: &MediaAction) -> Result<(), PlatformError> {
        let (session, action) = (session.to_owned(), *action);
        self.bridge.call(CONTROL, DEFAULT_DEADLINE, move |buses| async move {
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
        self.bridge.watch(SUBSCRIBE, DEFAULT_DEADLINE, open, each)
    }
}
