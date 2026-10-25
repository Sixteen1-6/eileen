//! Ports (BUILD.md section 3.3.15): the interfaces that let `eileen-core` stay free of every
//! other crate, and let the runtime compose components it does not know. They use only std
//! types, so this crate needs no async runtime.
//!
//! Each component crate exposes one constructor, for example
//! `eileen_harness::component(cfg: &HarnessConfig) -> Box<dyn Component>`.

use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::Poll;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{Config, Proposal, SettingChange, SettingSpecView, ValidationError};
use crate::connectors::{AppCapability, ConnectorManifest};
use crate::core_io::{Effect, EffectFamily, Input};
use crate::desktop::FieldCategory;
use crate::ids::{Envelope, Personal, SessionId, SkillId};
use crate::intent::{Capabilities, FollowUp, Resolution, ResolveCtx, SlotValues};
use crate::learn::{
    CheckPlan, Outcome, PhrasingFact, Recipe, ReplayRefusal, ReplayState, ReplayStep, RouteStat,
    ShadowVerdict, SkillMeta, SkillRef, StepResult, TrajectoryStep, Transition, UseRecord,
    VocabularyEntry,
};
use crate::schema::Registry;
use crate::tools::ToolSpec;

/// Implemented by `eileen_intent::Resolver`. Pure and deterministic; called inside
/// `Core::step`.
pub trait IntentResolver: Send {
    /// Rebuilds the indexes from the current skills and phrasings.
    fn rebuild(&mut self, skills: &[SkillMeta], phrasings: &[PhrasingFact]);
    /// L1 and the lexical part of L2.
    fn resolve(&self, utterance: &str, ctx: &ResolveCtx) -> Resolution;
    /// L2 with an embedding the core requested from the speech process.
    fn resolve_semantic(&self, utterance: &str, embedding: &[f32], ctx: &ResolveCtx) -> Resolution;
    /// The conversation window's follow-up feature (section 2.8 rule 3), from `ctx.last_turn`;
    /// `FollowUp::None` without it.
    fn follow_up(&self, utterance: &str, ctx: &ResolveCtx) -> FollowUp;
    /// What Eileen can do here, each app's routes ranked.
    fn introspect(
        &self,
        tools: &[ToolSpec],
        app_caps: &[AppCapability],
        routes: &[RouteStat],
        focused: Option<&str>,
    ) -> Capabilities;
}

/// Implemented by `eileen_dictation::Cleaner`. Pure.
pub trait DictationCleaner: Send {
    /// Cleans a raw transcript for a field of this category.
    fn clean(&self, raw: &str, category: FieldCategory, vocabulary: &[VocabularyEntry]) -> Cleaned;
}

/// A cleaned dictation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Cleaned {
    /// The text to insert.
    pub text: Personal<String>,
    /// Fillers removed.
    pub fillers_removed: u16,
    /// Self-corrections applied ("no, Tuesday").
    pub corrections: u16,
}

/// Implemented by `eileen_config::Validator`. Pure: validation, diff and safety bounds, no file
/// IO.
pub trait SettingsValidator: Send {
    /// Validates changes against the current configuration.
    fn validate(
        &self,
        current: &Config,
        changes: &[SettingChange],
    ) -> Result<Proposal, ValidationError>;
    /// Every setting's spec.
    fn specs(&self) -> &[SettingSpecView];
}

/// Implemented by `eileen_skills::Engine`. Pure; the core drives replay and the ladder through
/// it.
pub trait SkillEngine: Send {
    /// Rebuilds from the current skills, recipes, manifests and route statistics.
    fn rebuild(
        &mut self,
        skills: &[SkillMeta],
        recipes: &[Recipe],
        manifests: &[ConnectorManifest],
        routes: &[RouteStat],
    );
    /// The one-line route summary for an app that goes into a worker's facts block
    /// (`eileen_skills::routes::facts_line`), for example "Resolve: scripting worked 14 of 14
    /// times for navigation; use `apps_resolve` before the screen".
    fn route_line(&self, app: &str, caps: &AppCapability) -> Option<String>;
    /// The ladder's decision for a skill.
    fn next_level(&self, skill: &SkillId, uses: &[UseRecord]) -> Transition;
    /// Whether an outcome counts as a success.
    fn success(&self, outcome: &Outcome) -> bool;
    /// Starts a replay, or says why it cannot.
    fn start_replay(
        &self,
        skill: &SkillRef,
        slots: &SlotValues,
    ) -> Result<ReplayState, ReplayRefusal>;
    /// Feeds the last step's result (or `None` to begin) and gets the next thing to do; a failed
    /// postcondition with a fallback yields the fallback's steps, then the re-run.
    fn next(&self, state: &mut ReplayState, last: Option<&StepResult>) -> ReplayStep;
    /// After each Eileen tool result in a shadowed job: the check ops the core must issue now
    /// with origin `Core`, because the newest step was just matched to a recipe step whose
    /// postcondition names one (section 2.10 item 7).
    fn shadow_checks(
        &self,
        skill: &SkillRef,
        slots: &SlotValues,
        steps: &[TrajectoryStep],
    ) -> Vec<CheckPlan>;
    /// Whether a shadowed job agreed with the recipe.
    fn shadow_agree(
        &self,
        skill: &SkillRef,
        slots: &SlotValues,
        steps: &[TrajectoryStep],
    ) -> ShadowVerdict;
}

/// A component the runtime starts: audio, speech client, harness, sidecar clients, learning,
/// HUD client, shell.
pub trait Component: Send + 'static {
    /// Its name, for logs.
    fn name(&self) -> &'static str;
    /// Which effect families this component consumes (the runtime routes accordingly).
    fn accepts(&self) -> &'static [EffectFamily];
    /// Runs the component until shutdown.
    fn start(
        self: Box<Self>,
        ctx: ComponentCtx,
    ) -> Pin<Box<dyn Future<Output = Result<(), ComponentError>> + Send>>;
}

/// What the runtime gives a component when it starts.
pub struct ComponentCtx {
    /// Bounded; returns `Backpressure` instead of growing.
    pub inputs: Box<dyn InputSink>,
    /// The effects routed to it.
    pub effects: Box<dyn EffectSource>,
    /// The data directory.
    pub data_dir: PathBuf,
    /// The core session.
    pub session: SessionId,
    /// Resolves when the runtime shuts down.
    pub shutdown: Box<dyn ShutdownSignal>,
}

impl fmt::Debug for ComponentCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentCtx")
            .field("data_dir", &self.data_dir)
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

/// Where a component sends inputs to the core.
pub trait InputSink: Send + Sync {
    /// Sends one input, or refuses when the queue is full.
    fn send(&self, input: Envelope<Input>) -> Result<(), Backpressure>;
}

/// Where a component receives its effects.
pub trait EffectSource: Send {
    /// The next effect; `None` once the runtime has shut the source.
    fn recv(&mut self) -> Pin<Box<dyn Future<Output = Option<Envelope<Effect>>> + Send + '_>>;
}

/// Resolves when the runtime shuts down.
pub trait ShutdownSignal: Send + Sync {
    /// Waits for shutdown.
    fn wait(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// The input queue is full; the input was not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backpressure {
    /// The queue's capacity.
    pub capacity: usize,
}

impl fmt::Display for Backpressure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the core's input queue is full ({} entries)", self.capacity)
    }
}

impl std::error::Error for Backpressure {}

/// A component stopped with an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentError {
    /// The component.
    pub component: &'static str,
    /// What went wrong, in one sentence.
    pub message: String,
}

impl fmt::Display for ComponentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.component, self.message)
    }
}

impl std::error::Error for ComponentError {}

/// A component that takes the effects routed to it, drops them and does nothing else, until
/// the runtime shuts down or closes its effect source. WP0's stub crates return it from their
/// `component` constructors, so `eileen-cli` wires every component from the first day, and a
/// test can stand it in for a component it does not exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleComponent {
    name: &'static str,
    accepts: &'static [EffectFamily],
}

impl IdleComponent {
    /// An idle component named `name` that accepts the effect families in `accepts`.
    #[must_use]
    pub const fn new(name: &'static str, accepts: &'static [EffectFamily]) -> Self {
        Self { name, accepts }
    }
}

impl Component for IdleComponent {
    fn name(&self) -> &'static str {
        self.name
    }

    fn accepts(&self) -> &'static [EffectFamily] {
        self.accepts
    }

    fn start(
        self: Box<Self>,
        ctx: ComponentCtx,
    ) -> Pin<Box<dyn Future<Output = Result<(), ComponentError>> + Send>> {
        Box::pin(async move {
            let ComponentCtx { mut effects, shutdown, .. } = ctx;
            let mut stop = shutdown.wait();
            loop {
                let mut next = effects.recv();
                // True when an effect arrived (it is dropped here); false once the source is
                // closed or the runtime is shutting down.
                let took_one = std::future::poll_fn(|cx| {
                    if stop.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(false);
                    }
                    next.as_mut().poll(cx).map(|effect| effect.is_some())
                })
                .await;
                if !took_one {
                    return Ok(());
                }
            }
        })
    }
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<Cleaned>();
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::{Future, pending, ready};
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    use super::{
        Backpressure, Component, ComponentCtx, EffectSource, IdleComponent, InputSink,
        ShutdownSignal,
    };
    use crate::core_io::{Effect, EffectFamily, Input, TimerId};
    use crate::ids::{Envelope, MonoTime, SessionId, TraceCtx, WallTime};

    struct Refuse;

    impl InputSink for Refuse {
        fn send(&self, _input: Envelope<Input>) -> Result<(), Backpressure> {
            Err(Backpressure { capacity: 0 })
        }
    }

    /// Hands out its queued effects, then reports the source closed.
    struct Queue(VecDeque<Envelope<Effect>>);

    impl EffectSource for Queue {
        fn recv(
            &mut self,
        ) -> Pin<Box<dyn Future<Output = Option<Envelope<Effect>>> + Send + '_>> {
            Box::pin(ready(self.0.pop_front()))
        }
    }

    /// Never hands out an effect and never closes.
    struct Silent;

    impl EffectSource for Silent {
        fn recv(
            &mut self,
        ) -> Pin<Box<dyn Future<Output = Option<Envelope<Effect>>> + Send + '_>> {
            Box::pin(pending())
        }
    }

    struct Shutdown(bool);

    impl ShutdownSignal for Shutdown {
        fn wait(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            if self.0 { Box::pin(ready(())) } else { Box::pin(pending()) }
        }
    }

    fn timer(id: u64) -> Envelope<Effect> {
        Envelope {
            seq: id,
            at: WallTime(0),
            mono: MonoTime(0),
            ctx: TraceCtx::default(),
            body: Effect::CancelTimer(TimerId(id)),
        }
    }

    fn ctx(effects: Box<dyn EffectSource>, shut_down: bool) -> ComponentCtx {
        ComponentCtx {
            inputs: Box::new(Refuse),
            effects,
            data_dir: PathBuf::from("data"),
            session: SessionId::from_ulid(ulid::Ulid::from_parts(1, 1)),
            shutdown: Box::new(Shutdown(shut_down)),
        }
    }

    /// Polls a future that needs no wake-up to finish: every future these tests build is ready
    /// or pending for good, so one poll per step decides it.
    fn finish<F: Future>(future: F) -> Option<F::Output> {
        let mut future = Box::pin(future);
        match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(output) => Some(output),
            Poll::Pending => None,
        }
    }

    #[test]
    fn an_idle_component_drains_its_effects_and_stops_when_the_source_closes() {
        let idle = Box::new(IdleComponent::new("idle", &[EffectFamily::Timer]));
        assert_eq!(idle.name(), "idle", "its name");
        assert_eq!(idle.accepts(), [EffectFamily::Timer], "its families");
        let queue = Queue([timer(1), timer(2)].into_iter().collect());
        let outcome = finish(idle.start(ctx(Box::new(queue), false)));
        assert_eq!(outcome, Some(Ok(())), "it ends cleanly once the source is closed");
    }

    #[test]
    fn an_idle_component_stops_at_shutdown() {
        let idle = Box::new(IdleComponent::new("idle", &[]));
        let outcome = finish(idle.start(ctx(Box::new(Silent), true)));
        assert_eq!(outcome, Some(Ok(())), "it ends cleanly at shutdown");
    }

    #[test]
    fn an_idle_component_waits_while_nothing_happens() {
        let idle = Box::new(IdleComponent::new("idle", &[]));
        let outcome = finish(idle.start(ctx(Box::new(Silent), false)));
        assert_eq!(outcome, None, "no effect and no shutdown: it keeps waiting");
    }
}
