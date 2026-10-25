//! Eileen's shell on Windows (BUILD.md sections 2.1, 2.5, 2.12 and 3.4): `eileen-platform`'s
//! `ShellHost` (the tray with its data-driven menu, global hotkeys, Raw Input for the dictation
//! key and physical activity, toasts, the Run entry, power requests, the session lock),
//! `SystemMemory`, `MachineIdentity` and `SecretStore`, and the shell `component`. A stub until
//! WP2 builds it: every call answers `PlatformError::Unavailable`.
//!
//! # Owns
//!
//! - `WinShell`, `WinSystemMemory`, `WinMachineIdentity` and `WinSecretStore`, the
//!   Windows implementations of those traits; from WP0 phase B, `MachineProbe` too.
//! - The shell `component`, which shows toasts and runs the tray and hotkeys on the core's
//!   shell thread.
//! - The Start menu shortcut with the `AppUserModelID`, and the `HKCU` Run entry.
//!
//! # Does not own
//!
//! - The traits (`eileen-platform`), what a hotkey or a tray click means (`eileen-core`), or the
//!   sampler that turns memory figures into inputs (`eileen-runtime`).
//!
//! # Invariants
//!
//! - Windows only: the crate is `cfg(windows)` and compiles to an empty library elsewhere (the
//!   `logic-linux` CI job builds it on Linux, where it has no dependencies).
//! - Unsafe code only in a private `ffi` module, one operation per block (check 10).
//! - The Raw Input module stores no key code but the dictation key, and for the approve, deny
//!   and unmute chords only a "came from a device" flag; never a pointer position (check 25).
//! - Its native libraries appear only under its Windows target table (section 4.1, check 15).
//! - The stub answers `PlatformError::Unavailable` and takes toasts (unit test
//!   `tests::the_stub_shell_answers_unavailable_and_takes_toasts`, run on Windows).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli` (under its Windows target table), the composition root.
//! - May depend on `eileen-platform`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`, and
//!   on `keyring-core`, `windows-native-keyring-store`, `tray-icon`, `global-hotkey` and
//!   `tauri-winrt-notification`.
//! - Must not depend on `eileen-cli` or `eileen-runtime`, another `-win` crate, `eileen-rt` or
//!   rmcp, nor know vendor CLI flags (section 3.1); nor on a native library that section 4.1
//!   keeps for other crates, such as `cpal` or `ort` (check 15).
//! - Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-shell-win` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-shell-win`).
//! - Layers: the few tray and hotkey tests in the `desktop` group under `desktop.lock`, and
//!   `owner_pc::` tests of the machine id.
//! - A bug here starts as a hotkey another app took, or an unmute from a synthetic click.
//!
//! # Traps
//!
//! - The raw `MachineGuid` never reaches a file: only its hash with the install id does.
//!
//! # Logs and tracing
//!
//! - None yet; WP2 names its events from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - The physical-click check for unmute and which chords count only from a device.

#![cfg(windows)]

use eileen_contracts::core_io::{EffectFamily, Toast};
use eileen_contracts::jobs::MemoryPressure;
use eileen_contracts::ports::{Component, IdleComponent};
use eileen_platform::{
    MachineIdentity, PlatformError, PowerRequest, SecretStore, SecretString, ShellHost,
    ShellInputEvent, Subscription, SystemMemory, TrayState, VirtualKey,
};

/// What every stub call answers.
const NOT_BUILT: PlatformError = PlatformError::Unavailable { what: "the Windows shell" };

/// The tray, hotkeys, Raw Input, toasts, the Run entry and the session. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinShell;

impl ShellHost for WinShell {
    fn register_hotkey(&self, _chord: &str, _id: u32) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn set_tray(&self, _state: TrayState) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn toast(&self, _toast: &Toast) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn watch_input(
        &self,
        _dictation_key: VirtualKey,
        _sink: Box<dyn Fn(ShellInputEvent) + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        Err(NOT_BUILT)
    }

    fn set_run_at_sign_in(&self, _on: bool) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn keep_awake(&self) -> Result<PowerRequest, PlatformError> {
        Err(NOT_BUILT)
    }

    fn lock_workstation(&self) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn session_locked(&self) -> Result<bool, PlatformError> {
        Err(NOT_BUILT)
    }
}

/// Memory figures from `GlobalMemoryStatusEx`, the commit figures and the PDH paging counter.
/// A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinSystemMemory;

impl SystemMemory for WinSystemMemory {
    fn sample(&self) -> Result<MemoryPressure, PlatformError> {
        Err(NOT_BUILT)
    }
}

/// The OS's machine id, `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinMachineIdentity;

impl MachineIdentity for WinMachineIdentity {
    fn os_machine_id(&self) -> Result<Option<String>, PlatformError> {
        Err(NOT_BUILT)
    }
}

/// Secrets in Windows Credential Manager through `keyring-core`. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinSecretStore;

impl SecretStore for WinSecretStore {
    fn get(&self, _name: &str) -> Result<Option<SecretString>, PlatformError> {
        Err(NOT_BUILT)
    }

    fn set(&self, _name: &str, _value: &SecretString) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn delete(&self, _name: &str) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }
}

/// What the shell component reads at start. WP2 adds its fields (the hotkeys, the tray menu).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShellConfig;

/// The shell component. The stub accepts toasts and does nothing.
#[must_use]
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "section 3.3.15's signature; ShellConfig has no fields until WP2"
)]
pub fn component(_cfg: &ShellConfig) -> Box<dyn Component> {
    Box::new(IdleComponent::new("shell", &[EffectFamily::Notify]))
}

#[cfg(test)]
mod tests {
    use eileen_contracts::core_io::EffectFamily;
    use eileen_platform::{MachineIdentity, PlatformError, ShellHost};

    use super::{ShellConfig, WinMachineIdentity, WinShell, component};

    #[test]
    fn the_stub_shell_answers_unavailable_and_takes_toasts() {
        let unavailable = PlatformError::Unavailable { what: "the Windows shell" };
        assert_eq!(WinShell.session_locked(), Err(unavailable.clone()), "the session");
        assert_eq!(WinMachineIdentity.os_machine_id(), Err(unavailable), "the machine id");
        let shell = component(&ShellConfig);
        assert_eq!(shell.accepts(), [EffectFamily::Notify], "toasts");
    }
}
