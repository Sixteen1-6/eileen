//! `AudioEndpoints` on Linux: `LinuxAudioEndpoints` through a bridge on the private bus, against
//! the fake rtkit of the client's tests. The test's thread registers as an audio thread does,
//! blocking; the fake runs on a runtime of its own, whose workers serve it meanwhile.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use eileen_audio_linux::{LinuxAudioEndpoints, LinuxAudioSettings, RealtimePolicy};
use eileen_dbus_linux::testing::{PrivateBus, serve};
use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE};
use eileen_platform::{AudioEndpoints, Deadline, PlatformError, StreamCategory};
use tokio::sync::Notify;
use tokio::sync::mpsc::error::TryRecvError;

use crate::rtkit::{
    Call, DEFAULTS, FakeRtkit, Gate, Limits, RTKIT, RTKIT_PATH, this_nice, this_thread,
};

/// The deadline of a registration whose fake holds its answer: it passes whatever its length,
/// and a short one keeps the test short.
const SHORT: Deadline = Deadline::after(Duration::from_millis(100));

/// The fake, on a multi-thread runtime of its own.
struct Fakes {
    runtime: tokio::runtime::Runtime,
    service: Option<zbus::Connection>,
}

impl Fakes {
    /// Serves `fake` as rtkit on `bus` until the fakes drop.
    fn serve(bus: &PrivateBus, fake: FakeRtkit) -> Self {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build();
        let runtime = runtime.unwrap();
        let service = runtime.block_on(serve(bus, RTKIT, RTKIT_PATH, fake));
        Self { runtime, service: Some(service) }
    }
}

impl Drop for Fakes {
    fn drop(&mut self) {
        // The connection ends inside its runtime's context, a panicking test's included.
        let _context = self.runtime.enter();
        drop(self.service.take());
    }
}

/// Endpoints on a bridge to `bus`, with `deadline` and the default policy.
fn endpoints_on(bus: &PrivateBus, deadline: Deadline) -> LinuxAudioEndpoints {
    let settings = LinuxAudioSettings { deadline, ..LinuxAudioSettings::default() };
    LinuxAudioEndpoints::new(Bridge::on_private_bus(bus).unwrap(), settings)
}

/// Lowers the calling thread's priority by one nice level, up to 19, which needs no privilege,
/// so that a revert to nice 0 cannot pass for one to the level the thread had. Returns the
/// level.
fn lower_priority() -> i32 {
    let nice = (this_nice() + 1).min(19);
    rustix::process::setpriority_process(Some(rustix::thread::gettid()), nice).unwrap();
    assert_eq!(this_nice(), nice, "this thread's own nice level");
    nice
}

#[test]
fn register_audio_thread_promotes_the_calling_thread_and_its_ticket_reverts_it() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, None);
    let _fakes = Fakes::serve(&bus, fake.clone());
    let endpoints = endpoints_on(&bus, DEFAULT_DEADLINE);
    let (thread, nice) = (this_thread(), lower_priority());
    let ticket = endpoints.register_audio_thread().unwrap();
    assert_eq!(ticket.task(), "RealtimeKit real-time", "as the fake grants");
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(thread, 10)), "the calling thread, at 10");
    drop(ticket);
    // The drop returns once the promoter has the revert's outcome, which the fake sends after
    // its report.
    assert_eq!(
        calls.try_recv(),
        Ok(Call::HighPriority(thread, nice)),
        "back to the nice level it had, by then"
    );
    assert_eq!(fake.resume(), [], "and forgotten, so a resume after a suspend leaves it alone");
}

#[test]
fn a_refused_registration_falls_back_to_high_priority_and_its_ticket_reverts_it() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, None);
    fake.refuse_realtime.store(true, Ordering::SeqCst);
    let _fakes = Fakes::serve(&bus, fake);
    let endpoints = endpoints_on(&bus, DEFAULT_DEADLINE);
    let (thread, nice) = (this_thread(), lower_priority());
    let ticket = endpoints.register_audio_thread().unwrap();
    assert_eq!(ticket.task(), "RealtimeKit high priority", "as the fake grants");
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(thread, 10)), "real-time first");
    assert_eq!(calls.try_recv(), Ok(Call::HighPriority(thread, -11)), "then high priority");
    drop(ticket);
    assert_eq!(
        calls.try_recv(),
        Ok(Call::HighPriority(thread, nice)),
        "back to the nice level it had"
    );
}

#[test]
fn a_refused_registration_leaves_a_thread_that_already_runs_higher_alone() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, None);
    fake.refuse_realtime.store(true, Ordering::SeqCst);
    let _fakes = Fakes::serve(&bus, fake);
    // Nice 19, the lowest priority there is, is to this thread what -11 is to a thread at -16,
    // which a test cannot make without privilege.
    let policy = RealtimePolicy { fallback_nice: 19, ..RealtimePolicy::default() };
    let settings = LinuxAudioSettings { policy, ..LinuxAudioSettings::default() };
    let endpoints = LinuxAudioEndpoints::new(Bridge::on_private_bus(&bus).unwrap(), settings);
    let thread = this_thread();
    let ticket = endpoints.register_audio_thread().unwrap();
    assert_eq!(ticket.task(), "RealtimeKit unchanged", "real-time refused, and nothing else asked");
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(thread, 10)), "real-time asked");
    drop(ticket);
    assert_eq!(
        calls.try_recv(),
        Err(TryRecvError::Empty),
        "neither the fallback, which would lower the thread, nor a revert, with nothing to revert"
    );
}

#[test]
fn register_audio_thread_on_an_empty_bus_is_unavailable() {
    let bus = PrivateBus::start();
    assert_eq!(
        endpoints_on(&bus, DEFAULT_DEADLINE).register_audio_thread().unwrap_err(),
        PlatformError::Unavailable { what: "org.freedesktop.RealtimeKit1.RTTimeUSecMax" },
        "the promoter's own error: the private bus has no activation, so the first read answers \
         ServiceUnknown"
    );
}

#[test]
fn a_ticket_reverts_a_thread_whose_nice_level_is_below_the_minimum_to_the_minimum() {
    let bus = PrivateBus::start();
    let (thread, nice) = (this_thread(), lower_priority());
    assert!(nice < 19, "a nice level that a minimum can be above: {nice}");
    let (fake, mut calls) = FakeRtkit::new(Limits { min_nice_level: nice + 1, ..DEFAULTS }, None);
    let _fakes = Fakes::serve(&bus, fake.clone());
    let ticket = endpoints_on(&bus, DEFAULT_DEADLINE).register_audio_thread().unwrap();
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(thread, 10)), "real-time");
    drop(ticket);
    assert_eq!(
        calls.try_recv(),
        Ok(Call::HighPriority(thread, nice + 1)),
        "the minimum: rtkit refuses the level the thread had, which is below it"
    );
    assert_eq!(fake.resume(), [], "granted, so the thread is off real-time and forgotten");
}

#[test]
fn a_promotion_answered_after_the_deadline_is_reverted() {
    let bus = PrivateBus::start();
    let gate = Arc::new(Notify::new());
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, Some(Gate::Realtime(Arc::clone(&gate))));
    let _fakes = Fakes::serve(&bus, fake);
    let endpoints = endpoints_on(&bus, SHORT);
    let (thread, nice) = (this_thread(), this_nice());
    assert_eq!(
        endpoints.register_audio_thread().unwrap_err(),
        PlatformError::DeadlineExceeded { call: "audio thread registration" },
        "rtkit has not answered"
    );
    gate.notify_one();
    assert_eq!(calls.blocking_recv(), Some(Call::Realtime(thread, 10)), "the request it held");
    assert_eq!(
        calls.blocking_recv(),
        Some(Call::HighPriority(thread, nice)),
        "the promotion rtkit granted after the call gave up is reverted at once"
    );
}

#[test]
fn a_late_promotion_is_reverted_after_the_endpoints_drop() {
    let bus = PrivateBus::start();
    let gate = Arc::new(Notify::new());
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, Some(Gate::Realtime(Arc::clone(&gate))));
    let _fakes = Fakes::serve(&bus, fake);
    let endpoints = endpoints_on(&bus, SHORT);
    let (thread, nice) = (this_thread(), this_nice());
    assert_eq!(
        endpoints.register_audio_thread().unwrap_err(),
        PlatformError::DeadlineExceeded { call: "audio thread registration" },
        "rtkit has not answered"
    );
    assert_eq!(calls.blocking_recv(), Some(Call::Realtime(thread, 10)), "the request it holds");
    // The bridge's last handle but the promoter's, which keeps the bridge's thread running.
    drop(endpoints);
    gate.notify_one();
    assert_eq!(
        calls.blocking_recv(),
        Some(Call::HighPriority(thread, nice)),
        "the promotion rtkit granted after the call gave up is reverted all the same"
    );
}

#[test]
fn a_promotion_whose_connection_failed_is_reverted_over_the_bridges_next_one() {
    let bus = PrivateBus::start();
    let gate = Arc::new(Notify::new());
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, Some(Gate::Realtime(Arc::clone(&gate))));
    let _fakes = Fakes::serve(&bus, fake.clone());
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let settings = LinuxAudioSettings { deadline: SHORT, ..LinuxAudioSettings::default() };
    let endpoints = LinuxAudioEndpoints::new(bridge.clone(), settings);
    let (thread, nice) = (this_thread(), lower_priority());
    assert_eq!(
        endpoints.register_audio_thread().unwrap_err(),
        PlatformError::DeadlineExceeded { call: "audio thread registration" },
        "rtkit has not answered"
    );
    assert_eq!(calls.blocking_recv(), Some(Call::Realtime(thread, 10)), "the request it holds");
    // The bridge's system-bus connection fails under the request.
    let closed = bridge.call("closing the system bus", DEFAULT_DEADLINE, |buses| async move {
        let _ignored = buses.system().await?.close().await;
        Ok(())
    });
    assert_eq!(closed, Ok(()), "the promoter's connection, which the bridge shares");
    fake.report_reads.store(true, Ordering::SeqCst);
    gate.notify_one();
    assert_eq!(
        calls.blocking_recv(),
        Some(Call::HighPriority(thread, nice)),
        "the grant that came after the connection failed is reverted over a new one"
    );
    assert_eq!(calls.blocking_recv(), Some(Call::Read("RTTimeUSecMax")), "and the read after it");
    assert_eq!(fake.resume(), [], "so the thread is off real-time and forgotten");
}

#[test]
fn a_ticket_that_holds_the_bridges_last_handle_leaves_a_slow_revert_to_finish() {
    let bus = PrivateBus::start();
    let gate = Arc::new(Notify::new());
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, Some(Gate::HighPriority(Arc::clone(&gate))));
    let _fakes = Fakes::serve(&bus, fake.clone());
    let endpoints = endpoints_on(&bus, DEFAULT_DEADLINE);
    let (thread, nice) = (this_thread(), lower_priority());
    let ticket = endpoints.register_audio_thread().unwrap();
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(thread, 10)), "real-time");
    drop(endpoints);
    fake.report_reads.store(true, Ordering::SeqCst);
    // Waits REVERT_WAIT for the revert the gate holds, then drops the bridge's last handle but
    // the promoter's.
    drop(ticket);
    gate.notify_one();
    assert_eq!(calls.blocking_recv(), Some(Call::HighPriority(thread, nice)), "the revert");
    assert_eq!(
        calls.blocking_recv(),
        Some(Call::Read("RTTimeUSecMax")),
        "and the read after it, which only a promoter that outlived the ticket's wait sends"
    );
    assert_eq!(fake.resume(), [], "so the thread is off real-time and forgotten");
}

#[test]
fn the_endpoint_methods_answer_unavailable() {
    let bus = PrivateBus::start();
    let endpoints = endpoints_on(&bus, DEFAULT_DEADLINE);
    for category in [StreamCategory::Speech, StreamCategory::Communications] {
        assert_eq!(
            endpoints.default_capture(category),
            Err(PlatformError::Unavailable { what: "default capture endpoint" }),
            "PipeWire's graph is not on D-Bus: {category:?}"
        );
    }
    assert_eq!(
        endpoints.default_render_loopback(),
        Err(PlatformError::Unavailable { what: "default render loopback" }),
        "nor is the default sink's monitor"
    );
}

#[test]
fn opting_out_of_ducking_succeeds_and_opting_in_is_unavailable() {
    let bus = PrivateBus::start();
    let endpoints = endpoints_on(&bus, DEFAULT_DEADLINE);
    assert_eq!(endpoints.set_communications_ducking(true), Ok(()), "nothing ducks for Eileen");
    assert_eq!(
        endpoints.set_communications_ducking(false),
        Err(PlatformError::Unavailable { what: "communications ducking" }),
        "no ducking of the OS's to opt into"
    );
}
