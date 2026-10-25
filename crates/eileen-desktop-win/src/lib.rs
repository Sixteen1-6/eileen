//! Eileen's desktop backend on Windows (BUILD.md sections 2.6 and 3.4): `eileen-platform`'s
//! `Grounding`, `Automation`, `Input`, `Capture`, `TextInsertion` and `SystemVolume`, through UI
//! Automation on its own MTA thread, `SendInput` with the foreground and physical-input guard,
//! `BitBlt` and `PrintWindow` capture, and `IAudioEndpointVolume`. A stub until WP5 builds it:
//! every call answers `PlatformError::Unavailable`.
//!
//! # Owns
//!
//! - [`WinGrounding`], [`WinAutomation`], [`WinInput`], [`WinCapture`], [`WinTextInsertion`]
//!   and [`WinSystemVolume`], the Windows implementations of those traits.
//! - UI Automation's apartment thread with deadlines, abandoned and replaced on a hang; Raw
//!   Input for physical activity; the `testpad` fixture.
//!
//! # Does not own
//!
//! - The tools and their safety rules (`eileen-desktop`), the traits (`eileen-platform`), or
//!   launching apps for the user (`eileen_supervisor::user_launch`).
//!
//! # Invariants
//!
//! - Windows only: the crate is `cfg(windows)` and compiles to an empty library elsewhere.
//! - Unsafe code only in a private `ffi` module, one operation per block (check 10).
//! - COM calls run on the crate's own apartment thread with a deadline, never on a tokio worker.
//! - The Raw Input module stores no key code but the dictation key and no pointer position
//!   (check 25).
//!
//! # Boundaries
//!
//! - Called by `eileen-desktop` (under its Windows target table).
//! - May depend on `eileen-platform`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-desktop-win` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-desktop-win`).
//! - Layers: desktop tests in the `desktop` group under `desktop.lock`, on windows the test
//!   created; the canary of WP5 acceptance 9 first in each nightly.
//! - A bug here starts as a UI Automation call that hung past its deadline.
//!
//! # Traps
//!
//! - Chromium and Electron apps get on-demand UI Automation, never a global focus handler.
//!
//! # Logs and tracing
//!
//! - None yet; WP5 names its spans from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - The physical-input guard and the lease rules of section 2.6.

#![cfg(windows)]

use eileen_contracts::core_io::Verification;
use eileen_contracts::desktop::{
    DesktopChange, ElementInfo, Monitor, Point, PresentationState, Selection, Selector, WindowInfo,
    WindowRef,
};
use eileen_contracts::tools::desktop::{InsertTarget, ShotTarget};
use eileen_platform::{
    Automation, Capture, Deadline, ElementCandidate, ElementHandle, ElementTree, ForegroundGuard,
    ForegroundOutcome, Grounding, Input, InputEvent, InsertMethod, InsertOutcome, PlatformError,
    RestoreToken, RgbaImage, SendOutcome, Subscription, SystemVolume, TextInsertion, TreeLimits,
    VolumeState,
};

/// What every stub call answers.
const NOT_BUILT: PlatformError = PlatformError::Unavailable { what: "desktop control" };

/// Grounding: monitors, windows, the foreground, elements and the selection. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinGrounding;

impl Grounding for WinGrounding {
    fn monitors(&self) -> Result<Vec<Monitor>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn windows(&self, _include_minimized: bool) -> Result<Vec<WindowInfo>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn foreground(&self) -> Result<Option<WindowRef>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn window_from_point(&self, _p: Point) -> Result<Option<WindowRef>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn focused_element(&self, _deadline: Deadline) -> Result<Option<ElementInfo>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn element_from_point(
        &self,
        _p: Point,
        _deadline: Deadline,
    ) -> Result<Option<ElementInfo>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn selection(
        &self,
        _allow_clipboard: bool,
        _deadline: Deadline,
    ) -> Result<Option<Selection>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn presentation_state(&self) -> Result<PresentationState, PlatformError> {
        Err(NOT_BUILT)
    }

    fn mic_in_use_elsewhere(&self) -> Result<bool, PlatformError> {
        Err(NOT_BUILT)
    }

    fn subscribe(
        &self,
        _sink: Box<dyn Fn(DesktopChange) + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        Err(NOT_BUILT)
    }
}

/// UI Automation: element trees, invoke, set value and find. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinAutomation;

impl Automation for WinAutomation {
    fn tree(
        &self,
        _hwnd: i64,
        _limits: TreeLimits,
        _deadline: Deadline,
    ) -> Result<ElementTree, PlatformError> {
        Err(NOT_BUILT)
    }

    fn invoke(
        &self,
        _element: &ElementHandle,
        _deadline: Deadline,
    ) -> Result<Verification, PlatformError> {
        Err(NOT_BUILT)
    }

    fn set_value(
        &self,
        _element: &ElementHandle,
        _value: &str,
        _deadline: Deadline,
    ) -> Result<Verification, PlatformError> {
        Err(NOT_BUILT)
    }

    fn find(
        &self,
        _hwnd: i64,
        _selector: &Selector,
        _deadline: Deadline,
    ) -> Result<Vec<ElementCandidate>, PlatformError> {
        Err(NOT_BUILT)
    }
}

/// Input: idle time, the foreground and guarded synthetic input. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinInput;

impl Input for WinInput {
    fn user_idle_ms(&self) -> Result<u32, PlatformError> {
        Err(NOT_BUILT)
    }

    fn take_foreground(&self, _hwnd: i64) -> Result<ForegroundOutcome, PlatformError> {
        Err(NOT_BUILT)
    }

    fn send(
        &self,
        _hwnd: i64,
        _events: &[InputEvent],
        _guard: ForegroundGuard,
    ) -> Result<SendOutcome, PlatformError> {
        Err(NOT_BUILT)
    }

    fn restore(&self, _token: RestoreToken, _cursor_if_unmoved: bool) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }
}

/// Screen and window capture. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinCapture;

impl Capture for WinCapture {
    fn capture(&self, _target: &ShotTarget) -> Result<RgbaImage, PlatformError> {
        Err(NOT_BUILT)
    }
}

/// Text insertion, the bottom of the insertion ladder. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinTextInsertion;

impl TextInsertion for WinTextInsertion {
    fn insert(
        &self,
        _text: &str,
        _target: &InsertTarget,
        _method: InsertMethod,
    ) -> Result<InsertOutcome, PlatformError> {
        Err(NOT_BUILT)
    }
}

/// The system volume through `IAudioEndpointVolume`, served as `desktop_volume`. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinSystemVolume;

impl SystemVolume for WinSystemVolume {
    fn get(&self) -> Result<VolumeState, PlatformError> {
        Err(NOT_BUILT)
    }

    fn set(&self, _state: &VolumeState) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }
}

#[cfg(test)]
mod tests {
    use eileen_platform::{Grounding, Input, PlatformError, SystemVolume};

    use super::{WinGrounding, WinInput, WinSystemVolume};

    #[test]
    fn the_stub_backend_answers_unavailable() {
        let unavailable = PlatformError::Unavailable { what: "desktop control" };
        assert_eq!(WinGrounding.foreground(), Err(unavailable.clone()), "grounding");
        assert_eq!(WinInput.user_idle_ms(), Err(unavailable.clone()), "input");
        assert_eq!(WinSystemVolume.get(), Err(unavailable), "volume");
    }
}
