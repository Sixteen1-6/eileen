//! The core's timers: each comes back at its time, a cancelled one never does, one set again
//! moves, and one due before an input reaches the core before it.

use std::time::Duration;

use eileen_contracts::core_io::{AudioCmd, Effect, EffectFamily, Input, TimerId};
use eileen_contracts::ids::MonoTime;
use tokio::time::{Instant, advance, sleep, sleep_until};

use super::{
    Limits, Probes, Shutdown, envelope, next_seen, physical, scripted, secs, steps_of, toggle,
};

#[tokio::test(start_paused = true)]
async fn a_timer_fires_at_its_mono_time_and_a_cancelled_one_never_does() {
    let (probes, _seen) = Probes::new();
    let (feeder, sink) = probes.feeder("feeder", &[]);
    let mut users = 0;
    let (driver, steps) = scripted(Limits::default(), move |input| {
        if !matches!(input, Input::User(_)) {
            return Vec::new();
        }
        users += 1;
        if users == 1 {
            // Timer 2 is cancelled in the step that set it, timer 3 by the next input.
            vec![
                Effect::SetTimer { id: TimerId(1), at: secs(2) },
                Effect::SetTimer { id: TimerId(2), at: secs(3) },
                Effect::CancelTimer(TimerId(2)),
                Effect::SetTimer { id: TimerId(3), at: secs(5) },
            ]
        } else {
            vec![Effect::CancelTimer(TimerId(3))]
        }
    });
    let shutdown = Shutdown::new();
    let start = Instant::now();
    let run = eileen_rt::task::spawn("test.driver", driver.run(vec![feeder], shutdown.clone()));
    let sink = sink.await.unwrap();
    sink.send(envelope(toggle())).unwrap();
    sleep_until(start + Duration::from_secs(4)).await;
    sink.send(envelope(toggle())).unwrap();
    sleep_until(start + Duration::from_secs(10)).await;
    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "the run ends well");

    let seen: Vec<(u64, MonoTime, MonoTime, Input)> = steps_of(&steps)
        .into_iter()
        .map(|step| (step.input.seq, step.now, step.input.mono, step.input.body))
        .collect();
    assert_eq!(
        seen,
        [
            (1, secs(0), secs(0), toggle()),
            (2, secs(2), secs(2), Input::Timer(TimerId(1))),
            (3, secs(4), secs(4), toggle()),
        ],
        "timer 1 comes back at 2 s, stepped at its own time; timers 2 and 3 never fire"
    );
}

#[tokio::test(start_paused = true)]
async fn a_timer_set_again_moves_to_its_new_time() {
    let (probes, _seen) = Probes::new();
    let (feeder, sink) = probes.feeder("feeder", &[]);
    let mut users = 0;
    let (driver, steps) = scripted(Limits::default(), move |input| {
        if !matches!(input, Input::User(_)) {
            return Vec::new();
        }
        users += 1;
        // The same timer, for 2 s and then, from the second input at 1 s, for 5 s.
        let at = if users == 1 { secs(2) } else { secs(5) };
        vec![Effect::SetTimer { id: TimerId(1), at }]
    });
    let shutdown = Shutdown::new();
    let start = Instant::now();
    let run = eileen_rt::task::spawn("test.driver", driver.run(vec![feeder], shutdown.clone()));
    let sink = sink.await.unwrap();
    sink.send(envelope(toggle())).unwrap();
    sleep_until(start + Duration::from_secs(1)).await;
    sink.send(envelope(toggle())).unwrap();
    sleep_until(start + Duration::from_secs(10)).await;
    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "the run ends well");

    let seen: Vec<(u64, MonoTime, Input)> = steps_of(&steps)
        .into_iter()
        .map(|step| (step.input.seq, step.input.mono, step.input.body))
        .collect();
    assert_eq!(
        seen,
        [(1, secs(0), toggle()), (2, secs(1), toggle()), (3, secs(5), Input::Timer(TimerId(1))),],
        "set again before it fired, the timer comes back once, at its new time"
    );
}

#[tokio::test(start_paused = true)]
async fn a_timer_due_before_an_input_is_stepped_first() {
    let (probes, mut seen) = Probes::new();
    let (feeder, sink) = probes.feeder("feeder", &[EffectFamily::Audio]);
    let mut set = false;
    let (driver, steps) = scripted(Limits::default(), move |input| {
        if set || !matches!(input, Input::User(_)) {
            return Vec::new();
        }
        set = true;
        // Due at 1.0005 s, which tokio holds until its 1,001 ms tick; the audio says the step
        // that set it is over.
        vec![
            Effect::SetTimer { id: TimerId(1), at: MonoTime(1_000_500_000) },
            Effect::Audio(AudioCmd::Unduck),
        ]
    });
    let shutdown = Shutdown::new();
    let run = eileen_rt::task::spawn("test.driver", driver.run(vec![feeder], shutdown.clone()));
    let sink = sink.await.unwrap();
    sink.send(envelope(toggle())).unwrap();
    next_seen(&mut seen).await.expect("the first step's audio");

    // An input arrives at 1.0007 s, past the timer's time but before tokio wakes it.
    advance(Duration::from_micros(1_000_700)).await;
    sink.send(envelope(physical())).unwrap();
    sleep(Duration::from_millis(10)).await;
    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "the run ends well");

    let late = MonoTime(1_000_700_000);
    let stepped: Vec<(u64, MonoTime, MonoTime, Input)> = steps_of(&steps)
        .into_iter()
        .map(|step| (step.input.seq, step.now, step.input.mono, step.input.body))
        .collect();
    assert_eq!(
        stepped,
        [
            (1, secs(0), secs(0), toggle()),
            (2, late, late, Input::Timer(TimerId(1))),
            (3, late, late, physical()),
        ],
        "the timer due first is stepped first, both at the input's time, and fires only once"
    );
}
