//! Connectors (BUILD.md section 3.3.12): what each sidecar declares, and what passes between
//! the core and a sidecar's client.
//!
//! Provisional: WP2 pins this module in its first pull request (BUILD.md section 3.3).
//!
//! Each sidecar answers a `connector_manifest` call (core-only, never shown to models) right
//! after the MCP handshake. Each adapter fills `AppCapability.version` from the app itself,
//! which is what puts a recipe on probation after an app update.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core_io::Rung;
use crate::desktop::{DesktopDelta, MediaState};
use crate::ids::{ConnectorId, ToolName, Untrusted};
use crate::schema::Registry;
use crate::tools::ToolSpec;

/// What a connector serves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectorManifest {
    /// Its id.
    pub id: ConnectorId,
    /// Its version.
    pub version: String,
    /// Where it runs.
    pub runs: RunsIn,
    /// The operating systems it runs on.
    pub platforms: Vec<Platform>,
    /// Its tools.
    pub tools: Vec<ToolSpec>,
    /// The event kinds it publishes.
    pub events: Vec<String>,
    /// The (tool, op) pairs recipes may use as checks; each op is read-only and tier 0.
    pub checks: Vec<(ToolName, String)>,
    /// Whether it senses the room (camera, room microphone): opt in per device, local only.
    pub senses_room: bool,
    /// Whether it needs the user to opt in.
    pub requires_opt_in: bool,
}

/// Where a connector runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunsIn {
    /// A task inside the core.
    CoreTask,
    /// A supervised child process.
    Sidecar {
        /// The executable's name.
        binary: String,
    },
}

/// An operating system.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// Windows.
    Windows,
    /// Linux.
    Linux,
    /// macOS.
    MacOs,
}

/// What an app adapter can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppCapability {
    /// The app.
    pub app: String,
    /// What it can do.
    pub verbs: Vec<String>,
    /// The app's own version.
    pub version: Option<String>,
    /// The routes it offers, best first.
    pub rungs: Vec<Rung>,
}

/// A supervised child process.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SidecarId {
    /// `eileen-speech.exe`.
    Speech,
    /// `eileen-desktop.exe`.
    Desktop,
    /// `eileen-apps.exe`.
    Apps,
    /// `eileen-hud.exe`.
    Hud,
}

/// From a sidecar's client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SidecarEvent {
    /// It started and answered `connector_manifest`.
    Ready {
        /// Its manifest.
        manifest: Box<ConnectorManifest>,
    },
    /// It is alive.
    Heartbeat,
    /// One of its events.
    Event(ConnectorEvent),
    /// It exited.
    Exited {
        /// Its exit code.
        code: Option<i32>,
    },
}

/// To a sidecar's client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SidecarCmd {
    /// Start it.
    Start,
    /// Restart it.
    Restart,
    /// Write a hang dump, then kill it.
    HangDumpThenKill,
}

/// An event a connector publishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorEvent {
    /// The desktop changed.
    Desktop(DesktopDelta),
    /// A media session changed.
    Media(MediaState),
    /// An app's own event.
    App {
        /// The app.
        app: String,
        /// The event.
        event: AppEvent,
    },
}

/// An app's own event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AppEvent {
    /// A render started.
    RenderStarted {
        /// The render job.
        render_job: String,
    },
    /// A render made progress.
    RenderProgress {
        /// The render job.
        render_job: String,
        /// Percent.
        pct: u8,
    },
    /// A render finished.
    RenderDone {
        /// The render job.
        render_job: String,
        /// Whether it succeeded.
        ok: bool,
    },
    /// The app's version changed.
    VersionChanged {
        /// The new version.
        version: String,
    },
    /// Anything else the adapter reports.
    Other(Untrusted<Value>),
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<ConnectorManifest>();
    r.add::<RunsIn>();
    r.add::<Platform>();
    r.add::<AppCapability>();
    r.add::<SidecarId>();
    r.add::<SidecarEvent>();
    r.add::<SidecarCmd>();
    r.add::<ConnectorEvent>();
    r.add::<AppEvent>();
}
