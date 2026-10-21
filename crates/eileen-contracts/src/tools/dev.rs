//! The developer toolset (`dev`): read-only and redacted, for coding workers in an Eileen
//! worktree and for "explain that" jobs.
//!
//! Provisional: WP2 pins this module in its first pull request (BUILD.md section 3.3).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{NoArgs, ToolInput, input};
use crate::ids::IncidentId;
use crate::schema::Registry;

const MODULE: &str = "tools/dev";

/// `dev_crate_doc`: one crate's guidance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DevCrateDoc {
    /// The crate, for example `eileen-core`.
    #[serde(rename = "crate")]
    pub krate: String,
}

/// `dev_event_map`: where an event is produced and consumed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DevEventMap {
    /// The dotted event name.
    pub event: String,
}

/// `dev_trace_get`: one trace, at most 400 lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DevTraceGet {
    /// A turn, job or call id.
    pub id: String,
    /// At most this many lines.
    pub max_lines: Option<u16>,
}

/// `dev_logs_query`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DevLogsQuery {
    /// The filter, in the logs' own syntax.
    pub filter: String,
    /// How far back, minutes.
    pub since_minutes: Option<u32>,
    /// At most this many lines.
    pub limit: Option<u16>,
}

/// `dev_incident_read`: one part of an incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DevIncidentRead {
    /// The incident.
    pub id: IncidentId,
    /// Which part.
    pub part: IncidentPart,
}

/// The parts of an incident bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncidentPart {
    /// Its manifest.
    Manifest,
    /// Its log lines.
    Logs,
    /// Its trace.
    Trace,
    /// Its recorded inputs.
    Inputs,
}

/// `dev_replay_core`: replay a recording or incident at the core level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DevReplayCore {
    /// A recording or incident id.
    pub id: String,
}

pub(super) fn inputs(out: &mut BTreeMap<&'static str, ToolInput>) {
    input::<NoArgs>(out, MODULE, "dev_codemap");
    input::<DevCrateDoc>(out, MODULE, "dev_crate_doc");
    input::<DevEventMap>(out, MODULE, "dev_event_map");
    input::<DevTraceGet>(out, MODULE, "dev_trace_get");
    input::<DevLogsQuery>(out, MODULE, "dev_logs_query");
    input::<NoArgs>(out, MODULE, "dev_incident_list");
    input::<DevIncidentRead>(out, MODULE, "dev_incident_read");
    input::<DevReplayCore>(out, MODULE, "dev_replay_core");
    input::<NoArgs>(out, MODULE, "dev_scenario_list");
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<DevCrateDoc>();
    r.add::<DevEventMap>();
    r.add::<DevTraceGet>();
    r.add::<DevLogsQuery>();
    r.add::<DevIncidentRead>();
    r.add::<IncidentPart>();
    r.add::<DevReplayCore>();
}
