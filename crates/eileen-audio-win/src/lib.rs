//! Eileen's audio endpoints on Windows (BUILD.md sections 2.8 and 3.4): `eileen-platform`'s
//! `AudioEndpoints` through the WASAPI endpoint APIs, with the loopback of the default render
//! endpoint as the far-end reference, the Communications category with Windows ducking opted out,
//! and MMCSS for the audio threads. cpal does the streaming itself, in `eileen-audio`. A stub
//! until WP4 builds it: every call answers `PlatformError::Unavailable`.
//!
//! # Owns
//!
//! - `WinAudioEndpoints`, the Windows implementation of `AudioEndpoints`, and from WP0 phase B
//!   its `endpoints` and `watch_endpoints` through `IMMNotificationClient`.
//! - MMCSS registration for the audio threads, reverted on the registering thread.
//!
//! # Does not own
//!
//! - Streaming and echo cancellation (`eileen-audio`), the system volume
//!   (`eileen-desktop-win`'s `SystemVolume`), or the trait (`eileen-platform`).
//!
//! # Invariants
//!
//! - Windows only: the crate is `cfg(windows)` and compiles to an empty library elsewhere (the
//!   `logic-linux` CI job builds it on Linux, where it has no dependencies).
//! - Unsafe code only in a private `ffi` module, one operation per block (check 10).
//! - Endpoint names leave the process only as `Personal` (check 26's text-wrapping test; WP4
//!   acceptance 14).
//! - The stub answers `PlatformError::Unavailable` (unit test
//!   `tests::the_stub_endpoints_answer_unavailable`, run on Windows).
//!
//! # Boundaries
//!
//! - Called by `eileen-audio` and `eileen-speech` (under their Windows target tables).
//! - May depend on `eileen-platform`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Must not depend on `eileen-audio` or `eileen-speech` (they depend on it), another `-win`
//!   crate, `eileen-rt` or rmcp, nor know vendor CLI flags (section 3.1); nor on a native library
//!   that section 4.1 keeps for other crates, such as `ort` (check 15).
//! - Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-audio-win` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-audio-win`).
//! - Layers: device tests on the owner's PC under `desktop.lock`; the hosted runner has no audio
//!   device.
//! - A bug here starts as echo cancellation fed the wrong reference.
//!
//! # Traps
//!
//! - An MMCSS ticket is not `Send`: it reverts on the thread that registered.
//!
//! # Logs and tracing
//!
//! - None yet; WP4 names its spans from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - The ducking opt-out and the stream category Eileen captures with.

#![cfg(windows)]

use eileen_platform::{AudioEndpoints, EndpointInfo, PlatformError, StreamCategory, ThreadTicket};

/// What every stub call answers.
const NOT_BUILT: PlatformError = PlatformError::Unavailable { what: "audio endpoints" };

/// The audio endpoints of this PC. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinAudioEndpoints;

impl AudioEndpoints for WinAudioEndpoints {
    fn default_capture(&self, _category: StreamCategory) -> Result<EndpointInfo, PlatformError> {
        Err(NOT_BUILT)
    }

    fn default_render_loopback(&self) -> Result<EndpointInfo, PlatformError> {
        Err(NOT_BUILT)
    }

    fn set_communications_ducking(&self, _opt_out: bool) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn register_audio_thread(&self) -> Result<ThreadTicket, PlatformError> {
        Err(NOT_BUILT)
    }
}

#[cfg(test)]
mod tests {
    use eileen_platform::{AudioEndpoints, PlatformError};

    use super::WinAudioEndpoints;

    #[test]
    fn the_stub_endpoints_answer_unavailable() {
        let unavailable = PlatformError::Unavailable { what: "audio endpoints" };
        assert_eq!(WinAudioEndpoints.default_render_loopback(), Err(unavailable), "loopback");
    }
}
