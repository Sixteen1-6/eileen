//! The driver on paused tokio time, with a scripted stand-in for the core: routing by family,
//! claims, backpressure, the stamps, the pace of `Hud` snapshots and the log of unserved
//! families here; the core's timers, failures and shutdown, and the real core, in the modules
//! below.

mod fixtures;
mod real_core;
mod stopping;
mod timers;

use std::collections::BTreeMap;
use std::future::{Future, pending};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use eileen_contracts::core_io::{
    ActivityEvent, AudioCmd, Effect, EffectFamily, IncidentTrigger, Input, TimerId, Toast,
    UserInput,
};
use eileen_contracts::ids::{Envelope, MonoTime, SessionId, TraceCtx, WallTime};
use eileen_contracts::ports::{Component, ComponentCtx, ComponentError, InputSink};
use eileen_rt::channel::{self, Receiver, Sender};
use eileen_stdx::clock::Clock;
use eileen_stdx::rng::CoreRng;
use eileen_stdx::soft_assert::{Mode, failures, with_mode};
use eileen_trace::names;
use eileen_trace::test_support::{Captured, capture};
use fixtures::{hud, sample};
use tokio::sync::oneshot;
use tokio::time::{Instant, sleep, sleep_until, timeout, timeout_at};

use super::{Backpressure, DRIVER, Driver, HUD_EVERY, Limits, Machine, RunError, Setup, Shutdown};

/// 2026-01-01T00:00:00Z in microseconds, the paused clock's wall time at its start.
const EPOCH_US: i64 = 1_767_225_600_000_000;

/// Long enough for anything the driver owes a test; on paused time a wait that fails costs
/// nothing.
const PATIENCE: Duration = Duration::from_secs(60);

/// Every family, in `EffectFamily`'s order.
const ALL: [EffectFamily; 16] = [
    EffectFamily::Speech,
    EffectFamily::Audio,
    EffectFamily::Brain,
    EffectFamily::Worker,
    EffectFamily::Hook,
    EffectFamily::ToolForward,
    EffectFamily::ToolReply,
    EffectFamily::Sidecar,
    EffectFamily::Hud,
    EffectFamily::Learn,
    EffectFamily::Settings,
    EffectFamily::Persist,
    EffectFamily::Emit,
    EffectFamily::Timer,
    EffectFamily::Incident,
    EffectFamily::Notify,
];

/// Three components that, with the driver's `Timer`, claim every family between them.
const VOICE: &[EffectFamily] = &[EffectFamily::Speech, EffectFamily::Audio];
const MODELS: &[EffectFamily] = &[
    EffectFamily::Brain,
    EffectFamily::Worker,
    EffectFamily::Hook,
    EffectFamily::ToolForward,
    EffectFamily::ToolReply,
];
const REST: &[EffectFamily] = &[
    EffectFamily::Sidecar,
    EffectFamily::Hud,
    EffectFamily::Learn,
    EffectFamily::Settings,
    EffectFamily::Persist,
    EffectFamily::Emit,
    EffectFamily::Incident,
    EffectFamily::Notify,
];

const fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

const fn secs(n: u64) -> MonoTime {
    MonoTime(n * 1_000_000_000)
}

/// Monotonic and wall time from tokio's clock, so on paused time they move only as it does.
struct Paused {
    start: Instant,
}

impl Paused {
    fn new() -> Self {
        Self { start: Instant::now() }
    }
}

impl Clock for Paused {
    fn mono(&self) -> MonoTime {
        MonoTime(u64::try_from(self.start.elapsed().as_nanos()).unwrap())
    }

    fn wall(&self) -> WallTime {
        WallTime(EPOCH_US + i64::try_from(self.start.elapsed().as_micros()).unwrap())
    }
}

fn setup(limits: Limits) -> Setup {
    Setup {
        data_dir: PathBuf::from("data"),
        session: SessionId::from_parts(1, 1),
        clock: Arc::new(Paused::new()),
        rng: CoreRng::from_seed(7),
        snapshot: None,
        limits,
    }
}

/// Runs `test` on a paused current-thread runtime, capturing its tracing.
fn on_paused_time<T>(test: impl Future<Output = T>) -> (T, Captured) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    capture(|| runtime.block_on(test))
}

/// Each event named `name`, as the values of its fields `first` and `second`.
fn lines<'a>(
    captured: &'a Captured,
    name: &str,
    first: &str,
    second: &str,
) -> Vec<(&'a str, &'a str)> {
    captured
        .events_named(name)
        .into_iter()
        .map(|event| {
            (event.fields[first].as_str().unwrap(), event.fields[second].as_str().unwrap())
        })
        .collect()
}

/// One step of the stand-in core: the time it was stepped at and the input.
#[derive(Debug, Clone, PartialEq)]
struct Step {
    now: MonoTime,
    input: Envelope<Input>,
}

type Steps = Arc<Mutex<Vec<Step>>>;

fn steps_of(steps: &Steps) -> Vec<Step> {
    steps.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// What the stand-in core answers an input with.
type Reply = Box<dyn FnMut(&Input) -> Vec<Effect> + Send>;

/// Stands in for the core: answers each input with what `reply` gives for it, and keeps every
/// step it was given.
struct Scripted {
    reply: Reply,
    steps: Steps,
    seq: u64,
}

impl Machine for Scripted {
    fn step(&mut self, now: MonoTime, input: Envelope<Input>, effects: &mut Vec<Envelope<Effect>>) {
        for body in (self.reply)(&input.body) {
            self.seq += 1;
            effects.push(Envelope { seq: self.seq, at: input.at, mono: now, ctx: input.ctx, body });
        }
        self.steps.lock().unwrap_or_else(PoisonError::into_inner).push(Step { now, input });
    }
}

/// A driver around the stand-in, and the steps it will be given.
fn scripted(
    limits: Limits,
    reply: impl FnMut(&Input) -> Vec<Effect> + Send + 'static,
) -> (Driver, Steps) {
    let steps = Steps::default();
    let machine = Scripted { reply: Box::new(reply), steps: Arc::clone(&steps), seq: 0 };
    (Driver::with_machine(Box::new(machine), setup(limits)), steps)
}

fn envelope(body: Input) -> Envelope<Input> {
    Envelope { seq: 0, at: WallTime(0), mono: MonoTime(0), ctx: TraceCtx::default(), body }
}

const fn toggle() -> Input {
    Input::User(UserInput::ToggleMute)
}

const fn physical() -> Input {
    Input::Activity(ActivityEvent::PhysicalInput)
}

/// What a probe does once started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    /// Forwards its effects to the test until shutdown or until its queue closes.
    Serve,
    /// Ignores shutdown and its effects.
    Stubborn,
    /// Fails at once.
    Fail,
    /// Fails after this long.
    FailAfter(Duration),
    /// Panics at once.
    Panic,
    /// Returns at once, before any shutdown.
    ReturnEarly,
}

/// How many probes have started, and how many have ended (returned or dropped).
#[derive(Debug, Clone, Default)]
struct Counts {
    started: Arc<AtomicUsize>,
    ended: Arc<AtomicUsize>,
}

impl Counts {
    fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    fn ended(&self) -> usize {
        self.ended.load(Ordering::SeqCst)
    }
}

/// Counts a probe as ended when its future is dropped, aborted or not.
struct EndGuard(Arc<AtomicUsize>);

impl Drop for EndGuard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// A test component: forwards each effect routed to it to the test with its name, and can
/// hand the test its input sink.
struct Probe {
    name: &'static str,
    accepts: &'static [EffectFamily],
    act: Act,
    seen: Sender<(&'static str, Envelope<Effect>)>,
    counts: Counts,
    sink: Option<oneshot::Sender<Box<dyn InputSink>>>,
}

/// What a failing probe stops with.
fn failure(component: &'static str) -> Result<(), ComponentError> {
    Err(ComponentError { component, message: "the device went away".into() })
}

impl Component for Probe {
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
            let Self { name, act, seen, counts, sink, .. } = *self;
            counts.started.fetch_add(1, Ordering::SeqCst);
            let _ended = EndGuard(Arc::clone(&counts.ended));
            let ComponentCtx { inputs, mut effects, shutdown, .. } = ctx;
            if let Some(sink) = sink {
                let _handed = sink.send(inputs);
            }
            match act {
                Act::Fail => failure(name),
                Act::FailAfter(after) => {
                    sleep(after).await;
                    failure(name)
                }
                Act::Panic => panic!("the {name} probe broke"),
                Act::ReturnEarly => Ok(()),
                Act::Stubborn => pending().await,
                Act::Serve => loop {
                    let next = tokio::select! {
                        () = shutdown.wait() => None,
                        effect = effects.recv() => effect,
                    };
                    let Some(effect) = next else { return Ok(()) };
                    seen.send((name, effect)).await.unwrap();
                },
            }
        })
    }
}

/// Builds the probes of one test, which share its counts and its channel of effects seen.
struct Probes {
    counts: Counts,
    seen: Sender<(&'static str, Envelope<Effect>)>,
}

impl Probes {
    fn new() -> (Self, Receiver<(&'static str, Envelope<Effect>)>) {
        let (seen, rx) = channel::bounded("test.seen", nz(256));
        (Self { counts: Counts::default(), seen }, rx)
    }

    fn probe(&self, name: &'static str, accepts: &'static [EffectFamily], act: Act) -> Probe {
        Probe {
            name,
            accepts,
            act,
            seen: self.seen.clone(),
            counts: self.counts.clone(),
            sink: None,
        }
    }

    /// A probe, boxed as a component.
    fn boxed(
        &self,
        name: &'static str,
        accepts: &'static [EffectFamily],
        act: Act,
    ) -> Box<dyn Component> {
        Box::new(self.probe(name, accepts, act))
    }

    /// A serving probe that also hands the test its input sink.
    fn feeder(
        &self,
        name: &'static str,
        accepts: &'static [EffectFamily],
    ) -> (Box<dyn Component>, oneshot::Receiver<Box<dyn InputSink>>) {
        let (tx, rx) = oneshot::channel();
        let mut probe = self.probe(name, accepts, Act::Serve);
        probe.sink = Some(tx);
        (Box::new(probe), rx)
    }
}

/// The next effect a probe saw, or `None` when none comes within [`PATIENCE`].
async fn next_seen(
    seen: &mut Receiver<(&'static str, Envelope<Effect>)>,
) -> Option<(&'static str, Envelope<Effect>)> {
    timeout(PATIENCE, seen.recv()).await.ok().flatten()
}

#[tokio::test(start_paused = true)]
async fn each_family_reaches_the_component_that_accepts_it() {
    let (probes, mut seen) = Probes::new();
    let (voice, sink) = probes.feeder("voice", VOICE);
    let components: Vec<Box<dyn Component>> = vec![
        voice,
        probes.boxed("models", MODELS, Act::Serve),
        probes.boxed("rest", REST, Act::Serve),
    ];
    let (driver, _steps) = scripted(Limits::default(), |input| {
        if matches!(input, Input::User(_)) {
            ALL.into_iter().map(sample).collect()
        } else {
            Vec::new()
        }
    });
    let shutdown = Shutdown::new();
    let run = eileen_rt::task::spawn("test.driver", driver.run(components, shutdown.clone()));
    sink.await.unwrap().send(envelope(toggle())).unwrap();

    // Every effect but the timer's, each with the core's own seq.
    let mut by_component: BTreeMap<&str, Vec<Envelope<Effect>>> = BTreeMap::new();
    for _ in 0..ALL.len() - 1 {
        let (name, effect) = next_seen(&mut seen).await.expect("an effect went missing");
        by_component.entry(name).or_default().push(effect);
    }
    for (name, claims) in [("voice", VOICE), ("models", MODELS), ("rest", REST)] {
        let got = &by_component[name];
        let bodies: Vec<Effect> = got.iter().map(|effect| effect.body.clone()).collect();
        let wanted: Vec<Effect> = claims.iter().copied().map(sample).collect();
        assert_eq!(bodies, wanted, "{name} gets exactly its families, in order and unchanged");
        let seqs: Vec<u64> = got.iter().map(|effect| effect.seq).collect();
        let core_seqs: Vec<u64> = claims
            .iter()
            .map(|family| {
                1 + u64::try_from(ALL.iter().position(|all| all == family).unwrap()).unwrap()
            })
            .collect();
        assert_eq!(seqs, core_seqs, "{name}'s effects keep the core's seq");
    }
    assert!(next_seen(&mut seen).await.is_none(), "the timer effect stays in the driver");

    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "a run asked to stop ends well");
}

#[tokio::test(start_paused = true)]
async fn a_family_claimed_twice_refuses_to_start() {
    let cases: [(&'static [EffectFamily], &'static [EffectFamily], RunError); 2] = [
        (
            &[EffectFamily::Speech, EffectFamily::Audio],
            &[EffectFamily::Hud, EffectFamily::Audio],
            RunError::FamilyClaimedTwice {
                family: EffectFamily::Audio,
                first: "first",
                second: "second",
            },
        ),
        (
            &[EffectFamily::Speech],
            &[EffectFamily::Timer],
            RunError::FamilyClaimedTwice {
                family: EffectFamily::Timer,
                first: DRIVER,
                second: "second",
            },
        ),
    ];
    for (first, second, refusal) in cases {
        let (probes, _seen) = Probes::new();
        let components: Vec<Box<dyn Component>> = vec![
            probes.boxed("first", first, Act::Serve),
            probes.boxed("second", second, Act::Serve),
        ];
        let (driver, steps) = scripted(Limits::default(), |_| Vec::new());
        let outcome = timeout(PATIENCE, driver.run(components, Shutdown::new())).await;
        assert_eq!(outcome, Ok(Err(refusal.clone())), "refused at once: {refusal}");
        assert_eq!(probes.counts.started(), 0, "no component started: {refusal}");
        assert!(steps_of(&steps).is_empty(), "the core was never stepped: {refusal}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_full_input_queue_answers_backpressure() {
    let (probes, _seen) = Probes::new();
    let (feeder, sink) = probes.feeder("feeder", &[]);
    let limits = Limits { inputs: nz(4), ..Limits::default() };
    let (driver, steps) = scripted(limits, |_| Vec::new());
    let shutdown = Shutdown::new();
    let run = eileen_rt::task::spawn("test.driver", driver.run(vec![feeder], shutdown.clone()));
    let sink = sink.await.unwrap();

    // Nothing yields between these sends, so the driver takes none of them yet.
    for n in 0..4 {
        assert_eq!(sink.send(envelope(physical())), Ok(()), "input {n} fits");
    }
    assert_eq!(
        sink.send(envelope(physical())),
        Err(Backpressure { capacity: 4 }),
        "the fifth is refused"
    );

    // Once the driver has drained the queue there is room again.
    sleep(Duration::from_millis(1)).await;
    assert_eq!(sink.send(envelope(physical())), Ok(()), "room again once drained");
    sleep(Duration::from_millis(1)).await;
    let seqs: Vec<u64> = steps_of(&steps).iter().map(|step| step.input.seq).collect();
    assert_eq!(seqs, [1, 2, 3, 4, 5], "the four taken and the one after, in order");

    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "the run ends well");
}

#[tokio::test(start_paused = true)]
async fn inputs_take_seq_and_time_in_the_order_the_core_sees() {
    let (probes, _seen) = Probes::new();
    let (first, first_sink) = probes.feeder("first", &[]);
    let (second, second_sink) = probes.feeder("second", &[]);
    // The first input sets a timer for 1 s, which lands between the two rounds of sends.
    let mut set = false;
    let (driver, steps) = scripted(Limits::default(), move |input| {
        if set || !matches!(input, Input::User(_)) {
            return Vec::new();
        }
        set = true;
        vec![Effect::SetTimer { id: TimerId(1), at: secs(1) }]
    });
    let shutdown = Shutdown::new();
    let start = Instant::now();
    let run =
        eileen_rt::task::spawn("test.driver", driver.run(vec![first, second], shutdown.clone()));
    let (first_sink, second_sink) = (first_sink.await.unwrap(), second_sink.await.unwrap());

    // Both components send at once, each with seq and times zero, then again at 1.5 s.
    first_sink.send(envelope(toggle())).unwrap();
    second_sink.send(envelope(physical())).unwrap();
    first_sink.send(envelope(toggle())).unwrap();
    sleep_until(start + Duration::from_millis(1_500)).await;
    second_sink.send(envelope(physical())).unwrap();
    first_sink.send(envelope(toggle())).unwrap();
    sleep_until(start + Duration::from_secs(2)).await;
    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "the run ends well");

    let half = MonoTime(1_500_000_000);
    let seen: Vec<(u64, MonoTime, MonoTime, Input)> = steps_of(&steps)
        .into_iter()
        .map(|step| (step.input.seq, step.now, step.input.mono, step.input.body))
        .collect();
    assert_eq!(
        seen,
        [
            (1, secs(0), secs(0), toggle()),
            (2, secs(0), secs(0), physical()),
            (3, secs(0), secs(0), toggle()),
            (4, secs(1), secs(1), Input::Timer(TimerId(1))),
            (5, half, half, physical()),
            (6, half, half, toggle()),
        ],
        "seq counts up in the order the core sees, the timer's included, and time never goes back"
    );
}

#[tokio::test(start_paused = true)]
async fn an_input_sent_with_a_seq_trips_the_soft_assertion() {
    let (probes, _seen) = Probes::new();
    let (feeder, sink) = probes.feeder("feeder", &[]);
    let (driver, steps) = scripted(Limits::default(), |_| Vec::new());
    let shutdown = Shutdown::new();
    let run = eileen_rt::task::spawn("test.driver", driver.run(vec![feeder], shutdown.clone()));
    let sink = sink.await.unwrap();
    sleep(Duration::from_secs(3)).await;

    let before = failures();
    let stamped = Envelope { seq: 7, at: WallTime(9), mono: MonoTime(9), ..envelope(toggle()) };
    let sent = with_mode(Mode::Report, || sink.send(stamped));
    assert_eq!(sent, Ok(()), "a running Eileen still takes the input");
    assert_eq!(failures(), before + 1, "and the soft assertion failed once");
    sleep(Duration::from_millis(1)).await;
    let first = steps_of(&steps).first().map(|step| (step.input.seq, step.input.mono));
    assert_eq!(first, Some((1, secs(3))), "the queue's own stamp replaced what was sent");

    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "the run ends well");
}

#[tokio::test(start_paused = true)]
async fn hud_snapshots_go_at_most_ten_a_second_ending_with_the_last() {
    let (probes, mut seen) = Probes::new();
    let (hud_probe, sink) = probes.feeder("hud", &[EffectFamily::Hud]);
    let (driver, _steps) = scripted(Limits::default(), |_| vec![Effect::Hud(Box::new(hud()))]);
    let shutdown = Shutdown::new();
    let start = Instant::now();
    let run = eileen_rt::task::spawn("test.driver", driver.run(vec![hud_probe], shutdown.clone()));
    // Each snapshot the HUD gets, by the core's seq, with the time it got it.
    let taken = eileen_rt::task::spawn("test.hud", async move {
        let mut taken = Vec::new();
        let until = start + Duration::from_secs(3);
        while let Ok(Some((_, effect))) = timeout_at(until, seen.recv()).await {
            taken.push((start.elapsed(), effect.seq));
        }
        taken
    });
    let sink = sink.await.unwrap();

    // 30 snapshots, one every 33 ms from 0 to 957 ms.
    for _ in 0..30 {
        sink.send(envelope(toggle())).unwrap();
        sleep(Duration::from_millis(33)).await;
    }
    let taken = taken.await.unwrap();
    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "the run ends well");

    let times: Vec<Duration> = taken.iter().map(|&(at, _)| at).collect();
    let paced: Vec<Duration> = (0..=10).map(|n| HUD_EVERY * n).collect();
    assert_eq!(times, paced, "the first at once, then one every 100 ms while more come");
    let seqs: Vec<u64> = taken.iter().map(|&(_, seq)| seq).collect();
    assert_eq!(
        seqs,
        [1, 4, 7, 10, 13, 16, 19, 22, 25, 28, 30],
        "each the newest at its turn, and the last snapshot last"
    );
}

#[test]
fn an_unserved_family_is_logged_once_and_dropped() {
    let ((outcome, audio, steps), captured) = on_paused_time(async {
        let (probes, mut seen) = Probes::new();
        let (voice, sink) = probes.feeder("voice", VOICE);
        let (driver, steps) = scripted(Limits::default(), |_| {
            vec![
                Effect::Notify(Toast {
                    title: "Done".to_owned(),
                    body: "Done.".to_owned(),
                    job: None,
                }),
                Effect::Audio(AudioCmd::Unduck),
                Effect::Incident(IncidentTrigger::UserReport),
            ]
        });
        let shutdown = Shutdown::new();
        let run = eileen_rt::task::spawn("test.driver", driver.run(vec![voice], shutdown.clone()));
        let sink = sink.await.unwrap();
        let mut audio = 0;
        for _ in 0..3 {
            sink.send(envelope(toggle())).unwrap();
            let (_, effect) = next_seen(&mut seen).await.expect("the audio effect");
            assert_eq!(effect.body, Effect::Audio(AudioCmd::Unduck), "only audio is served");
            audio += 1;
        }
        shutdown.trigger();
        (run.await.unwrap(), audio, steps_of(&steps).len())
    });
    assert_eq!(outcome, Ok(()), "the run ends well");
    assert_eq!((audio, steps), (3, 3), "three steps, each served its audio");
    assert_eq!(
        lines(&captured, names::EFFECT_UNSERVED, "family", "component"),
        [("Notify", "none"), ("Incident", "none")],
        "one line per unserved family, however often it comes"
    );
    assert_eq!(captured.spans_named(names::CORE_STEP).len(), 3, "one span per step");
    captured.assert_schema_ok();
}
