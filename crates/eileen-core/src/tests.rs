use eileen_contracts::config::{Config, Proposal, SettingChange, SettingSpecView, ValidationError};
use eileen_contracts::connectors::{AppCapability, ConnectorManifest};
use eileen_contracts::core_io::{Input, TimerId};
use eileen_contracts::desktop::FieldCategory;
use eileen_contracts::ids::{Envelope, MonoTime, SkillId, TraceCtx, WallTime};
use eileen_contracts::intent::{Capabilities, FollowUp, Resolution, ResolveCtx, SlotValues};
use eileen_contracts::learn::{
    CheckPlan, Outcome, PhrasingFact, Recipe, ReplayRefusal, ReplayState, ReplayStep, RouteStat,
    ShadowVerdict, SkillMeta, SkillRef, StepResult, TrajectoryStep, Transition, UseRecord,
    VocabularyEntry,
};
use eileen_contracts::ports::{
    Cleaned, DictationCleaner, IntentResolver, SettingsValidator, SkillEngine,
};
use eileen_contracts::tools::ToolSpec;
use eileen_stdx::rng::CoreRng;

use crate::{Core, CoreConfig, CorePorts, fresh_snapshot};

/// Every port at once; the stub core calls none of them.
struct NoPort;

const UNCALLED: &str = "the stub core calls no port";

impl IntentResolver for NoPort {
    fn rebuild(&mut self, _: &[SkillMeta], _: &[PhrasingFact]) {
        panic!("{UNCALLED}");
    }
    fn resolve(&self, _: &str, _: &ResolveCtx) -> Resolution {
        panic!("{UNCALLED}");
    }
    fn resolve_semantic(&self, _: &str, _: &[f32], _: &ResolveCtx) -> Resolution {
        panic!("{UNCALLED}");
    }
    fn follow_up(&self, _: &str, _: &ResolveCtx) -> FollowUp {
        panic!("{UNCALLED}");
    }
    fn introspect(
        &self,
        _: &[ToolSpec],
        _: &[AppCapability],
        _: &[RouteStat],
        _: Option<&str>,
    ) -> Capabilities {
        panic!("{UNCALLED}");
    }
}

impl DictationCleaner for NoPort {
    fn clean(&self, _: &str, _: FieldCategory, _: &[VocabularyEntry]) -> Cleaned {
        panic!("{UNCALLED}");
    }
}

impl SettingsValidator for NoPort {
    fn validate(&self, _: &Config, _: &[SettingChange]) -> Result<Proposal, ValidationError> {
        panic!("{UNCALLED}");
    }
    fn specs(&self) -> &[SettingSpecView] {
        panic!("{UNCALLED}");
    }
}

impl SkillEngine for NoPort {
    fn rebuild(&mut self, _: &[SkillMeta], _: &[Recipe], _: &[ConnectorManifest], _: &[RouteStat]) {
        panic!("{UNCALLED}");
    }
    fn route_line(&self, _: &str, _: &AppCapability) -> Option<String> {
        panic!("{UNCALLED}");
    }
    fn next_level(&self, _: &SkillId, _: &[UseRecord]) -> Transition {
        panic!("{UNCALLED}");
    }
    fn success(&self, _: &Outcome) -> bool {
        panic!("{UNCALLED}");
    }
    fn start_replay(&self, _: &SkillRef, _: &SlotValues) -> Result<ReplayState, ReplayRefusal> {
        panic!("{UNCALLED}");
    }
    fn next(&self, _: &mut ReplayState, _: Option<&StepResult>) -> ReplayStep {
        panic!("{UNCALLED}");
    }
    fn shadow_checks(&self, _: &SkillRef, _: &SlotValues, _: &[TrajectoryStep]) -> Vec<CheckPlan> {
        panic!("{UNCALLED}");
    }
    fn shadow_agree(&self, _: &SkillRef, _: &SlotValues, _: &[TrajectoryStep]) -> ShadowVerdict {
        panic!("{UNCALLED}");
    }
}

fn ports() -> CorePorts {
    CorePorts {
        resolver: Box::new(NoPort),
        cleaner: Box::new(NoPort),
        settings: Box::new(NoPort),
        skills: Box::new(NoPort),
    }
}

fn timer(seq: u64) -> Envelope<Input> {
    Envelope {
        seq,
        at: WallTime(0),
        mono: MonoTime(0),
        ctx: TraceCtx::default(),
        body: Input::Timer(TimerId(seq)),
    }
}

#[test]
fn a_stub_core_takes_any_input_and_emits_nothing() {
    let mut core = Core::new(CoreConfig::default(), ports(), None);
    let mut rng = CoreRng::from_seed(7);
    let mut effects = Vec::new();
    for seq in 1..=3 {
        core.step(MonoTime(seq), &mut rng, timer(seq), &mut effects);
    }
    assert!(effects.is_empty(), "the stub emits no effect");
    assert_eq!(core.snapshot(), fresh_snapshot(), "a new core reports a fresh snapshot");
}

#[test]
fn a_stub_core_resumes_from_the_snapshot_it_is_given() {
    let mut saved = fresh_snapshot();
    saved.muted = true;
    saved.settings_version = 4;
    let core = Core::new(CoreConfig::default(), ports(), Some(saved.clone()));
    assert_eq!(core.snapshot(), saved, "the stub keeps the snapshot it resumed from");
    assert_eq!(saved.v, 1, "snapshots are version 1");
}
