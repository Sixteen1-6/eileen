//! Audio endpoints and the system volume.

use eileen_contracts::ids::Personal;
use eileen_contracts::machine::AudioFacts;

use crate::{PlatformError, Subscription, ThreadTicket};

/// The kind of capture stream, which selects the OS's processing for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamCategory {
    /// Voice communication (the OS may apply its own echo cancellation).
    Communications,
    /// Speech recognition: raw capture, Eileen's own echo cancellation.
    Speech,
}

/// An audio endpoint as the OS describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInfo {
    /// The OS's endpoint id.
    pub id: String,
    /// Its friendly name, which may carry the user's name ("Sam's headphones").
    pub name: Personal<String>,
    /// Its mix format's sample rate, Hz.
    pub sample_rate_hz: u32,
    /// Its mix format's channel count.
    pub channels: u16,
}

/// The master volume of the default output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VolumeState {
    /// The volume, 0 to 100.
    pub percent: u8,
    /// Whether it is muted.
    pub muted: bool,
}

/// Audio endpoints (`eileen-audio-win`; cpal does the streaming itself).
pub trait AudioEndpoints: Send + Sync {
    /// The default capture endpoint for `category`.
    fn default_capture(&self, category: StreamCategory) -> Result<EndpointInfo, PlatformError>;
    /// The default render endpoint's loopback, the far-end reference for echo cancellation.
    fn default_render_loopback(&self) -> Result<EndpointInfo, PlatformError>;
    /// Opts the session out of (or back into) Windows' communications ducking.
    fn set_communications_ducking(&self, opt_out: bool) -> Result<(), PlatformError>;
    /// Registers the calling thread with MMCSS; the ticket reverts it on drop.
    fn register_audio_thread(&self) -> Result<ThreadTicket, PlatformError>;
    /// The endpoints for the machine profile's facts (section 2.16): form factor, bus, mix
    /// rate, the defaults, and whether the default render endpoint's loopback opens. Until an
    /// implementation lists them, [`PlatformError::Unavailable`].
    fn endpoints(&self) -> Result<AudioFacts, PlatformError> {
        Err(PlatformError::Unavailable { what: "the audio endpoint list" })
    }
    /// Calls `sink` when an endpoint arrives, leaves or becomes the default
    /// (`IMMNotificationClient`). Until an implementation watches them,
    /// [`PlatformError::Unavailable`].
    fn watch_endpoints(
        &self,
        sink: Box<dyn Fn() + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        drop(sink);
        Err(PlatformError::Unavailable { what: "audio endpoint changes" })
    }
}

/// The system volume (`eileen-desktop-win`, through `IAudioEndpointVolume`), served as the
/// `desktop_volume` tool.
pub trait SystemVolume: Send + Sync {
    /// The volume and mute state.
    fn get(&self) -> Result<VolumeState, PlatformError>;
    /// Sets the volume and mute state.
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "the signature BUILD.md section 3.4 pins"
    )]
    fn set(&self, state: &VolumeState) -> Result<(), PlatformError>;
}

#[cfg(any(test, feature = "fake"))]
pub use fake::{FakeAudioEndpoints, FakeSystemVolume};

#[cfg(any(test, feature = "fake"))]
mod fake {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use eileen_contracts::machine::AudioFacts;

    use super::{AudioEndpoints, EndpointInfo, StreamCategory, SystemVolume, VolumeState};
    use crate::fake_util::{Sinks, lock};
    use crate::{PlatformError, Subscription, ThreadTicket};

    /// Endpoints a test sets, the machine profile's endpoint list among them (`None` answers
    /// `NotFound`), and their changes; registrations are counted and reverted on drop.
    #[derive(Debug)]
    pub struct FakeAudioEndpoints {
        capture: Mutex<Option<EndpointInfo>>,
        loopback: Mutex<Option<EndpointInfo>>,
        ducking_opt_out: Mutex<Option<bool>>,
        registered: Arc<AtomicU32>,
        listed: Mutex<Option<AudioFacts>>,
        changes: Sinks<()>,
    }

    impl Default for FakeAudioEndpoints {
        fn default() -> Self {
            Self {
                capture: Mutex::new(None),
                loopback: Mutex::new(None),
                ducking_opt_out: Mutex::new(None),
                registered: Arc::new(AtomicU32::new(0)),
                listed: Mutex::new(None),
                changes: Sinks::new(),
            }
        }
    }

    impl FakeAudioEndpoints {
        /// Sets the endpoint list [`AudioEndpoints::endpoints`] answers.
        pub fn set_listed(&self, listed: Option<AudioFacts>) {
            *lock(&self.listed) = listed;
        }

        /// Tells every endpoint watcher that an endpoint arrived, left or became the default.
        pub fn endpoints_changed(&self) {
            self.changes.emit(&());
        }

        /// How many endpoint watchers there are.
        #[must_use]
        pub fn endpoint_watchers(&self) -> usize {
            self.changes.len()
        }

        /// Sets the default capture and loopback endpoints (`None` answers `NotFound`).
        pub fn set_endpoints(&self, capture: Option<EndpointInfo>, loopback: Option<EndpointInfo>) {
            *lock(&self.capture) = capture;
            *lock(&self.loopback) = loopback;
        }

        /// The last ducking choice, if one was made.
        #[must_use]
        pub fn ducking_opt_out(&self) -> Option<bool> {
            *lock(&self.ducking_opt_out)
        }

        /// How many threads are registered now.
        #[must_use]
        pub fn registered_threads(&self) -> u32 {
            self.registered.load(Ordering::SeqCst)
        }
    }

    impl AudioEndpoints for FakeAudioEndpoints {
        fn default_capture(&self, _: StreamCategory) -> Result<EndpointInfo, PlatformError> {
            lock(&self.capture).clone().ok_or(PlatformError::NotFound { what: "capture endpoint" })
        }

        fn default_render_loopback(&self) -> Result<EndpointInfo, PlatformError> {
            lock(&self.loopback).clone().ok_or(PlatformError::NotFound { what: "render endpoint" })
        }

        fn set_communications_ducking(&self, opt_out: bool) -> Result<(), PlatformError> {
            *lock(&self.ducking_opt_out) = Some(opt_out);
            Ok(())
        }

        fn register_audio_thread(&self) -> Result<ThreadTicket, PlatformError> {
            self.registered.fetch_add(1, Ordering::SeqCst);
            let registered = Arc::clone(&self.registered);
            Ok(ThreadTicket::new("Pro Audio", move || {
                registered.fetch_sub(1, Ordering::SeqCst);
            }))
        }

        fn endpoints(&self) -> Result<AudioFacts, PlatformError> {
            lock(&self.listed).clone().ok_or(PlatformError::NotFound { what: "audio endpoints" })
        }

        fn watch_endpoints(
            &self,
            sink: Box<dyn Fn() + Send + Sync>,
        ) -> Result<Subscription, PlatformError> {
            Ok(self.changes.add(Box::new(move |()| sink())))
        }
    }

    /// A volume that remembers what it was set to.
    #[derive(Debug)]
    pub struct FakeSystemVolume {
        state: Mutex<VolumeState>,
    }

    impl FakeSystemVolume {
        /// A volume starting at `state`.
        #[must_use]
        pub const fn new(state: VolumeState) -> Self {
            Self { state: Mutex::new(state) }
        }
    }

    impl SystemVolume for FakeSystemVolume {
        fn get(&self) -> Result<VolumeState, PlatformError> {
            Ok(*lock(&self.state))
        }

        fn set(&self, state: &VolumeState) -> Result<(), PlatformError> {
            if state.percent > 100 {
                return Err(PlatformError::Invalid {
                    what: "volume",
                    detail: format!("{} percent", state.percent),
                });
            }
            *lock(&self.state) = *state;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use eileen_contracts::machine::AudioFacts;

    use super::{
        AudioEndpoints, EndpointInfo, FakeAudioEndpoints, FakeSystemVolume, StreamCategory,
        SystemVolume, VolumeState,
    };
    use crate::{PlatformError, ThreadTicket};

    /// An implementation written before the endpoint listing existed, as `eileen-audio-win` and
    /// `eileen-audio-linux` are: it has only the four first methods.
    struct Bare;

    impl AudioEndpoints for Bare {
        fn default_capture(&self, _: StreamCategory) -> Result<EndpointInfo, PlatformError> {
            Err(PlatformError::NotFound { what: "capture endpoint" })
        }

        fn default_render_loopback(&self) -> Result<EndpointInfo, PlatformError> {
            Err(PlatformError::NotFound { what: "render endpoint" })
        }

        fn set_communications_ducking(&self, _: bool) -> Result<(), PlatformError> {
            Ok(())
        }

        fn register_audio_thread(&self) -> Result<ThreadTicket, PlatformError> {
            Ok(ThreadTicket::new("Pro Audio", || {}))
        }
    }

    #[test]
    fn an_implementation_without_the_listing_answers_unavailable() {
        let listed = Bare.endpoints();
        assert!(matches!(listed, Err(PlatformError::Unavailable { .. })), "{listed:?}");
        let watched = Bare.watch_endpoints(Box::new(|| {}));
        assert!(matches!(watched, Err(PlatformError::Unavailable { .. })), "{watched:?}");
    }

    #[test]
    fn the_fake_lists_what_the_test_set_and_tells_its_watchers_of_a_change() {
        let fake = FakeAudioEndpoints::default();
        assert!(fake.endpoints().is_err(), "nothing listed yet");
        let listed = AudioFacts { loopback: true, ..AudioFacts::default() };
        fake.set_listed(Some(listed.clone()));
        assert_eq!(fake.endpoints().unwrap(), listed, "as set");
        let heard = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&heard);
        let watch = fake
            .watch_endpoints(Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }))
            .unwrap();
        fake.endpoints_changed();
        assert_eq!(heard.load(Ordering::SeqCst), 1, "told once");
        drop(watch);
        assert_eq!(fake.endpoint_watchers(), 0, "unsubscribed on drop");
    }

    #[test]
    fn a_registration_reverts_on_drop() {
        let fake = FakeAudioEndpoints::default();
        let ticket = fake.register_audio_thread().unwrap();
        assert_eq!(fake.registered_threads(), 1, "registered");
        drop(ticket);
        assert_eq!(fake.registered_threads(), 0, "reverted");
        assert!(fake.default_capture(super::StreamCategory::Speech).is_err(), "no endpoint set");
    }

    #[test]
    fn the_volume_refuses_out_of_range_values() {
        let volume = FakeSystemVolume::new(VolumeState { percent: 40, muted: false });
        assert!(volume.set(&VolumeState { percent: 101, muted: false }).is_err(), "over 100");
        volume.set(&VolumeState { percent: 70, muted: true }).unwrap();
        assert_eq!(volume.get().unwrap(), VolumeState { percent: 70, muted: true }, "stored");
    }
}
