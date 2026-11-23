//! How a run ends: shutdown stops every component, a component's error or panic ends the run
//! with its name, even while the driver waits on a full queue, a component that returns early
//! leaves its families unserved, and a panic in the core still stops every component.

use std::time::Duration;

use eileen_contracts::core_io::{AudioCmd, Effect, Input};
use eileen_contracts::ids::{Envelope, MonoTime, Personal};
use eileen_trace::names;
use tokio::time::{Instant, sleep, timeout};

use super::{
    Act, Driver, Limits, MODELS, Machine, PATIENCE, Probes, REST, RunError, Shutdown, VOICE,
    envelope, lines, nz, on_paused_time, scripted, setup, steps_of, toggle,
};

/// The error a failing probe ends the run with.
fn failed(component: &'static str) -> RunError {
    RunError::Component { component, message: Personal::new("the device went away".to_owned()) }
}

#[test]
fn shutdown_stops_every_component() {
    let ((outcome, ended, waited), captured) = on_paused_time(async {
        let (probes, _seen) = Probes::new();
        let components = vec![
            probes.boxed("voice", VOICE, Act::Serve),
            probes.boxed("stubborn", MODELS, Act::Stubborn),
            probes.boxed("rest", REST, Act::Serve),
        ];
        let limits = Limits { stop_within: Duration::from_secs(2), ..Limits::default() };
        let (driver, _steps) = scripted(limits, |_| Vec::new());
        let shutdown = Shutdown::new();
        let run = eileen_rt::task::spawn("test.driver", driver.run(components, shutdown.clone()));
        sleep(Duration::from_secs(1)).await;
        assert_eq!(probes.counts.started(), 3, "every component started");
        assert_eq!(probes.counts.ended(), 0, "and runs until shutdown");

        let asked = Instant::now();
        shutdown.trigger();
        let outcome = run.await.unwrap();
        (outcome, probes.counts.ended(), asked.elapsed())
    });
    assert_eq!(outcome, Ok(()), "a run asked to stop ends well");
    assert_eq!(ended, 3, "every component's task has ended");
    assert_eq!(waited, Duration::from_secs(2), "the one that ignored it was aborted");
    assert_eq!(
        lines(&captured, names::COMPONENT_ENDED, "component", "outcome"),
        [("stubborn", "aborted")],
        "one line, for the component aborted"
    );
}

#[tokio::test(start_paused = true)]
async fn a_component_error_ends_the_run_with_its_name() {
    let (probes, _seen) = Probes::new();
    let components =
        vec![probes.boxed("voice", VOICE, Act::Serve), probes.boxed("models", MODELS, Act::Fail)];
    let (driver, _steps) = scripted(Limits::default(), |_| Vec::new());
    let shutdown = Shutdown::new();
    let outcome = timeout(PATIENCE, driver.run(components, shutdown.clone())).await;
    assert_eq!(outcome, Ok(Err(failed("models"))), "the failed component's name and words");
    assert!(shutdown.is_triggered(), "the others were told to stop");
    assert_eq!(probes.counts.ended(), 2, "and every component's task has ended");
}

#[tokio::test(start_paused = true)]
async fn a_component_panic_ends_the_run_with_its_name() {
    let (probes, _seen) = Probes::new();
    let components =
        vec![probes.boxed("voice", VOICE, Act::Serve), probes.boxed("models", MODELS, Act::Panic)];
    let (driver, _steps) = scripted(Limits::default(), |_| Vec::new());
    let shutdown = Shutdown::new();
    let outcome = timeout(PATIENCE, driver.run(components, shutdown.clone())).await;
    assert_eq!(
        outcome,
        Ok(Err(RunError::Panicked { component: "models" })),
        "the panicked component's name"
    );
    assert!(shutdown.is_triggered(), "the others were told to stop");
    assert_eq!(probes.counts.ended(), 2, "and every component's task has ended");
}

#[test]
fn a_component_that_returns_early_leaves_its_families_unserved() {
    let ((outcome, steps), captured) = on_paused_time(async {
        let (probes, _seen) = Probes::new();
        let (feeder, sink) = probes.feeder("feeder", &[]);
        let components = vec![feeder, probes.boxed("voice", VOICE, Act::ReturnEarly)];
        let (driver, steps) =
            scripted(Limits::default(), |_| vec![Effect::Audio(AudioCmd::Unduck)]);
        let shutdown = Shutdown::new();
        let run = eileen_rt::task::spawn("test.driver", driver.run(components, shutdown.clone()));
        let sink = sink.await.unwrap();
        sleep(Duration::from_secs(1)).await;
        sink.send(envelope(toggle())).unwrap();
        sink.send(envelope(toggle())).unwrap();
        sleep(Duration::from_secs(1)).await;
        shutdown.trigger();
        (run.await.unwrap(), steps_of(&steps).len())
    });
    assert_eq!(outcome, Ok(()), "a component that returned does not end the run");
    assert_eq!(steps, 2, "and the core is still stepped");
    assert_eq!(
        lines(&captured, names::COMPONENT_ENDED, "component", "outcome"),
        [("voice", "returned")],
        "its early return is logged"
    );
    assert_eq!(
        lines(&captured, names::EFFECT_UNSERVED, "family", "component"),
        [("Audio", "voice")],
        "and its family is unserved, once, naming it"
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_unblocks_a_driver_waiting_on_a_full_component_queue() {
    let (probes, _seen) = Probes::new();
    let (feeder, sink) = probes.feeder("feeder", &[]);
    let components = vec![feeder, probes.boxed("stubborn", VOICE, Act::Stubborn)];
    let limits =
        Limits { effects: nz(1), stop_within: Duration::from_secs(2), ..Limits::default() };
    let (driver, steps) = scripted(limits, |_| vec![Effect::Audio(AudioCmd::Unduck); 3]);
    let shutdown = Shutdown::new();
    let run = eileen_rt::task::spawn("test.driver", driver.run(components, shutdown.clone()));
    let sink = sink.await.unwrap();
    sink.send(envelope(toggle())).unwrap();
    sink.send(envelope(toggle())).unwrap();
    sleep(Duration::from_secs(1)).await;
    assert_eq!(steps_of(&steps).len(), 1, "the second effect waits for room, and the core with it");

    let asked = Instant::now();
    shutdown.trigger();
    let outcome = timeout(PATIENCE, run).await.expect("the run ends").unwrap();
    assert_eq!(outcome, Ok(()), "a run asked to stop ends well");
    assert_eq!(asked.elapsed(), Duration::from_secs(2), "once the stubborn one is aborted");
}

#[tokio::test(start_paused = true)]
async fn a_component_failure_ends_a_run_blocked_on_a_full_queue() {
    let (probes, _seen) = Probes::new();
    let (feeder, sink) = probes.feeder("feeder", &[]);
    let components = vec![
        feeder,
        probes.boxed("stubborn", VOICE, Act::Stubborn),
        probes.boxed("models", MODELS, Act::FailAfter(Duration::from_secs(1))),
    ];
    let limits =
        Limits { effects: nz(1), stop_within: Duration::from_secs(2), ..Limits::default() };
    let (driver, _steps) = scripted(limits, |_| vec![Effect::Audio(AudioCmd::Unduck); 3]);
    let shutdown = Shutdown::new();
    let start = Instant::now();
    let run = eileen_rt::task::spawn("test.driver", driver.run(components, shutdown.clone()));
    sink.await.unwrap().send(envelope(toggle())).unwrap();

    let outcome = timeout(PATIENCE, run).await.expect("the run ends").unwrap();
    assert_eq!(outcome, Err(failed("models")), "the failed component's name, not a hang");
    assert_eq!(
        start.elapsed(),
        Duration::from_secs(3),
        "it failed at 1 s, and the stubborn one was aborted 2 s later"
    );
    assert!(shutdown.is_triggered(), "the others were told to stop");
}

/// A stand-in core that panics on its first step.
struct Breaks;

impl Machine for Breaks {
    fn step(&mut self, _: MonoTime, _: Envelope<Input>, _: &mut Vec<Envelope<Effect>>) {
        panic!("the stand-in core broke");
    }
}

#[tokio::test(start_paused = true)]
async fn a_core_panic_still_stops_every_component() {
    let (probes, _seen) = Probes::new();
    let (feeder, sink) = probes.feeder("feeder", &[]);
    let components = vec![feeder, probes.boxed("stubborn", VOICE, Act::Stubborn)];
    let driver = Driver::with_machine(Box::new(Breaks), setup(Limits::default()));
    let shutdown = Shutdown::new();
    let run = eileen_rt::task::spawn("test.driver", driver.run(components, shutdown.clone()));
    sink.await.unwrap().send(envelope(toggle())).unwrap();

    let joined = timeout(PATIENCE, run).await.expect("the driver's task ends");
    assert!(joined.is_err_and(|error| error.is_panic()), "the core's panic ends it");
    // An aborted task ends the next time the runtime turns to it.
    sleep(Duration::from_millis(1)).await;
    assert!(shutdown.is_triggered(), "the components were told to stop");
    assert_eq!(probes.counts.started(), 2, "both started");
    assert_eq!(probes.counts.ended(), 2, "and both tasks have ended, the stubborn one aborted");
}
