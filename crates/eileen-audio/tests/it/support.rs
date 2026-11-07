//! What the integration tests share: the audio component running on a task of its own with the
//! runtime's side of it (its effects, its inputs and its shutdown) over bounded channels, and a
//! paused runtime whose logs a test reads.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use eileen_audio::{AudioConfig, component};
use eileen_contracts::core_io::{Effect, Input};
use eileen_contracts::ids::{Envelope, MonoTime, SessionId, TraceCtx, WallTime};
use eileen_contracts::ports::{
    Backpressure, ComponentCtx, ComponentError, EffectSource, InputSink, ShutdownSignal,
};
use eileen_rt::channel::{self, Receiver, Sender};
use eileen_stdx::soft_assert::{self, Mode};
use eileen_trace::EventRecord;
use eileen_trace::test_support::{Captured, capture};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::sleep;

/// Every wait in these tests that is not the subject of the test.
pub(crate) const WAIT: Duration = Duration::from_secs(5);

pub(crate) fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

pub(crate) const fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Runs `body` on paused tokio time, as `#[tokio::test(start_paused = true)]` does, under a
/// subscriber that captures every event, so the test can read what the component logged.
pub(crate) fn logged<T>(body: impl Future<Output = T>) -> (T, Captured) {
    logged_in(Mode::Panic, body)
}

/// [`logged`] with soft assertions in `mode`: `Report` lets a test see a `never!` fail and the
/// component carry on, as it does in a running Eileen. The component's task runs on the test's
/// thread, so the mode reaches it.
pub(crate) fn logged_in<T>(mode: Mode, body: impl Future<Output = T>) -> (T, Captured) {
    capture(|| {
        soft_assert::with_mode(mode, || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .start_paused(true)
                .build()
                .unwrap()
                .block_on(body)
        })
    })
}

/// The events whose message is `message`.
pub(crate) fn messages<'a>(captured: &'a Captured, message: &str) -> Vec<&'a EventRecord> {
    captured
        .events
        .iter()
        .filter(|event| event.fields.get("message").is_some_and(|m| m == message))
        .collect()
}

fn envelope<T>(body: T) -> Envelope<T> {
    Envelope { seq: 0, at: WallTime(0), mono: MonoTime(0), ctx: TraceCtx::default(), body }
}

struct Inputs(Sender<Envelope<Input>>);

impl InputSink for Inputs {
    fn send(&self, input: Envelope<Input>) -> Result<(), Backpressure> {
        self.0.try_send(input).map_err(|_| Backpressure { capacity: self.0.capacity() })
    }
}

struct Effects(Receiver<Envelope<Effect>>);

impl EffectSource for Effects {
    fn recv(&mut self) -> Pin<Box<dyn Future<Output = Option<Envelope<Effect>>> + Send + '_>> {
        Box::pin(self.0.recv())
    }
}

struct Stop(watch::Receiver<bool>);

impl ShutdownSignal for Stop {
    fn wait(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let mut stop = self.0.clone();
        Box::pin(async move {
            let _stopped = stop.wait_for(|stop| *stop).await;
        })
    }
}

/// The audio component on a task of its own, built through `eileen_audio::component`, and the
/// runtime's side of it.
pub(crate) struct Running {
    effects: Sender<Envelope<Effect>>,
    inputs: Receiver<Envelope<Input>>,
    stop: watch::Sender<bool>,
    task: JoinHandle<Result<(), ComponentError>>,
}

impl Running {
    /// Starts the component on `config`, with an input queue of `inputs` that the test empties
    /// only when it reads it. The task first runs at the test's next wait.
    pub(crate) fn start(config: &AudioConfig, inputs: usize) -> Self {
        let audio = component(config);
        let (effects, effects_rx) = channel::bounded("test.effects", nz(64));
        let (inputs_tx, inputs) = channel::bounded("test.inputs", nz(inputs));
        let (stop, stop_rx) = watch::channel(false);
        let ctx = ComponentCtx {
            inputs: Box::new(Inputs(inputs_tx)),
            effects: Box::new(Effects(effects_rx)),
            data_dir: PathBuf::from("data"),
            session: SessionId::from_parts(1, 1),
            shutdown: Box::new(Stop(stop_rx)),
        };
        let task = eileen_rt::task::spawn("test.audio", audio.start(ctx));
        Self { effects, inputs, stop, task }
    }

    /// Routes an effect to the component and gives it a millisecond to take it.
    pub(crate) async fn effect(&self, body: Effect) {
        self.effects.send(envelope(body)).await.unwrap();
        sleep(ms(1)).await;
    }

    /// Every input the component has sent so far.
    pub(crate) fn inputs(&mut self) -> Vec<Input> {
        std::iter::from_fn(|| self.inputs.try_recv().ok()).map(|input| input.body).collect()
    }

    /// Stops the component and checks that it ended cleanly.
    pub(crate) async fn stop(self) {
        self.stop.send(true).unwrap();
        // Within a write's deadline and a read's, which a component stopping mid-write waits out,
        // with room to spare.
        let within = Duration::from_secs(10);
        let ended = tokio::time::timeout(within, self.task).await.unwrap().unwrap();
        assert_eq!(ended, Ok(()), "the component stops cleanly");
    }
}
