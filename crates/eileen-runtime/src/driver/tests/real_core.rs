//! The real core behind the driver, with ports that fail the test if the core calls them: the
//! core the driver owns gets its own timers back.

use std::time::Duration;

use eileen_contracts::config::{Config, Proposal, SettingChange, SettingSpecView, ValidationError};
use eileen_contracts::connectors::{AppCapability, ConnectorManifest, SidecarCmd, SidecarId};
use eileen_contracts::core_io::{ActivityEvent, Effect, EffectFamily, Input};
use eileen_contracts::desktop::FieldCategory;
use eileen_contracts::ids::SkillId;
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
use eileen_core::{CoreConfig, CorePorts};
use tokio::time::timeout;

use super::{Driver, Limits, Probes, Shutdown, envelope, next_seen, secs, setup};

/// The ports the core does not call while it starts the speech process and waits for it.
struct Uncalled;

const UNCALLED: &str = "the core does not call this port while speech loads";

impl IntentResolver for Uncalled {
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

impl DictationCleaner for Uncalled {
    fn clean(&self, _: &str, _: FieldCategory, _: &[VocabularyEntry]) -> Cleaned {
        panic!("{UNCALLED}");
    }
}

impl SettingsValidator for Uncalled {
    fn validate(&self, _: &Config, _: &[SettingChange]) -> Result<Proposal, ValidationError> {
        panic!("{UNCALLED}");
    }
    fn specs(&self) -> &[SettingSpecView] {
        panic!("{UNCALLED}");
    }
}

impl SkillEngine for Uncalled {
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

#[tokio::test(start_paused = true)]
async fn the_core_it_owns_kills_a_speech_process_that_never_gets_ready() {
    let ports = CorePorts {
        resolver: Box::new(Uncalled),
        cleaner: Box::new(Uncalled),
        settings: Box::new(Uncalled),
        skills: Box::new(Uncalled),
    };
    let driver = Driver::new(CoreConfig::default(), ports, setup(Limits::default()));
    let (probes, mut seen) = Probes::new();
    let (sidecars, sink) = probes.feeder("sidecars", &[EffectFamily::Sidecar]);
    let shutdown = Shutdown::new();
    let run = eileen_rt::task::spawn("test.driver", driver.run(vec![sidecars], shutdown.clone()));
    sink.await.unwrap().send(envelope(Input::Activity(ActivityEvent::PhysicalInput))).unwrap();

    // The core's first step starts the speech process and gives it 120 s to be ready.
    let (_, start) = next_seen(&mut seen).await.expect("the start");
    assert_eq!(start.body, Effect::Sidecar(SidecarId::Speech, SidecarCmd::Start), "started");
    assert_eq!(start.mono, secs(0), "on the first step");
    let (_, kill) = timeout(Duration::from_secs(200), seen.recv()).await.unwrap().unwrap();
    assert_eq!(
        kill.body,
        Effect::Sidecar(SidecarId::Speech, SidecarCmd::HangDumpThenKill),
        "never ready, so dumped and killed"
    );
    assert_eq!(kill.mono, secs(120), "when the core's own timer came back, at 120 s");

    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "the run ends well");
}
