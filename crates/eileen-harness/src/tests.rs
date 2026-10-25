use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use eileen_contracts::core_io::EffectFamily;
use eileen_contracts::jobs::Route;
use eileen_contracts::speech::LanguageSupport;

use crate::{
    AnyWorker, BrainBackend, BrainSuite, DriverError, HarnessConfig, WorkerDriver,
    brain_benchmark, brain_for, component, worker_for,
};

/// The output of a future that is ready at once, as every stub driver's is.
fn at_once<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("a stub driver answers at once"),
    }
}

#[test]
fn only_the_claude_route_has_a_worker_driver() {
    for route in Route::ALL {
        let driver = worker_for(route, &HarnessConfig);
        if route == Route::Claude {
            assert!(matches!(driver, Ok(AnyWorker::Claude(_))), "v0.1 builds Claude");
        } else {
            let refused = Some(DriverError::RouteUnavailable { route });
            assert_eq!(driver.err(), refused, "{route:?} is a designed slot");
        }
    }
}

#[test]
fn the_stub_claude_worker_reports_any_language_and_no_events() {
    let mut worker = worker_for(Route::Claude, &HarnessConfig).unwrap();
    assert_eq!(worker.route(), Route::Claude, "its route");
    assert_eq!(worker.languages(), LanguageSupport::Any, "Claude reports any language");
    assert!(at_once(worker.next_event()).is_none(), "no session, so no event");
}

#[test]
fn the_stub_claude_brain_answers_route_unavailable_and_measures_nothing() {
    let mut brain = brain_for(Route::Claude, &HarnessConfig).unwrap();
    let unavailable = Err(DriverError::RouteUnavailable { route: Route::Claude });
    assert_eq!(at_once(brain.interrupt()), unavailable, "nothing to interrupt until WP3");
    let result = at_once(brain_benchmark(&mut brain, &BrainSuite::default()));
    assert_eq!(result.route, Route::Claude, "the benchmarked route");
    assert_eq!(result.first_word_p50_ms, None, "the stub measures nothing");
    assert!(brain_for(Route::Local, &HarnessConfig).is_err(), "a local brain is a slot");
}

#[test]
fn the_stub_component_takes_the_brain_and_worker_families() {
    let harness = component(&HarnessConfig);
    assert_eq!(harness.name(), "harness", "its name");
    assert_eq!(harness.accepts(), [EffectFamily::Brain, EffectFamily::Worker], "its families");
}
