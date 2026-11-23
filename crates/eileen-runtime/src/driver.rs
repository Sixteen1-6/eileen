//! The driver loop (BUILD.md sections 2.1, 3.3.3, 3.3.15 and 6.4): one task owns
//! [`eileen_core::Core`], feeds it every input and carries its effects to the components that
//! claim them.
//!
//! - **Inputs.** Components send through one bounded [`eileen_rt::channel`]; a full queue answers
//!   [`Backpressure`] at once and never grows. A component sends an envelope's `seq` and times as
//!   zero (a non-zero `seq` fails a soft assertion) and fills its `TraceCtx`. The input queue's
//!   receiving end, in the driver, stamps each input's `seq` and its wall and monotonic time from
//!   the [`Clock`] as it hands the input to the core, overwriting what was sent, and steps the
//!   core with that same monotonic time (section 3.3.15). A fired timer takes its stamp the same
//!   way, so `seq` is the order the core sees, counting up by one whatever the source, the
//!   core's time never runs backwards along it, and a recorded input replays with its own time
//!   (section 3.3.16).
//! - **Effects.** Each effect goes, in order, to the component whose [`Component::accepts`]
//!   names its [`EffectFamily`], through that component's own bounded queue, unchanged but for
//!   `Hud`: section 3.3.3 has the driver hold HUD snapshots to 10 a second, so one goes at most
//!   every [`HUD_EVERY`], and since each is the whole HUD state, the newest replaces one still
//!   waiting, which goes once that time has passed. A family two components claim is a start
//!   error, and so is a component that claims `Timer`, which the driver serves itself. An effect
//!   nobody serves (no claim, or its component has ended) is dropped, with one `effect.unserved`
//!   line per family per run. While a component's queue is full the driver waits for room,
//!   still watching for shutdown and for any component's end.
//! - **Timers.** `SetTimer` and `CancelTimer` stay in the driver. A timer fires on tokio's clock
//!   when the monotonic clock reaches its `at`, and comes back to the core as `Input::Timer`,
//!   stepped at once with a time no earlier than `at`. Tokio wakes a timer up to a millisecond
//!   late, and later on Windows, so an input taken in that gap first steps every timer due
//!   strictly before the time the input takes: no input is stepped past a pending timer's `at`,
//!   and a tie goes to the input, as in the simulator. A fired timer never waits in the input
//!   queue, so the driver never blocks on its own channel.
//! - **Components.** Each starts through [`eileen_rt::task::spawn`] with a [`ComponentCtx`]:
//!   the input sink, its effects, the data directory, the core session and the run's
//!   [`Shutdown`]. A component that returns an error or panics ends the run, which reports the
//!   name the component was started under.
//! - **Shutdown.** On [`Shutdown::trigger`], or when a component fails, the driver stops
//!   stepping, triggers the shutdown every component waits on, closes every effect queue and the
//!   input queue, and gives the components [`Limits::stop_within`] to end; it aborts the rest.
//!   A run that ends any other way, by a panic in the core's step or by its caller dropping it,
//!   still triggers the shutdown and aborts every component's task.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::{Future, pending, poll_fn};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use eileen_contracts::core_io::{Effect, EffectFamily, Input, TimerId};
use eileen_contracts::ids::{Envelope, MonoTime, Personal, SessionId, TraceCtx};
use eileen_contracts::ports::{
    Backpressure, Component, ComponentCtx, ComponentError, EffectSource, InputSink, ShutdownSignal,
};
use eileen_contracts::record::CoreSnapshotV1;
use eileen_core::{Core, CoreConfig, CorePorts};
use eileen_rt::channel::{self, Receiver, Sender};
use eileen_stdx::clock::Clock;
use eileen_stdx::rng::CoreRng;
use eileen_trace::names;
use tokio::sync::watch;
use tokio::task::{JoinError, JoinHandle};
use tokio::time::Instant;
use tracing::field::Empty;

mod timing;

use timing::{HudPace, Timers};

/// The name the driver claims `Timer` under, which a component claiming it is told.
pub const DRIVER: &str = "driver";

/// The name of the core's input queue, as `channel.full` reports it.
const INPUT_QUEUE: &str = "core.inputs";

/// How long an aborted component's task has to go once aborted.
const ABORT_WAIT: Duration = Duration::from_secs(1);

/// The least time between two `Hud` snapshots: 10 a second (section 3.3.3).
pub const HUD_EVERY: Duration = Duration::from_millis(100);

/// The bounds of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The core's input queue: inputs past it are refused with [`Backpressure`].
    pub inputs: NonZeroUsize,
    /// Each component's effect queue: when one is full, the driver waits for room, in order.
    pub effects: NonZeroUsize,
    /// How long the components have to end after shutdown before their tasks are aborted.
    pub stop_within: Duration,
}

impl Limits {
    /// The bounds a running Eileen uses.
    pub const DEFAULT: Self = Self {
        inputs: NonZeroUsize::MIN.saturating_add(1_024 - 1),
        effects: NonZeroUsize::MIN.saturating_add(256 - 1),
        stop_within: Duration::from_secs(5),
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What the caller hands the driver beside the core's configuration and ports.
pub struct Setup {
    /// The data directory each component is given.
    pub data_dir: PathBuf,
    /// The core session each component is given.
    pub session: SessionId,
    /// Where each input's wall and monotonic time comes from.
    pub clock: Arc<dyn Clock>,
    /// The core's random numbers.
    pub rng: CoreRng,
    /// The checkpoint the core resumes from; `None` for a fresh core.
    pub snapshot: Option<CoreSnapshotV1>,
    /// The bounds of the run.
    pub limits: Limits,
}

impl fmt::Debug for Setup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Setup")
            .field("data_dir", &self.data_dir)
            .field("session", &self.session)
            .field("resumed", &self.snapshot.is_some())
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

/// Why a run ended other than by [`Shutdown::trigger`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    /// Two components claim one family, or a component claims `Timer` (claimed by [`DRIVER`]).
    /// Nothing was started.
    #[error("{second} claims the {family:?} effects, which {first} already serves")]
    FamilyClaimedTwice {
        /// The family.
        family: EffectFamily,
        /// The one that claims it first.
        first: &'static str,
        /// The one that claims it again.
        second: &'static str,
    },
    /// A component stopped with an error. Its words can quote a path, so they stay out of this
    /// error's message: a caller that logs them logs `message.get()` under `pii.message`, as
    /// the driver's own `component.ended` line does.
    #[error("the {component} component stopped")]
    Component {
        /// The component's name, as it was started.
        component: &'static str,
        /// What went wrong, in its words.
        message: Personal<String>,
    },
    /// A component's task panicked.
    #[error("the {component} component panicked")]
    Panicked {
        /// The component's name, as it was started.
        component: &'static str,
    },
}

/// Asks a run to stop, and tells every component it is stopping: each [`ComponentCtx`] holds a
/// clone as its [`ShutdownSignal`].
#[derive(Debug, Clone)]
pub struct Shutdown(Arc<watch::Sender<bool>>);

impl Shutdown {
    /// A shutdown not yet triggered.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(watch::Sender::new(false)))
    }

    /// Asks the run, and every component, to stop. Triggering it again does nothing more.
    pub fn trigger(&self) {
        self.0.send_replace(true);
    }

    /// Whether it has been triggered.
    #[must_use]
    pub fn is_triggered(&self) -> bool {
        *self.0.borrow()
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.0.subscribe()
    }
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownSignal for Shutdown {
    fn wait(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let mut stop = self.subscribe();
        Box::pin(async move {
            let _stopped = stop.wait_for(|stop| *stop).await;
        })
    }
}

/// What the driver steps: the core with its random numbers, or a stand-in in tests.
trait Machine: Send {
    fn step(&mut self, now: MonoTime, input: Envelope<Input>, effects: &mut Vec<Envelope<Effect>>);
}

/// The core and the random numbers it steps with.
struct OwnedCore {
    core: Core,
    rng: CoreRng,
}

impl Machine for OwnedCore {
    fn step(&mut self, now: MonoTime, input: Envelope<Input>, effects: &mut Vec<Envelope<Effect>>) {
        self.core.step(now, &mut self.rng, input, effects);
    }
}

/// The driver: the core, ready to run with the components the caller wires.
pub struct Driver {
    machine: Box<dyn Machine>,
    data_dir: PathBuf,
    session: SessionId,
    clock: Arc<dyn Clock>,
    limits: Limits,
}

impl fmt::Debug for Driver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Driver")
            .field("data_dir", &self.data_dir)
            .field("session", &self.session)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl Driver {
    /// A driver for a core built from `config` and `ports`, resumed from `setup.snapshot`.
    #[must_use]
    pub fn new(config: CoreConfig, ports: CorePorts, setup: Setup) -> Self {
        let Setup { data_dir, session, clock, rng, snapshot, limits } = setup;
        let core = Core::new(config, ports, snapshot);
        Self { machine: Box::new(OwnedCore { core, rng }), data_dir, session, clock, limits }
    }

    /// Runs the core with `components` until `shutdown` is triggered or a component fails.
    /// Every family is checked before anything starts, and when the run returns every
    /// component's task has ended.
    pub async fn run(
        self,
        components: Vec<Box<dyn Component>>,
        shutdown: Shutdown,
    ) -> Result<(), RunError> {
        let Self { machine, data_dir, session, clock, limits } = self;
        let routes = Routes::claim(&components)?;
        let (sink, inputs) = channel::bounded(INPUT_QUEUE, limits.inputs);
        let mut outlets = Vec::with_capacity(components.len());
        let mut tasks = Vec::with_capacity(components.len());
        for component in components {
            let name = component.name();
            let (outlet, effects) = channel::bounded(name, limits.effects);
            let ctx = ComponentCtx {
                inputs: Box::new(Inputs(sink.clone())),
                effects: Box::new(Effects(effects)),
                data_dir: data_dir.clone(),
                session,
                shutdown: Box::new(shutdown.clone()),
            };
            let handle = eileen_rt::task::spawn(name, component.start(ctx));
            tasks.push(Some(Task { name, handle }));
            outlets.push(Some(outlet));
        }
        drop(sink);
        let origin = (Instant::now(), clock.mono());
        let mut run = Run {
            machine,
            clock,
            inputs: Some(inputs),
            outlets,
            tasks,
            routes,
            timers: Timers::default(),
            hud: HudPace::default(),
            origin,
            seq: 0,
            last: MonoTime(0),
            unserved: BTreeSet::new(),
            stop: shutdown.subscribe(),
            shutdown,
            effects: Vec::new(),
        };
        // A panic in the core's step, or the caller dropping this future, skips `finish`:
        // dropping `run` then triggers the shutdown and aborts every component's task.
        let outcome = run.drive().await;
        run.finish(limits.stop_within).await;
        outcome
    }

    /// A driver around a stand-in for the core.
    #[cfg(test)]
    fn with_machine(machine: Box<dyn Machine>, setup: Setup) -> Self {
        let Setup { data_dir, session, clock, limits, .. } = setup;
        Self { machine, data_dir, session, clock, limits }
    }
}

/// Which component serves each family, by its index and name.
#[derive(Debug)]
struct Routes(BTreeMap<EffectFamily, (usize, &'static str)>);

impl Routes {
    /// Every component's claims, or the first family claimed twice.
    fn claim(components: &[Box<dyn Component>]) -> Result<Self, RunError> {
        let mut routes = BTreeMap::new();
        for (index, component) in components.iter().enumerate() {
            let name = component.name();
            for &family in component.accepts() {
                let first = if family == EffectFamily::Timer {
                    Some(DRIVER)
                } else {
                    match routes.entry(family) {
                        Entry::Vacant(vacant) => {
                            vacant.insert((index, name));
                            None
                        }
                        // The same component naming a family twice claims it once.
                        Entry::Occupied(occupied) => {
                            let (claimed, first) = *occupied.get();
                            (claimed != index).then_some(first)
                        }
                    }
                };
                if let Some(first) = first {
                    return Err(RunError::FamilyClaimedTwice { family, first, second: name });
                }
            }
        }
        Ok(Self(routes))
    }

    fn get(&self, family: EffectFamily) -> Option<(usize, &'static str)> {
        self.0.get(&family).copied()
    }
}

/// A component's sender into the core's input queue. A component sends `seq` as zero: the
/// driver stamps it, so a non-zero one is a component's bug, reported where it happens.
struct Inputs(Sender<Envelope<Input>>);

impl InputSink for Inputs {
    fn send(&self, input: Envelope<Input>) -> Result<(), Backpressure> {
        eileen_stdx::never!(
            input.seq != 0,
            "a component sent an input with seq {}, which the input queue stamps",
            input.seq
        );
        self.0.try_send(input).map_err(|_| Backpressure { capacity: self.0.capacity() })
    }
}

/// A component's effect queue.
struct Effects(Receiver<Envelope<Effect>>);

impl EffectSource for Effects {
    fn recv(&mut self) -> Pin<Box<dyn Future<Output = Option<Envelope<Effect>>> + Send + '_>> {
        Box::pin(self.0.recv())
    }
}

/// A running component's task.
struct Task {
    name: &'static str,
    handle: JoinHandle<Result<(), ComponentError>>,
}

/// How a component's task ended.
struct Ended {
    index: usize,
    name: &'static str,
    result: Result<Result<(), ComponentError>, JoinError>,
}

/// What woke the loop. An input waits beside it, in `Run::drive`.
enum Wake {
    Stop,
    Ended(Ended),
    Timer,
    Hud,
    Input,
}

/// Whether the loop goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

/// The state of one run.
struct Run {
    machine: Box<dyn Machine>,
    clock: Arc<dyn Clock>,
    /// `None` once every sender is gone, or once the run is finishing.
    inputs: Option<Receiver<Envelope<Input>>>,
    /// Each component's effect queue, by index; `None` once it has ended.
    outlets: Vec<Option<Sender<Envelope<Effect>>>>,
    /// Each component's task, by index; `None` once it has ended.
    tasks: Vec<Option<Task>>,
    routes: Routes,
    timers: Timers,
    hud: HudPace,
    /// Tokio's clock and the monotonic clock at the start, to turn a timer's `at` into a
    /// deadline.
    origin: (Instant, MonoTime),
    /// The last input's `seq`.
    seq: u64,
    /// The last input's monotonic time.
    last: MonoTime,
    /// The families already logged as unserved.
    unserved: BTreeSet<EffectFamily>,
    stop: watch::Receiver<bool>,
    shutdown: Shutdown,
    /// The step's effects, kept to reuse its allocation.
    effects: Vec<Envelope<Effect>>,
}

impl Run {
    /// Steps the core until shutdown or a component fails.
    async fn drive(&mut self) -> Result<(), RunError> {
        loop {
            let due = self.timers.next();
            let deadline = due.and_then(|(_, at)| self.deadline(at));
            let paced = self.hud.release_at();
            let mut input = None;
            let wake = tokio::select! {
                biased;
                _ = self.stop.wait_for(|stop| *stop) => Wake::Stop,
                ended = first_ended(&mut self.tasks) => Wake::Ended(ended),
                () = sleep_until(deadline) => Wake::Timer,
                () = sleep_until(paced) => Wake::Hud,
                received = recv(self.inputs.as_mut()) => {
                    input = received;
                    Wake::Input
                }
            };
            let flow = match wake {
                Wake::Stop => Flow::Stop,
                Wake::Ended(ended) => {
                    self.ended(ended)?;
                    Flow::Continue
                }
                Wake::Timer => match due {
                    Some((id, at)) => {
                        let mono = self.stamp(at);
                        self.fire(id, mono).await?
                    }
                    None => Flow::Continue,
                },
                Wake::Hud => self.release_hud().await?,
                Wake::Input => {
                    if let Some(input) = input {
                        self.take(input).await?
                    } else {
                        self.inputs = None;
                        Flow::Continue
                    }
                }
            };
            if flow == Flow::Stop {
                return Ok(());
            }
        }
    }

    /// When the monotonic clock reaches `at`, on tokio's clock; `None` past its range.
    fn deadline(&self, at: MonoTime) -> Option<Instant> {
        let (instant, mono) = self.origin;
        instant.checked_add(Duration::from_nanos(at.0.saturating_sub(mono.0)))
    }

    /// The time the next step takes: the clock's, never before the last step's or `floor`.
    fn stamp(&self, floor: MonoTime) -> MonoTime {
        self.clock.mono().max(self.last).max(floor)
    }

    /// Steps timer `id` at `mono`, no earlier than its `at`, and forgets it.
    async fn fire(&mut self, id: TimerId, mono: MonoTime) -> Result<Flow, RunError> {
        self.timers.apply(&Effect::CancelTimer(id));
        self.step(TraceCtx::default(), Input::Timer(id), mono).await
    }

    /// Steps an input taken off the queue, after every timer due strictly before the time it
    /// takes, which tokio may not have woken yet. Each of these steps takes that one time, so
    /// a timer still pending is due no earlier than the input.
    async fn take(&mut self, input: Envelope<Input>) -> Result<Flow, RunError> {
        let stamp = self.stamp(MonoTime(0));
        while let Some((id, at)) = self.timers.next()
            && at < stamp
        {
            if self.fire(id, stamp).await? == Flow::Stop {
                return Ok(Flow::Stop);
            }
        }
        self.step(input.ctx, input.body, stamp).await
    }

    /// Gives an input its place, steps the core with it at `mono` and routes the effects.
    async fn step(&mut self, ctx: TraceCtx, body: Input, mono: MonoTime) -> Result<Flow, RunError> {
        self.seq = self.seq.saturating_add(1);
        self.last = mono;
        let at = self.clock.wall();
        let seq = self.seq;
        let span = tracing::debug_span!(
            parent: None,
            names::CORE_STEP,
            seq,
            input = label(&body),
            effects = Empty,
        );
        let mut effects = std::mem::take(&mut self.effects);
        let input = Envelope { seq, at, mono, ctx, body };
        span.in_scope(|| self.machine.step(mono, input, &mut effects));
        span.record("effects", effects.len());
        drop(span);
        let mut flow = Ok(Flow::Continue);
        for effect in effects.drain(..) {
            flow = self.route(effect).await;
            if flow != Ok(Flow::Continue) {
                break;
            }
        }
        self.effects = effects;
        flow
    }

    /// Carries one effect out: a timer here, anything else to the component that claims it.
    async fn route(&mut self, effect: Envelope<Effect>) -> Result<Flow, RunError> {
        let family = effect.body.family();
        if family == EffectFamily::Timer {
            self.timers.apply(&effect.body);
            return Ok(Flow::Continue);
        }
        let Some((index, name)) = self.routes.get(family) else {
            self.unserved(family, None);
            return Ok(Flow::Continue);
        };
        let paced = if family == EffectFamily::Hud {
            self.hud.offer(effect, Instant::now())
        } else {
            Some(effect)
        };
        let Some(effect) = paced else { return Ok(Flow::Continue) };
        self.deliver(index, name, family, effect).await
    }

    /// Sends the `Hud` snapshot that waited for its turn.
    async fn release_hud(&mut self) -> Result<Flow, RunError> {
        let Some(effect) = self.hud.release(Instant::now()) else { return Ok(Flow::Continue) };
        let Some((index, name)) = self.routes.get(EffectFamily::Hud) else {
            return Ok(Flow::Continue);
        };
        self.deliver(index, name, EffectFamily::Hud, effect).await
    }

    /// Puts an effect on component `index`'s queue. While the queue is full it waits for room,
    /// and still for shutdown and for any component's end, so a failure elsewhere ends the run
    /// with its name even then.
    async fn deliver(
        &mut self,
        index: usize,
        name: &'static str,
        family: EffectFamily,
        effect: Envelope<Effect>,
    ) -> Result<Flow, RunError> {
        // A clone, so the send can wait across `ended`, which closes the slot it came from.
        let Some(outlet) = self.outlets.get(index).and_then(Option::as_ref).cloned() else {
            self.unserved(family, Some(name));
            return Ok(Flow::Continue);
        };
        let mut sending = pin!(outlet.send(effect));
        loop {
            let ended = tokio::select! {
                biased;
                _ = self.stop.wait_for(|stop| *stop) => return Ok(Flow::Stop),
                result = &mut sending => {
                    if result.is_ok() {
                        return Ok(Flow::Continue);
                    }
                    None
                }
                ended = first_ended(&mut self.tasks) => Some(ended),
            };
            let Some(ended) = ended else {
                // The queue has closed, so its component has ended.
                self.unserved(family, Some(name));
                return Ok(Flow::Continue);
            };
            // When the component that ended is this one, its queue has closed too, and the
            // send ends with the effect unserved on the next turn.
            self.ended(ended)?;
        }
    }

    /// Logs a family no running component serves, once per run.
    fn unserved(&mut self, family: EffectFamily, component: Option<&'static str>) {
        if self.unserved.insert(family) {
            tracing::info!(
                name: names::EFFECT_UNSERVED,
                family = ?family,
                component = component.unwrap_or("none"),
                "no running component serves these effects, so they are dropped",
            );
        }
    }

    /// A component's task ended before shutdown: an error or a panic ends the run, and a
    /// component that returned early leaves its families unserved.
    fn ended(&mut self, ended: Ended) -> Result<(), RunError> {
        let Ended { index, name, result } = ended;
        if let Some(outlet) = self.outlets.get_mut(index) {
            *outlet = None;
        }
        match result {
            Ok(Ok(())) => {
                tracing::warn!(
                    name: names::COMPONENT_ENDED,
                    component = name,
                    outcome = "returned",
                    "a component returned before shutdown; its effects are now dropped",
                );
                Ok(())
            }
            Ok(Err(error)) => {
                log_error(name, &error);
                Err(RunError::Component { component: name, message: Personal::new(error.message) })
            }
            Err(join) => {
                log_join(name, &join);
                Err(RunError::Panicked { component: name })
            }
        }
    }

    /// Stops every component: the shutdown they wait on, their effect queues and the input
    /// queue closed, then up to `within` for them to end before the rest are aborted. A task
    /// leaves its slot only once it has ended, so a run dropped meanwhile still aborts it.
    async fn finish(&mut self, within: Duration) {
        self.shutdown.trigger();
        self.outlets.clear();
        self.inputs = None;
        let deadline = Instant::now().checked_add(within).unwrap_or_else(Instant::now);
        for slot in &mut self.tasks {
            if let Some(task) = slot.as_mut() {
                stop_task(task, deadline).await;
            }
            *slot = None;
        }
    }
}

impl Drop for Run {
    /// Stops what a run that did not finish left running: a panic in the core's step, or a
    /// caller that dropped the run, ends it without `finish`, and a dropped `JoinHandle` would
    /// leave its task running with nothing to stop it.
    fn drop(&mut self) {
        self.shutdown.trigger();
        for task in self.tasks.iter().flatten() {
            task.handle.abort();
        }
    }
}

/// Waits for one component's task until `deadline`, then aborts it.
async fn stop_task(task: &mut Task, deadline: Instant) {
    let name = task.name;
    match tokio::time::timeout_at(deadline, &mut task.handle).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(error))) => log_error(name, &error),
        Ok(Err(join)) => log_join(name, &join),
        Err(_) => {
            task.handle.abort();
            tracing::warn!(
                name: names::COMPONENT_ENDED,
                component = name,
                outcome = "aborted",
                "a component did not stop in time and was aborted",
            );
            if tokio::time::timeout(ABORT_WAIT, &mut task.handle).await.is_err() {
                tracing::warn!(
                    name: names::COMPONENT_ENDED,
                    component = name,
                    outcome = "still_running",
                    "an aborted component's task has not ended",
                );
            }
        }
    }
}

fn log_error(name: &'static str, error: &ComponentError) {
    tracing::error!(
        name: names::COMPONENT_ENDED,
        component = name,
        outcome = "error",
        "pii.message" = %error.message,
    );
}

fn log_join(name: &'static str, join: &JoinError) {
    let outcome = if join.is_panic() { "panicked" } else { "cancelled" };
    tracing::error!(name: names::COMPONENT_ENDED, component = name, outcome);
}

/// The first component task to end, taken out of `tasks`.
fn first_ended(tasks: &mut [Option<Task>]) -> impl Future<Output = Ended> + '_ {
    poll_fn(move |cx| {
        for (index, slot) in tasks.iter_mut().enumerate() {
            let ready = match slot {
                Some(task) => Pin::new(&mut task.handle).poll(cx).map(|result| (task.name, result)),
                None => Poll::Pending,
            };
            if let Poll::Ready((name, result)) = ready {
                *slot = None;
                return Poll::Ready(Ended { index, name, result });
            }
        }
        Poll::Pending
    })
}

/// Sleeps until `deadline`, or for ever without one.
async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => pending().await,
    }
}

/// The next input, or waits for ever once the queue is gone.
async fn recv(inputs: Option<&mut Receiver<Envelope<Input>>>) -> Option<Envelope<Input>> {
    match inputs {
        Some(inputs) => inputs.recv().await,
        None => pending().await,
    }
}

/// An input's kind, for the step's span.
const fn label(input: &Input) -> &'static str {
    match input {
        Input::Voice(_) => "voice",
        Input::Playback(_) => "playback",
        Input::Brain(_) => "brain",
        Input::Worker(..) => "worker",
        Input::Hook(..) => "hook",
        Input::ToolCall(_) => "tool_call",
        Input::ToolResult(..) => "tool_result",
        Input::Sidecar(..) => "sidecar",
        Input::Learn(_) => "learn",
        Input::User(_) => "user",
        Input::Activity(_) => "activity",
        Input::Memory(_) => "memory",
        Input::Settings(_) => "settings",
        Input::Store(_) => "store",
        Input::Timer(_) => "timer",
    }
}

#[cfg(test)]
mod tests;
