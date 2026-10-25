//! Audio endpoints and the system volume.

use eileen_contracts::ids::Personal;

use crate::{PlatformError, ThreadTicket};

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

    use super::{AudioEndpoints, EndpointInfo, StreamCategory, SystemVolume, VolumeState};
    use crate::fake_util::lock;
    use crate::{PlatformError, ThreadTicket};

    /// Endpoints a test sets; registrations are counted and reverted on drop.
    #[derive(Debug, Default)]
    pub struct FakeAudioEndpoints {
        capture: Mutex<Option<EndpointInfo>>,
        loopback: Mutex<Option<EndpointInfo>>,
        ducking_opt_out: Mutex<Option<bool>>,
        registered: Arc<AtomicU32>,
    }

    impl FakeAudioEndpoints {
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
    use super::{AudioEndpoints, FakeAudioEndpoints, FakeSystemVolume, SystemVolume, VolumeState};

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
