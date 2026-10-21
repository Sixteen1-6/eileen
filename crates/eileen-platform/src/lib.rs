//! The traits every operating system implements for Eileen (BUILD.md section 3.4), a fake
//! beside each for tests, and the fleet's machine hash ([`machine`], owner answer 6).
//!
//! # Owns
//!
//! - The platform traits ([`Grounding`], [`Automation`], [`Input`], [`ShellHost`], ...) and
//!   the handle and value types only their signatures use ([`PlatformError`], [`Deadline`],
//!   [`Subscription`], ...).
//! - The fakes beside each trait, under `cfg(any(test, feature = "fake"))`.
//! - [`machine`]: the OS id normalisation, the machine hash and [`machine::FileMachineIdentity`].
//!
//! # Does not own
//!
//! - Any implementation that touches the OS: those are the `-win` crates (`eileen-shell-win`,
//!   `eileen-desktop-win`, `eileen-apps-win`, `eileen-audio-win`, `eileen-hud-win`).
//! - Types that cross a process: those are `eileen-contracts` (an owning package moves a type
//!   there by amendment when it starts crossing one).
//!
//! # Invariants
//!
//! - Neutral: no native dependency, no `cfg(windows)`; builds and tests on Linux (CI lanes).
//! - One machine hash derivation for every OS, pinned by a golden vector
//!   (`it::machine::the_golden_vector_is_stable`, run on Windows and Linux).
//! - A shipped binary never enables `fake` (check 15 reads the feature lists).
//!
//! # Boundaries
//!
//! - Called by the `-win` crates (implementations), `eileen-runtime` (`SystemMemory`,
//!   `MachineIdentity`, `machine`), `eileen-config` (`SecretStore`) and the sidecars' servers.
//! - May depend on `eileen-contracts`, `serde_json`, `secrecy`, `blake3` and `thiserror`;
//!   must not depend on any `-win` crate or any other Eileen crate.
//! - Wire types: none; everything that crosses a process is a contract type.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-platform` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-platform`).
//! - Layers: unit tests for each fake, integration tests for [`machine`] with a temporary root.
//! - A bug here starts as a fleet result whose machine hash changed between runs.
//!
//! # Traps
//!
//! - [`machine::machine_hash`] normalises its OS id itself: pass the raw value, and anything
//!   unusable counts as no id at all.
//! - [`HudSurface`] is the only trait that is not `Send + Sync`: it lives on the HUD's thread.
//! - Fakes call subscribers while holding their lock; a subscriber must not unsubscribe from
//!   inside its own callback.
//!
//! # Logs and tracing
//!
//! - None: traits do no IO. Implementations name their spans from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - [`machine::SALT`] and the hash's encoding (every recorded fleet result would change).
//! - A trait method's signature (amendment to BUILD.md section 3.4).

#![forbid(unsafe_code)]

mod apps;
mod audio;
mod desktop;
mod error;
#[cfg(any(test, feature = "fake"))]
mod fake_util;
mod handles;
mod hud;
mod identity;
pub mod machine;
mod memory;
mod secrets;
mod shell;

pub use apps::{
    AppEntry, AppHost, AppKind, AppOp, AppProcess, Apps, MediaSessions, RecentItem, RecentSources,
};
#[cfg(any(test, feature = "fake"))]
pub use apps::{FakeAppHost, FakeApps, FakeMediaSessions, FakeRecentSources};
pub use audio::{AudioEndpoints, EndpointInfo, StreamCategory, SystemVolume, VolumeState};
#[cfg(any(test, feature = "fake"))]
pub use audio::{FakeAudioEndpoints, FakeSystemVolume};
pub use desktop::{
    Automation, Capture, ElementCandidate, ElementHandle, ElementNode, ElementTree,
    ForegroundGuard, ForegroundOutcome, Grounding, Input, InputEvent, InsertMethod, InsertOutcome,
    RestoreToken, SendOutcome, TextInsertion, TreeLimits,
};
#[cfg(any(test, feature = "fake"))]
pub use desktop::{FakeAutomation, FakeCapture, FakeGrounding, FakeInput, FakeTextInsertion};
pub use error::PlatformError;
pub use handles::{Deadline, PowerRequest, RgbaImage, Subscription, ThreadTicket};
#[cfg(any(test, feature = "fake"))]
pub use hud::FakeHudSurface;
pub use hud::{DrawItem, DrawList, HudSurface};
#[cfg(any(test, feature = "fake"))]
pub use identity::FakeMachineIdentity;
pub use identity::MachineIdentity;
#[cfg(any(test, feature = "fake"))]
pub use memory::FakeSystemMemory;
pub use memory::SystemMemory;
pub use secrecy::SecretString;
#[cfg(any(test, feature = "fake"))]
pub use secrets::FakeSecretStore;
pub use secrets::SecretStore;
#[cfg(any(test, feature = "fake"))]
pub use shell::FakeShellHost;
pub use shell::{ShellHost, ShellInputEvent, TrayItem, TrayState, VirtualKey};
