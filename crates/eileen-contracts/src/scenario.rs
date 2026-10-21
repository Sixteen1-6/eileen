//! Scenarios (BUILD.md section 3.3.14): the format packages, contributors and CI all write and
//! read, in `scenarios/<area>/<id>/scenario.toml`.
//!
//! Provisional: WP2 pins this module in its first pull request (BUILD.md section 3.3).
//!
//! A file is valid when it deserializes into [`Scenario`] (unknown fields are refused) and
//! matches the JSON Schema generated from it into `scenarios/_schema/scenario.schema.json`.
//! [`status_problems`] is the one expiry rule, shared by `cargo xtask scenarios check` and
//! `eileen scenario check`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::connectors::SidecarId;
use crate::desktop::Selector;
use crate::hud::HudInput;
use crate::ids::{Date, MachineHash, ScenarioId, ToolName};
use crate::jobs::JobStatus;
use crate::schema::Registry;

/// The format version this module reads and writes.
pub const SCHEMA_VERSION: u16 = 1;

/// The longest a `Stale` status may last, days.
pub const STALE_CAP_DAYS: u16 = 30;

/// The longest a quarantine may last, days.
pub const QUARANTINE_CAP_DAYS: u16 = 90;

/// One scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// The format version, 1.
    pub schema: u16,
    /// Its folder's slug.
    pub id: ScenarioId,
    /// A title.
    pub title: String,
    /// Its area, for example `apps/resolve`.
    pub area: String,
    /// Tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Where it came from.
    pub origin: ScenarioOrigin,
    /// The contributor's GitHub handle, never an e-mail address.
    pub contributor: Option<String>,
    /// The machine that recorded it (owner answer 6): set by `eileen scenario save` from
    /// `fleet\machine.toml` on every saved scenario and kept by share; `None` only on
    /// hand-written scenarios.
    pub machine: Option<MachineHash>,
    /// The commit it was recorded at.
    pub recorded_at_commit: Option<String>,
    /// Whether it should pass.
    pub outcome: ExpectedOutcome,
    /// What the user says.
    pub intent: ScenarioIntent,
    /// What it needs.
    pub environment: ScenarioEnv,
    /// For hand-written sim scenarios: the simulated world.
    pub world: Option<SimWorld>,
    /// Files, apps and windows to set up.
    #[serde(default)]
    pub fixtures: Fixtures,
    /// What happens.
    #[serde(default)]
    pub steps: Vec<Step>,
    /// What must be true.
    #[serde(default)]
    pub expect: Vec<Expectation>,
    /// Timing budgets, `"turn.ended -> job.completed" = { max_ms, ci_factor }`.
    #[serde(default)]
    pub timing: BTreeMap<String, TimingBudget>,
    /// The sim-lane replay files.
    pub sim: Option<SimReplay>,
    /// Required for a known-flaky scenario, at most 90 days.
    pub quarantine: Option<Quarantine>,
    /// Active, or stale for at most 30 days.
    #[serde(default)]
    pub status: ScenarioStatus,
}

impl Scenario {
    /// The part the expiry rule reads.
    #[must_use]
    pub fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: self.id.clone(),
            status: self.status.clone(),
            quarantine: self.quarantine.clone(),
        }
    }
}

/// Where a scenario came from. Contributed scenarios are MIT like the code; consented audio is
/// CC-BY-4.0 and enters only with the speaker's consent record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ScenarioOrigin {
    /// Written by hand.
    Synthetic,
    /// Saved from real use and generalised by `eileen scenario share`.
    SyntheticFromUser,
    /// Saved from real use with the speaker's own audio and consent record.
    ContributedConsentedAudio,
    /// Written from a learned skill's evidence.
    FromSkill,
}

/// Whether a scenario should pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedOutcome {
    /// It passes.
    Pass,
    /// A saved failure, until fixed.
    ExpectedFail,
}

/// What the user says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScenarioIntent {
    /// The utterances; placeholders allowed.
    pub utterances: Vec<String>,
    /// The speaker's own audio, only with its consent record.
    pub audio: Option<ConsentedAudio>,
    /// The stock voice runners use when they need audio, for example `kokoro:af_heart`; the
    /// text is the source of truth.
    pub voice: Option<String>,
    /// The user's "what should have happened".
    pub expected: Option<String>,
}

/// Consented audio in a scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsentedAudio {
    /// The clip, inside the scenario folder.
    pub file: String,
    /// The consent record, inside the scenario folder.
    pub consent: String,
}

/// What a scenario needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScenarioEnv {
    /// The oldest Windows build it runs on.
    pub min_windows_build: Option<u32>,
    /// Capabilities, for example `resolve-studio>=21.0`, `monitors>=2`, `real-claude`.
    #[serde(default)]
    pub requires: Vec<String>,
    /// The lanes it runs in.
    pub lanes: Vec<Lane>,
}

/// A lane a scenario runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// The simulator, on every pull request.
    Sim,
    /// A real desktop: the owner's PC and fleet PCs.
    Desktop,
    /// A timed benchmark.
    Bench,
}

/// The simulated world of a hand-written sim scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SimWorld {
    /// The brain's stub script.
    pub brain_script: Option<String>,
    /// Worker stub scripts, by fan-out index.
    #[serde(default)]
    pub worker_scripts: Vec<String>,
    /// Sidecar latency and faults.
    #[serde(default)]
    pub sidecars: BTreeMap<SidecarId, SimSidecar>,
    /// The sim profile.
    pub profile: Option<String>,
    /// Memory pressure.
    pub memory: Option<SimMemory>,
}

/// One simulated sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SimSidecar {
    /// Its reply latency, ms.
    pub latency_ms: u32,
    /// Faults to inject, by name.
    #[serde(default)]
    pub faults: Vec<String>,
}

/// Simulated memory pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SimMemory {
    /// Available memory, MB.
    pub available_mb: u32,
    /// Commit headroom, MB.
    pub commit_headroom_mb: u32,
    /// The paging rate, KB/s.
    pub paging_kb_s: u32,
}

/// What to set up. Contributed scenarios may use only fixture templates already in the
/// repository; they carry no binary files of their own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fixtures {
    /// Files made from templates in the area's `_fixtures/`.
    #[serde(default)]
    pub files: Vec<FixtureFile>,
    /// Apps to launch.
    #[serde(default)]
    pub apps: Vec<FixtureApp>,
    /// Where windows start.
    #[serde(default)]
    pub layout: Vec<FixtureWindow>,
}

/// A file made from a template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FixtureFile {
    /// The template, under the area's `_fixtures/`.
    pub template: String,
    /// Where it goes, under the run's folder.
    pub to: String,
}

/// An app to launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FixtureApp {
    /// The app.
    pub launch: String,
    /// A project template to open, under the area's `_fixtures/`.
    pub project_template: Option<String>,
}

/// Where a window starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FixtureWindow {
    /// The window's process.
    pub process: String,
    /// The monitor's ordinal.
    pub monitor: u8,
    /// A snap zone.
    pub zone: Option<String>,
}

/// One step of a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Say an utterance.
    Say {
        /// Its index into `intent.utterances`.
        utterance: u16,
    },
    /// Type text.
    Type {
        /// The text.
        text: String,
    },
    /// Press a hotkey.
    Hotkey {
        /// The chord.
        keys: String,
    },
    /// Click on the HUD.
    Hud {
        /// What is clicked.
        input: HudInput,
    },
    /// Wait for an event.
    WaitFor {
        /// A name from `docs/reference/events.md`.
        event: String,
        /// How long, ms.
        timeout_ms: u32,
    },
    /// Approve the pending question.
    Approve,
    /// Deny the pending question.
    Deny,
    /// Wait.
    Pause {
        /// How long, ms.
        ms: u32,
    },
    /// The user touches the mouse.
    PhysicalInput,
}

/// What must be true after a scenario. Sim-lane expectations are the ones the simulator can
/// check (events, calls, spoken text, job status, settings, files in a simulated store);
/// desktop-lane ones need a real desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expectation {
    /// Matched against the event trace with `[ANY]`, `[ANYLINE]` and `[UNORDERED]`.
    Event {
        /// The pattern.
        pattern: String,
    },
    /// A tool call whose arguments contain these.
    Call {
        /// The tool.
        tool: ToolName,
        /// Arguments the call must contain.
        args_contain: Value,
    },
    /// Eileen said something containing this.
    Spoken {
        /// The text.
        contains: String,
    },
    /// Eileen said nothing.
    NotSpoken,
    /// A job ended in this status.
    JobStatus {
        /// The status.
        status: JobStatus,
    },
    /// A window is on a monitor.
    WindowOnMonitor {
        /// The window's process.
        process: String,
        /// The monitor's ordinal.
        ordinal: u8,
    },
    /// A window is snapped.
    WindowSnapped {
        /// The window's process.
        process: String,
        /// The zone.
        zone: String,
    },
    /// A field holds text.
    FieldText {
        /// The field.
        selector: Selector,
        /// The text.
        contains: String,
    },
    /// A file exists.
    File {
        /// Its path, under the run's folder.
        path: String,
        /// Its SHA-256.
        sha256: Option<String>,
        /// Text it contains.
        contains: Option<String>,
    },
    /// Resolve's playhead is at a timecode.
    ResolvePlayhead {
        /// The timecode.
        timecode: String,
        /// Frames of tolerance.
        tolerance_frames: u32,
    },
    /// A media session is at a position.
    MediaPosition {
        /// Seconds.
        seconds: f64,
        /// Tolerance, seconds.
        tolerance_s: f64,
    },
    /// Sharpness went up.
    SharpnessIncrease {
        /// At least this much, percent.
        min_pct: u16,
    },
    /// A setting has a value.
    SettingEquals {
        /// The key.
        key: String,
        /// The value.
        value: Value,
    },
    /// No file matches (for deletion checks: first run declined, forget).
    NoFileLeft {
        /// The pattern.
        glob: String,
    },
}

/// A timing budget between two events.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimingBudget {
    /// The budget on the owner's PC, ms.
    pub max_ms: u32,
    /// How much slower CI may be.
    pub ci_factor: f64,
}

/// The sim-lane replay files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SimReplay {
    /// `replay/inputs.jsonl`: plain JSON lines of `RecordLine`, size-capped.
    pub inputs: String,
    /// `replay/expected.trace`.
    pub expected: String,
}

/// A known-flaky scenario's quarantine: at most 90 days, with an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Quarantine {
    /// The issue's link.
    pub issue: String,
    /// When it started, written when it is set.
    pub since: Date,
    /// When it ends.
    pub expires: Date,
}

/// A scenario's status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ScenarioStatus {
    /// Runs normally.
    #[default]
    Active,
    /// No longer replays after a change, for at most 30 days; an expired stale scenario fails
    /// CI, like an expired quarantine.
    Stale {
        /// The commit that made it stale.
        since_commit: String,
        /// When it was marked, written by `eileen scenario stale`.
        since: Date,
        /// The issue's link.
        issue: String,
        /// When it expires.
        expires: Date,
    },
}

/// The part of a scenario the expiry rule reads; both callers build it from the parsed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioMeta {
    /// The scenario.
    pub id: ScenarioId,
    /// Its status.
    pub status: ScenarioStatus,
    /// Its quarantine.
    pub quarantine: Option<Quarantine>,
}

/// What the expiry rule found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusProblem {
    /// The status or quarantine has passed its expiry.
    Expired {
        /// Which.
        what: StatusKind,
        /// When it expired.
        expires: Date,
    },
    /// It names an expiry further from its `since` than its cap.
    TooLong {
        /// Which.
        what: StatusKind,
        /// Days from `since` to `expires`.
        days: u16,
        /// The cap.
        cap: u16,
    },
    /// Its `since` is after today.
    SinceInFuture {
        /// Which.
        what: StatusKind,
        /// The date.
        since: Date,
    },
    /// It has no issue link.
    NoIssue {
        /// Which.
        what: StatusKind,
    },
}

/// Which of a scenario's limits a problem is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    /// The `Stale` status.
    Stale,
    /// The quarantine.
    Quarantine,
}

/// The one expiry rule, pure, called by both `cargo xtask scenarios check` (check 18,
/// `tools/repo-tools`) and `eileen scenario check` (WP2) with today's UTC date: a problem when
/// a `Stale` status or a quarantine has passed its `expires`, names an `expires` more than 30
/// days (stale) or 90 days (quarantine) after its own `since`, has a `since` after today, or has
/// no issue link. The comparison of `since` with the commit that introduced it is repo-tools'
/// own (it needs `git blame`).
#[must_use]
pub fn status_problems(meta: &ScenarioMeta, today: Date) -> Vec<StatusProblem> {
    let mut problems = Vec::new();
    if let ScenarioStatus::Stale { since, issue, expires, .. } = &meta.status {
        let limit = Limit { what: StatusKind::Stale, since: *since, expires: *expires, issue };
        limit.check(STALE_CAP_DAYS, today, &mut problems);
    }
    if let Some(quarantine) = &meta.quarantine {
        let limit = Limit {
            what: StatusKind::Quarantine,
            since: quarantine.since,
            expires: quarantine.expires,
            issue: &quarantine.issue,
        };
        limit.check(QUARANTINE_CAP_DAYS, today, &mut problems);
    }
    problems
}

/// One dated limit: a stale status or a quarantine.
struct Limit<'a> {
    what: StatusKind,
    since: Date,
    expires: Date,
    issue: &'a str,
}

impl Limit<'_> {
    /// Adds this limit's problems against `cap` days and today's date.
    fn check(&self, cap: u16, today: Date, problems: &mut Vec<StatusProblem>) {
        let what = self.what;
        if today.days_until(self.expires) < 0 {
            problems.push(StatusProblem::Expired { what, expires: self.expires });
        }
        let length = self.since.days_until(self.expires);
        if length > i64::from(cap) {
            let days = u16::try_from(length).unwrap_or(u16::MAX);
            problems.push(StatusProblem::TooLong { what, days, cap });
        }
        if today.days_until(self.since) > 0 {
            problems.push(StatusProblem::SinceInFuture { what, since: self.since });
        }
        if self.issue.trim().is_empty() {
            problems.push(StatusProblem::NoIssue { what });
        }
    }
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<Scenario>();
    r.add::<ScenarioOrigin>();
    r.add::<ExpectedOutcome>();
    r.add::<ScenarioIntent>();
    r.add::<ConsentedAudio>();
    r.add::<ScenarioEnv>();
    r.add::<Lane>();
    r.add::<SimWorld>();
    r.add::<SimSidecar>();
    r.add::<SimMemory>();
    r.add::<Fixtures>();
    r.add::<FixtureFile>();
    r.add::<FixtureApp>();
    r.add::<FixtureWindow>();
    r.add::<Step>();
    r.add::<Expectation>();
    r.add::<TimingBudget>();
    r.add::<SimReplay>();
    r.add::<Quarantine>();
    r.add::<ScenarioStatus>();
    r.add::<ScenarioMeta>();
    r.add::<StatusProblem>();
    r.add::<StatusKind>();
}

#[cfg(test)]
mod tests;
