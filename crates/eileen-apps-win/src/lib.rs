//! Eileen's app backends on Windows (BUILD.md sections 2.7 and 3.4): `eileen-platform`'s
//! `AppHost` for Word, Excel, Explorer and Photoshop over COM and for the Resolve process gate,
//! `Apps` (the Start menu index and launch), `MediaSessions` (one SMTC manager for the process
//! lifetime) and `RecentSources` (the Recent folder and thumbnails). A stub until WP6 builds it:
//! every call answers `PlatformError::Unavailable`.
//!
//! # Owns
//!
//! - `WinAppHost`, `WinApps`, `WinMediaSessions` and `WinRecentSources`, the Windows
//!   implementations of those traits.
//! - Opening URLs, allowlisted URIs and paths through `eileen_supervisor::user_launch`.
//!
//! # Does not own
//!
//! - The tools and their logic (`eileen-apps`), the Resolve bridge (`adapters/resolve/`), or the
//!   traits (`eileen-platform`).
//!
//! # Invariants
//!
//! - Windows only: the crate is `cfg(windows)` and compiles to an empty library elsewhere (the
//!   `logic-linux` CI job builds it on Linux, where it has no dependencies).
//! - Unsafe code only in a private `ffi` module, one operation per block (check 10).
//! - COM calls run on the crate's own apartment thread with a deadline, never on a tokio worker
//!   (WP6 adds the test, against a fake COM target).
//! - A file the user's profile may keep on another machine is read within the caller's
//!   `Deadline`, on a helper thread abandoned past it (section 3.4; WP6 adds the test).
//! - The stub answers `PlatformError::Unavailable` (unit test
//!   `tests::the_stub_backends_answer_unavailable`, run on Windows).
//!
//! # Boundaries
//!
//! - Called by `eileen-apps` (under its Windows target table).
//! - May depend on `eileen-platform`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Must not depend on `eileen-apps` (the dependency runs the other way), another `-win` crate,
//!   `eileen-rt` or rmcp, nor know vendor CLI flags (section 3.1); nor on a native library that
//!   section 4.1 keeps for other crates, such as `ort` or `cpal` (check 15).
//! - Wire types: none of its own; app operations are contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-apps-win` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-apps-win`).
//! - Layers: desktop tests in the `desktop` group with the apps a test starts itself, and
//!   Resolve tests on the owner's PC.
//! - A bug here starts as a COM call that never returned, or a media control the session
//!   ignored.
//!
//! # Traps
//!
//! - SMTC control is confirmed by the next event, not by the call's return.
//!
//! # Logs and tracing
//!
//! - None yet; WP6 names its spans from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - Which URIs are allowlisted for `apps_open_uri`.

#![cfg(windows)]

use eileen_contracts::desktop::{MediaState, WindowRef};
use eileen_contracts::tools::apps::MediaAction;
use eileen_platform::{
    AppEntry, AppHost, AppOp, AppProcess, Apps, Deadline, MediaSessions, PlatformError, RecentItem,
    RecentSources, RgbaImage, Subscription,
};

/// What every stub call answers.
const NOT_BUILT: PlatformError = PlatformError::Unavailable { what: "app automation" };

/// One app's host over COM, or the Resolve process gate. A stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinAppHost {
    app: &'static str,
}

impl WinAppHost {
    /// The host for `app`, for example `"word"`.
    #[must_use]
    pub const fn new(app: &'static str) -> Self {
        Self { app }
    }
}

impl AppHost for WinAppHost {
    fn app(&self) -> &'static str {
        self.app
    }

    fn running(&self) -> Result<Option<AppProcess>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn call(&self, _op: &AppOp, _deadline: Deadline) -> Result<serde_json::Value, PlatformError> {
        Err(NOT_BUILT)
    }
}

/// The Start menu's app index and launch. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinApps;

impl Apps for WinApps {
    fn index(&self) -> Result<Vec<AppEntry>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn launch(&self, _app: &AppEntry) -> Result<u32, PlatformError> {
        Err(NOT_BUILT)
    }

    fn main_window(
        &self,
        _pid: u32,
        _deadline: Deadline,
    ) -> Result<Option<WindowRef>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn open_url(&self, _url: &str) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn open_uri(&self, _uri: &str) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn open_path(&self, _path: &str, _with_app: Option<&AppEntry>) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }
}

/// Media sessions through SMTC. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinMediaSessions;

impl MediaSessions for WinMediaSessions {
    fn sessions(&self) -> Result<Vec<MediaState>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn control(&self, _session: &str, _action: &MediaAction) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn subscribe(
        &self,
        _sink: Box<dyn Fn(MediaState) + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        Err(NOT_BUILT)
    }
}

/// Recent files from the Recent folder, and their thumbnails. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinRecentSources;

impl RecentSources for WinRecentSources {
    fn recent(&self, _limit: usize) -> Result<Vec<RecentItem>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn watch(
        &self,
        _roots: &[String],
        _sink: Box<dyn Fn(RecentItem) + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        Err(NOT_BUILT)
    }

    fn thumbnail(
        &self,
        _item: &RecentItem,
        _edge_px: u32,
    ) -> Result<Option<RgbaImage>, PlatformError> {
        Err(NOT_BUILT)
    }
}

#[cfg(test)]
mod tests {
    use eileen_platform::{AppHost, Apps, PlatformError};

    use super::{WinAppHost, WinApps};

    #[test]
    fn the_stub_backends_answer_unavailable() {
        let unavailable = PlatformError::Unavailable { what: "app automation" };
        let word = WinAppHost::new("word");
        assert_eq!(word.app(), "word", "the host's app");
        assert_eq!(word.running(), Err(unavailable.clone()), "COM");
        assert_eq!(WinApps.open_url("https://example.com"), Err(unavailable), "launch");
    }
}
