//! The `RealtimeKit` client against a fake rtkit on the private bus. Like rtkit, the fake refuses
//! `MakeThreadRealtime` past its maximum priority, or unless the caller's hard `RLIMIT_RTTIME` is
//! at most its `RTTimeUSecMax`, which it reads in the test's own process. It keeps rtkit 0.14's
//! record of each thread: the real-time priority it last granted, which `MakeThreadHighPriority`
//! leaves, forgotten at the start of a later call once the thread is neither real-time nor at a
//! negative nice level, and made real-time again by [`FakeRtkit::resume`], as 0.14 does after a
//! suspend.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eileen_audio_linux::{DbusError, Promotion, RealtimeKit, RealtimePolicy, ThreadId};
use eileen_dbus_linux::testing::{PrivateBus, serve};
use rustix::process::{Resource, getrlimit};
use tokio::sync::{Notify, mpsc};

/// rtkit's name and object.
pub(super) const RTKIT: &str = "org.freedesktop.RealtimeKit1";
pub(super) const RTKIT_PATH: &str = "/org/freedesktop/RealtimeKit1";

/// rtkit's defaults: 200 ms of real-time CPU, priority 20 and nice -15.
pub(super) const DEFAULTS: Limits =
    Limits { rttime_usec_max: 200_000, max_realtime_priority: 20, min_nice_level: -15 };

/// A call the fake reports, refused or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Call {
    /// `MakeThreadRealtime(thread, priority)`.
    Realtime(u64, u32),
    /// `MakeThreadHighPriority(thread, nice level)`.
    HighPriority(u64, i32),
}

/// The limits the fake reports.
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    pub(super) rttime_usec_max: i64,
    pub(super) max_realtime_priority: i32,
    pub(super) min_nice_level: i32,
}

/// How the fake has scheduled a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scheduling {
    Realtime(u32),
    Nice(i32),
}

/// What rtkit 0.14 keeps of a thread: the real-time priority it last granted, and how the thread
/// is scheduled now.
#[derive(Debug, Clone, Copy)]
struct Record {
    priority: Option<u32>,
    now: Scheduling,
}

/// A fake rtkit that reports each `MakeThread` call. It answers `MakeThreadRealtime` once its
/// gate, if it has one, opens, and refuses either call as its switches say.
#[derive(Debug, Clone)]
pub(super) struct FakeRtkit {
    limits: Arc<Mutex<Limits>>,
    pub(super) refuse_realtime: Arc<AtomicBool>,
    pub(super) refuse_high_priority: Arc<AtomicBool>,
    gate: Option<Arc<Notify>>,
    calls: mpsc::Sender<Call>,
    records: Arc<Mutex<BTreeMap<u64, Record>>>,
}

impl FakeRtkit {
    /// A fake reporting `limits`, behind `gate`, with the receiver of its reports.
    pub(super) fn new(limits: Limits, gate: Option<Arc<Notify>>) -> (Self, mpsc::Receiver<Call>) {
        let (calls, reports) = mpsc::channel(16);
        let fake = Self {
            limits: Arc::new(Mutex::new(limits)),
            refuse_realtime: Arc::default(),
            refuse_high_priority: Arc::default(),
            gate,
            calls,
            records: Arc::default(),
        };
        (fake, reports)
    }

    /// Reports `limits` from now on.
    fn set_limits(&self, limits: Limits) {
        *self.limits.lock().unwrap() = limits;
    }

    /// What rtkit 0.14 does after a suspend: makes each thread it records a priority for
    /// real-time again. Returns them, with their priorities.
    pub(super) fn resume(&self) -> Vec<(u64, u32)> {
        let mut records = self.records.lock().unwrap();
        let resumed = records.iter_mut().filter_map(|(&thread, record)| {
            let priority = record.priority?;
            record.now = Scheduling::Realtime(priority);
            Some((thread, priority))
        });
        let resumed = resumed.collect();
        drop(records);
        resumed
    }

    /// rtkit 0.14's garbage collection, at the start of each call to its object: it forgets the
    /// threads that are neither real-time nor at a negative nice level.
    fn collect(&self) {
        let mut records = self.records.lock().unwrap();
        records
            .retain(|_thread, record| !matches!(record.now, Scheduling::Nice(nice) if nice >= 0));
    }

    fn limits(&self) -> Limits {
        *self.limits.lock().unwrap()
    }
}

/// Whether this process's hard `RLIMIT_RTTIME` is a number no larger than `max`, which rtkit's
/// `verify_process_rttime` asks of the caller.
fn rttime_bounded(max: i64) -> bool {
    let hard = getrlimit(Resource::Rttime).maximum;
    hard.is_some_and(|hard| i64::try_from(hard).is_ok_and(|hard| hard <= max))
}

/// rtkit's answer to a request it refuses, for whatever reason.
fn refused() -> zbus::fdo::Error {
    zbus::fdo::Error::AccessDenied("Operation not permitted".to_owned())
}

#[zbus::interface(name = "org.freedesktop.RealtimeKit1")]
impl FakeRtkit {
    #[zbus(name = "MakeThreadRealtime")]
    async fn make_thread_realtime(&self, thread: u64, priority: u32) -> zbus::fdo::Result<()> {
        self.collect();
        let _ignored = self.calls.send(Call::Realtime(thread, priority)).await;
        if let Some(gate) = &self.gate {
            gate.notified().await;
        }
        let limits = self.limits();
        let within = i64::from(priority) <= i64::from(limits.max_realtime_priority);
        if self.refuse_realtime.load(Ordering::SeqCst)
            || !within
            || !rttime_bounded(limits.rttime_usec_max)
        {
            return Err(refused());
        }
        let record = Record { priority: Some(priority), now: Scheduling::Realtime(priority) };
        self.records.lock().unwrap().insert(thread, record);
        Ok(())
    }

    #[zbus(name = "MakeThreadHighPriority")]
    async fn make_thread_high_priority(
        &self,
        thread: u64,
        nice_level: i32,
    ) -> zbus::fdo::Result<()> {
        self.collect();
        let _ignored = self.calls.send(Call::HighPriority(thread, nice_level)).await;
        if self.refuse_high_priority.load(Ordering::SeqCst)
            || nice_level < self.limits().min_nice_level
        {
            return Err(refused());
        }
        let fresh = Record { priority: None, now: Scheduling::Nice(0) };
        self.records.lock().unwrap().entry(thread).or_insert(fresh).now =
            Scheduling::Nice(nice_level);
        Ok(())
    }

    #[zbus(property, name = "RTTimeUSecMax")]
    fn rttime_usec_max(&self) -> i64 {
        self.collect();
        self.limits().rttime_usec_max
    }

    #[zbus(property, name = "MaxRealtimePriority")]
    fn max_realtime_priority(&self) -> i32 {
        self.collect();
        self.limits().max_realtime_priority
    }

    #[zbus(property, name = "MinNiceLevel")]
    fn min_nice_level(&self) -> i32 {
        self.collect();
        self.limits().min_nice_level
    }
}

/// The calling thread's kernel id, as rtkit receives it.
pub(super) fn this_thread() -> u64 {
    u64::try_from(rustix::thread::gettid().as_raw_pid()).unwrap()
}

/// The calling thread's nice level.
pub(super) fn this_nice() -> i32 {
    rustix::process::getpriority_process(Some(rustix::thread::gettid())).unwrap()
}

/// A client on a new connection to `bus`.
async fn client(bus: &PrivateBus) -> RealtimeKit {
    RealtimeKit::new(&bus.connect().await)
}

#[tokio::test]
async fn the_limits_come_from_the_three_properties() {
    let bus = PrivateBus::start();
    let limits = Limits { rttime_usec_max: 150_000, max_realtime_priority: 8, min_nice_level: -9 };
    let (fake, _calls) = FakeRtkit::new(limits, None);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake).await;
    let read = client(&bus).await.limits().await.unwrap();
    assert_eq!(read.rttime_usec_max(), 150_000, "RTTimeUSecMax");
    assert_eq!(read.max_realtime_priority(), 8, "MaxRealtimePriority");
    assert_eq!(read.min_nice_level(), -9, "MinNiceLevel");
}

#[tokio::test]
async fn a_limit_out_of_range_is_invalid() {
    let bus = PrivateBus::start();
    let (fake, _calls) = FakeRtkit::new(DEFAULTS, None);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake.clone()).await;
    let client = client(&bus).await;
    let invalid = |what, detail: &str| DbusError::Invalid { what, detail: detail.to_owned() };
    let cases = [
        (
            Limits { rttime_usec_max: 0, ..DEFAULTS },
            invalid("org.freedesktop.RealtimeKit1.RTTimeUSecMax", "0, not positive"),
        ),
        (
            Limits { max_realtime_priority: -1, ..DEFAULTS },
            invalid("org.freedesktop.RealtimeKit1.MaxRealtimePriority", "-1, negative"),
        ),
        (
            Limits { min_nice_level: -21, ..DEFAULTS },
            invalid("org.freedesktop.RealtimeKit1.MinNiceLevel", "-21, not a nice level"),
        ),
        (
            Limits { min_nice_level: 20, ..DEFAULTS },
            invalid("org.freedesktop.RealtimeKit1.MinNiceLevel", "20, not a nice level"),
        ),
    ];
    for (limits, error) in cases {
        fake.set_limits(limits);
        assert_eq!(client.limits().await, Err(error), "{limits:?}");
    }
}

#[tokio::test]
async fn a_thread_is_made_real_time_within_the_maximum_after_the_rttime_bound() {
    let bus = PrivateBus::start();
    let limits = Limits { rttime_usec_max: 150_000, max_realtime_priority: 8, ..DEFAULTS };
    let (fake, mut calls) = FakeRtkit::new(limits, None);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake).await;
    let promoted = client(&bus).await.promote(ThreadId::current(), RealtimePolicy::default()).await;
    assert_eq!(promoted, Ok(Promotion::Realtime { priority: 8 }), "10, capped at the maximum");
    assert_eq!(
        calls.try_recv(),
        Ok(Call::Realtime(this_thread(), 8)),
        "the calling thread, by its kernel id"
    );
    let limit = getrlimit(Resource::Rttime);
    let hard = limit.maximum.unwrap();
    assert!(hard <= 150_000, "the hard limit is within RTTimeUSecMax: {limit:?}");
    assert!(limit.current.is_some_and(|soft| soft <= hard), "and the soft one within it");
}

#[tokio::test]
async fn a_refused_real_time_request_falls_back_to_high_priority() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(Limits { min_nice_level: -8, ..DEFAULTS }, None);
    fake.refuse_realtime.store(true, Ordering::SeqCst);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake).await;
    let promoted = client(&bus).await.promote(ThreadId::current(), RealtimePolicy::default()).await;
    assert_eq!(promoted, Ok(Promotion::HighPriority { nice: -8 }), "-11, floored at the minimum");
    let thread = this_thread();
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(thread, 10)), "real-time first");
    assert_eq!(calls.try_recv(), Ok(Call::HighPriority(thread, -8)), "then high priority");
}

#[tokio::test]
async fn refusing_both_is_refused_not_authorized() {
    let bus = PrivateBus::start();
    let (fake, _calls) = FakeRtkit::new(DEFAULTS, None);
    fake.refuse_realtime.store(true, Ordering::SeqCst);
    fake.refuse_high_priority.store(true, Ordering::SeqCst);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake).await;
    let promoted = client(&bus).await.promote(ThreadId::current(), RealtimePolicy::default()).await;
    assert_eq!(
        promoted,
        Err(DbusError::Refused {
            what: "org.freedesktop.RealtimeKit1.MakeThreadHighPriority",
            reason: "not authorized",
        }),
        "AccessDenied, as rtkit answers every refusal"
    );
}

#[tokio::test]
async fn no_realtimekit_on_the_bus_is_unavailable() {
    let bus = PrivateBus::start();
    let promoted = client(&bus).await.promote(ThreadId::current(), RealtimePolicy::default()).await;
    assert_eq!(
        promoted,
        Err(DbusError::Unavailable { what: "org.freedesktop.RealtimeKit1.RTTimeUSecMax" }),
        "the private bus has no activation, so the first read answers ServiceUnknown"
    );
}

#[tokio::test]
async fn a_reverted_thread_is_forgotten_so_a_resume_leaves_it_alone() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, None);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake.clone()).await;
    let client = client(&bus).await;
    let thread = ThreadId::current();
    client.promote(thread, RealtimePolicy::default()).await.unwrap();
    assert_eq!(
        fake.resume(),
        [(this_thread(), 10)],
        "recorded while real-time, as rtkit 0.14 does"
    );
    client.revert(thread, 0).await.unwrap();
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(this_thread(), 10)), "the promotion");
    assert_eq!(calls.try_recv(), Ok(Call::HighPriority(this_thread(), 0)), "the revert");
    assert_eq!(fake.resume(), [], "forgotten, so a resume after a suspend leaves it alone");
}
