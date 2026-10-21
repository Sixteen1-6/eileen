//! Fleet formats (BUILD.md section 3.3.16): one JSON line per scenario run on a contributor's
//! PC, in `fleet\results\<date>.jsonl`, and the run that groups them.
//!
//! Provisional: WP9 pins this module in its first pull request (BUILD.md section 3.3).
//!
//! A result carries the machine hash and anonymous measurements only: no name, path or
//! account. `devhub.yml` validates every arrival with the `fleet_result` parser.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::connectors::Platform;
use crate::ids::{MachineHash, ScenarioId, WallTime};
use crate::learn::Level;
use crate::schema::Registry;

/// One scenario run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FleetResult {
    /// The machine.
    pub machine: MachineHash,
    /// The run.
    pub run: String,
    /// The scenario.
    pub scenario: ScenarioId,
    /// The commit the scenario was taken from.
    pub scenario_commit: String,
    /// Recorded (no model) or live (real Claude, within a turn budget).
    pub mode: FleetMode,
    /// Whether every expectation passed.
    pub passed: bool,
    /// Each expectation and whether it passed.
    pub expectations: Vec<(String, bool)>,
    /// Phase timings, ms.
    pub phases_ms: BTreeMap<String, u32>,
    /// The operating system, so a result from any OS reads alike.
    pub os: Platform,
    /// The Windows build number in v0.1; the distribution and kernel on Linux later.
    pub os_build: String,
    /// Monitors.
    pub monitors: u8,
    /// Each monitor's DPI.
    pub dpi: Vec<u16>,
    /// The versions of the apps it used.
    pub app_versions: BTreeMap<String, String>,
    /// The locale.
    pub locale: String,
    /// The CPU class.
    pub cpu_class: String,
    /// RAM, GB.
    pub ram_gb: u8,
    /// Free RAM at the start, MB.
    pub free_ram_mb: u32,
    /// Whether the user became active during the run.
    pub user_became_active: bool,
    /// Anonymous learning statistics.
    pub learning: Option<LearningStats>,
}

/// How a fleet run serves model responses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FleetMode {
    /// Recorded responses, no model calls.
    #[default]
    Recorded,
    /// The contributor's own Claude plan, within a turn budget.
    Live,
}

/// Anonymous learning statistics: no names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LearningStats {
    /// Skills per level.
    pub skills_per_level: BTreeMap<Level, u32>,
    /// Replays that handed off, percent.
    pub handoff_pct: u8,
    /// Median model turns by run index.
    pub median_turns_by_run: Vec<u32>,
}

/// One fleet run's results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FleetRun {
    /// The machine.
    pub machine: MachineHash,
    /// The version of Eileen.
    pub eileen_version: String,
    /// When it started.
    pub started: WallTime,
    /// The results.
    pub results: Vec<FleetResult>,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<FleetResult>();
    r.add::<FleetMode>();
    r.add::<LearningStats>();
    r.add::<FleetRun>();
}
