//! The `RealtimeKit` client against a fake rtkit on the private bus. Like rtkit, the fake refuses
//! `MakeThreadRealtime` past its maximum priority, or unless the caller's hard `RLIMIT_RTTIME` is
//! at most its `RTTimeUSecMax`, which it reads in the test's own process. It keeps rtkit 0.14's
//! record of each thread: the real-time priority it last granted, which `MakeThreadHighPriority`
//! leaves, forgotten at the start of a later call once the thread is neither real-time nor at a
//! negative nice level, and made real-time again by [`FakeRtkit::resume`], as 0.14 does after a
//! suspend. And like rtkit, whose one thread serves every call in turn and waits for polkit inside
//! a call, the fake serves its methods in order: a call it holds keeps every later one waiting.
//! zbus answers `Properties.GetAll` for it, which rtkit 0.13 does not, so the fake counts the
//! reads that come that way.

use std::collections::BTreeMap;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use eileen_audio_linux::{
    DbusError, Promoted, Promotion, RealtimeKit, RealtimePolicy, Scheduling, ThreadId,
};
use eileen_dbus_linux::METHOD_TIMEOUT;
use eileen_dbus_linux::testing::{PrivateBus, serve};
use rustix::process::{Resource, getrlimit};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{Notify, mpsc};
use zbus::message::Header;

/// rtkit's name and object.
pub(super) const RTKIT: &str = "org.freedesktop.RealtimeKit1";
pub(super) const RTKIT_PATH: &str = "/org/freedesktop/RealtimeKit1";

/// rtkit's defaults: 200 ms of real-time CPU, priority 20 and nice -15.
pub(super) const DEFAULTS: Limits =
    Limits { rttime_usec_max: 200_000, max_realtime_priority: 20, min_nice_level: -15 };

/// The nice level the client's tests say their thread has: not 0, so that a revert to nice 0
/// cannot pass for one to it.
const NICE: i32 = 3;

/// The policy the client's tests say their thread has, as a read of it gives: the default,
/// `SCHED_OTHER`, which the test's thread has too.
const OTHER: Option<Scheduling> = Some(Scheduling::Other);

/// A call the fake reports, refused or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Call {
    /// `MakeThreadRealtime(thread, priority)`.
    Realtime(u64, u32),
    /// `MakeThreadHighPriority(thread, nice level)`.
    HighPriority(u64, i32),
    /// A read of the property named, reported once [`FakeRtkit::report_reads`] is set.
    Read(&'static str),
}

/// The call the fake holds until its gate opens, and the gate.
#[derive(Debug, Clone)]
pub(super) enum Gate {
    /// `MakeThreadRealtime`.
    Realtime(Arc<Notify>),
    /// `MakeThreadHighPriority`.
    HighPriority(Arc<Notify>),
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
enum Scheduled {
    Realtime(u32),
    Nice(i32),
}

/// What rtkit 0.14 keeps of a thread: the real-time priority it last granted, and how the thread
/// is scheduled now.
#[derive(Debug, Clone, Copy)]
struct Record {
    priority: Option<u32>,
    now: Scheduled,
}

/// A fake rtkit that reports each `MakeThread` call, and each property read once asked to. It
/// answers the call its gate holds, if it has one, once the gate opens, and refuses either call
/// as its switches say.
#[derive(Debug, Clone)]
pub(super) struct FakeRtkit {
    limits: Arc<Mutex<Limits>>,
    pub(super) refuse_realtime: Arc<AtomicBool>,
    /// Fails `MakeThreadRealtime` with `org.freedesktop.DBus.Error.Failed`, rtkit's answer for
    /// any error but a refusal (`EPERM`, `EACCES`, `EBUSY`) or `ENOMEM`.
    pub(super) fail_realtime: Arc<AtomicBool>,
    pub(super) refuse_high_priority: Arc<AtomicBool>,
    /// Reports each property read as a [`Call::Read`], after its garbage collection.
    pub(super) report_reads: Arc<AtomicBool>,
    gate: Option<Gate>,
    calls: mpsc::Sender<Call>,
    records: Arc<Mutex<BTreeMap<u64, Record>>>,
    /// How many property reads came through `GetAll`, one for each property it read.
    get_alls: Arc<AtomicUsize>,
}

impl FakeRtkit {
    /// A fake reporting `limits`, behind `gate`, with the receiver of its reports.
    pub(super) fn new(limits: Limits, gate: Option<Gate>) -> (Self, mpsc::Receiver<Call>) {
        let (calls, reports) = mpsc::channel(16);
        let fake = Self {
            limits: Arc::new(Mutex::new(limits)),
            refuse_realtime: Arc::default(),
            fail_realtime: Arc::default(),
            refuse_high_priority: Arc::default(),
            report_reads: Arc::default(),
            gate,
            calls,
            records: Arc::default(),
            get_alls: Arc::default(),
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
            record.now = Scheduled::Realtime(priority);
            Some((thread, priority))
        });
        let resumed = resumed.collect();
        drop(records);
        resumed
    }

    /// Records `thread` as real-time at `priority`, as rtkit records a thread it made so: one that
    /// registered before, or one that another library made real-time through rtkit.
    pub(super) fn grant(&self, thread: u64, priority: u32) {
        let record = Record { priority: Some(priority), now: Scheduled::Realtime(priority) };
        self.records.lock().unwrap().insert(thread, record);
    }

    /// How the fake has scheduled `thread`, if it records it.
    fn now(&self, thread: u64) -> Option<Scheduled> {
        self.records.lock().unwrap().get(&thread).map(|record| record.now)
    }

    /// rtkit 0.14's garbage collection, at the start of each call to its object: it forgets the
    /// threads that are neither real-time nor at a negative nice level.
    fn collect(&self) {
        let mut records = self.records.lock().unwrap();
        records.retain(|_thread, record| !matches!(record.now, Scheduled::Nice(nice) if nice >= 0));
    }

    /// How many property reads came through `GetAll`.
    pub(super) fn get_alls(&self) -> usize {
        self.get_alls.load(Ordering::SeqCst)
    }

    /// What each property read does first: the garbage collection, then the count of a read
    /// through `GetAll`, as `header` says, or the report of one through `Get` once
    /// [`Self::report_reads`] is set.
    async fn read(&self, property: &'static str, header: Option<&Header<'_>>) {
        self.collect();
        if header.and_then(Header::member).is_some_and(|member| member.as_str() == "GetAll") {
            self.get_alls.fetch_add(1, Ordering::SeqCst);
        } else if self.report_reads.load(Ordering::SeqCst) {
            let _ignored = self.calls.send(Call::Read(property)).await;
        }
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

/// Served in order (`spawn = false`): each call ends before the next begins, as in rtkit.
#[zbus::interface(name = "org.freedesktop.RealtimeKit1", spawn = false)]
impl FakeRtkit {
    #[zbus(name = "MakeThreadRealtime")]
    async fn make_thread_realtime(&self, thread: u64, priority: u32) -> zbus::fdo::Result<()> {
        self.collect();
        let _ignored = self.calls.send(Call::Realtime(thread, priority)).await;
        if let Some(Gate::Realtime(gate)) = &self.gate {
            gate.notified().await;
        }
        if self.fail_realtime.load(Ordering::SeqCst) {
            return Err(zbus::fdo::Error::Failed("Input/output error".to_owned()));
        }
        let limits = self.limits();
        let within = i64::from(priority) <= i64::from(limits.max_realtime_priority);
        if self.refuse_realtime.load(Ordering::SeqCst)
            || !within
            || !rttime_bounded(limits.rttime_usec_max)
        {
            return Err(refused());
        }
        let record = Record { priority: Some(priority), now: Scheduled::Realtime(priority) };
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
        if let Some(Gate::HighPriority(gate)) = &self.gate {
            gate.notified().await;
        }
        if self.refuse_high_priority.load(Ordering::SeqCst)
            || nice_level < self.limits().min_nice_level
        {
            return Err(refused());
        }
        let fresh = Record { priority: None, now: Scheduled::Nice(0) };
        self.records.lock().unwrap().entry(thread).or_insert(fresh).now =
            Scheduled::Nice(nice_level);
        Ok(())
    }

    #[zbus(property, name = "RTTimeUSecMax")]
    async fn rttime_usec_max(&self, #[zbus(header)] header: Option<Header<'_>>) -> i64 {
        self.read("RTTimeUSecMax", header.as_ref()).await;
        self.limits().rttime_usec_max
    }

    #[zbus(property, name = "MaxRealtimePriority")]
    async fn max_realtime_priority(&self, #[zbus(header)] header: Option<Header<'_>>) -> i32 {
        self.read("MaxRealtimePriority", header.as_ref()).await;
        self.limits().max_realtime_priority
    }

    #[zbus(property, name = "MinNiceLevel")]
    async fn min_nice_level(&self, #[zbus(header)] header: Option<Header<'_>>) -> i32 {
        self.read("MinNiceLevel", header.as_ref()).await;
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

/// A fresh connection to `bus`, as `promote` asks for one after its connection failed.
async fn fresh(bus: &PrivateBus) -> Result<zbus::Connection, DbusError> {
    Ok(bus.connect().await)
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
async fn every_property_is_read_alone_since_rtkit_0_13_answers_no_get_all() {
    let bus = PrivateBus::start();
    let (fake, _calls) = FakeRtkit::new(DEFAULTS, None);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake.clone()).await;
    let client = client(&bus).await;
    let thread = ThreadId::current();
    client.limits().await.unwrap();
    let promoted = client
        .promote(thread, NICE, OTHER, RealtimePolicy::default(), || fresh(&bus))
        .await
        .unwrap();
    client.revert(thread, promoted.restore).await.unwrap();
    assert_eq!(
        fake.get_alls(),
        0,
        "each through Properties.Get: zbus's property cache would call GetAll before its first read"
    );
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
    let client = client(&bus).await;
    let promoted = client
        .promote(ThreadId::current(), NICE, OTHER, RealtimePolicy::default(), || fresh(&bus))
        .await;
    assert_eq!(
        promoted,
        Ok(Promoted { promotion: Promotion::Realtime { priority: 8 }, restore: NICE }),
        "10, capped at the maximum"
    );
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
    let client = client(&bus).await;
    let promoted = client
        .promote(ThreadId::current(), NICE, OTHER, RealtimePolicy::default(), || fresh(&bus))
        .await;
    assert_eq!(
        promoted,
        Ok(Promoted { promotion: Promotion::HighPriority { nice: -8 }, restore: NICE }),
        "-11, floored at the minimum"
    );
    let thread = this_thread();
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(thread, 10)), "real-time first");
    assert_eq!(calls.try_recv(), Ok(Call::HighPriority(thread, -8)), "then high priority");
}

#[tokio::test]
async fn a_refused_request_never_lowers_a_thread_that_already_runs_higher() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, None);
    fake.refuse_realtime.store(true, Ordering::SeqCst);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake).await;
    let client = client(&bus).await;
    let thread = this_thread();
    // Below rtkit's minimum, -15; between it and the fallback, -11; and the fallback itself.
    for (nice, restore) in [(-16, -15), (-13, -13), (-11, -11)] {
        let policy = RealtimePolicy::default();
        let promoted =
            client.promote(ThreadId::current(), nice, OTHER, policy, || fresh(&bus)).await;
        assert_eq!(
            promoted,
            Ok(Promoted { promotion: Promotion::Unchanged { nice }, restore }),
            "nice {nice} is already the fallback's level or higher"
        );
        assert_eq!(calls.try_recv(), Ok(Call::Realtime(thread, 10)), "real-time asked: {nice}");
        assert_eq!(
            calls.try_recv(),
            Err(TryRecvError::Empty),
            "and no fallback, which would lower nice {nice} to -11 (or to the minimum)"
        );
    }
}

/// The real-time policies a read can report, at priorities below, at and above the default
/// policy's 10, which a test cannot give its thread without privilege.
const REALTIME: [Scheduling; 4] = [
    Scheduling::Fifo { priority: 5 },
    Scheduling::RoundRobin { priority: 10 },
    Scheduling::RoundRobin { priority: 20 },
    Scheduling::Deadline,
];

#[tokio::test]
async fn a_thread_already_real_time_is_asked_nothing_and_keeps_its_policy_when_rtkit_refuses() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, None);
    fake.refuse_realtime.store(true, Ordering::SeqCst);
    fake.report_reads.store(true, Ordering::SeqCst);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake.clone()).await;
    let client = client(&bus).await;
    let thread = this_thread();
    // rtkit's record of a thread that registered before; whatever policy the read reports, rtkit
    // hears nothing, so the record stays.
    fake.grant(thread, 10);
    for scheduling in REALTIME {
        let policy = RealtimePolicy::default();
        let promoted =
            client.promote(ThreadId::current(), NICE, Some(scheduling), policy, || fresh(&bus));
        assert_eq!(
            promoted.await,
            Ok(Promoted { promotion: Promotion::Unchanged { nice: NICE }, restore: NICE }),
            "{scheduling:?} is left as it is, with nothing to revert"
        );
        assert_eq!(
            calls.try_recv(),
            Err(TryRecvError::Empty),
            "no call at all, a limit's read included, so no fallback to SCHED_OTHER: \
             {scheduling:?}"
        );
        assert_eq!(fake.now(thread), Some(Scheduled::Realtime(10)), "still real-time");
    }
}

#[tokio::test]
async fn a_thread_already_real_time_below_the_asked_priority_is_not_raised() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, None);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake).await;
    let client = client(&bus).await;
    let scheduling = Some(Scheduling::Fifo { priority: 5 });
    let promoted = client
        .promote(ThreadId::current(), NICE, scheduling, RealtimePolicy::default(), || fresh(&bus))
        .await;
    assert_eq!(
        promoted.map(|promoted| promoted.promotion),
        Ok(Promotion::Unchanged { nice: NICE }),
        "a grant of 10 would make it SCHED_RR, and its revert SCHED_OTHER, not SCHED_FIFO at 5"
    );
    assert_eq!(calls.try_recv(), Err(TryRecvError::Empty), "nothing asked");
}

#[tokio::test]
async fn a_thread_not_real_time_or_of_unknown_policy_is_promoted_as_before() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, None);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake).await;
    let client = client(&bus).await;
    let thread = ThreadId::current();
    let batch = Scheduling::Another { policy: 3 };
    let idle = Scheduling::Another { policy: 5 };
    // None: a read that failed, which asks as for any thread that is not real-time.
    for scheduling in [Some(Scheduling::Other), Some(batch), Some(idle), None] {
        let policy = RealtimePolicy::default();
        let promoted = client.promote(thread, NICE, scheduling, policy, || fresh(&bus)).await;
        assert_eq!(
            promoted,
            Ok(Promoted { promotion: Promotion::Realtime { priority: 10 }, restore: NICE }),
            "{scheduling:?}"
        );
        assert_eq!(calls.try_recv(), Ok(Call::Realtime(this_thread(), 10)), "{scheduling:?}");
        client.revert(thread, NICE).await.unwrap();
        assert_eq!(calls.try_recv(), Ok(Call::HighPriority(this_thread(), NICE)), "reverted");
    }
}

#[tokio::test]
async fn a_real_time_request_that_fails_otherwise_answers_its_error_with_no_fallback() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, None);
    fake.fail_realtime.store(true, Ordering::SeqCst);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake).await;
    let client = client(&bus).await;
    let promoted = client
        .promote(ThreadId::current(), NICE, OTHER, RealtimePolicy::default(), || fresh(&bus))
        .await;
    assert_eq!(
        promoted,
        Err(DbusError::Invalid {
            what: "org.freedesktop.RealtimeKit1.MakeThreadRealtime",
            detail: "org.freedesktop.DBus.Error.Failed".to_owned(),
        }),
        "rtkit's Failed, which is no refusal"
    );
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(this_thread(), 10)), "real-time asked");
    assert_eq!(
        calls.try_recv(),
        Err(TryRecvError::Empty),
        "and nothing after: rtkit answered, so neither a fallback nor a revert"
    );
}

#[tokio::test]
async fn refusing_both_is_refused_not_authorized() {
    let bus = PrivateBus::start();
    let (fake, _calls) = FakeRtkit::new(DEFAULTS, None);
    fake.refuse_realtime.store(true, Ordering::SeqCst);
    fake.refuse_high_priority.store(true, Ordering::SeqCst);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake).await;
    let client = client(&bus).await;
    let promoted = client
        .promote(ThreadId::current(), NICE, OTHER, RealtimePolicy::default(), || fresh(&bus))
        .await;
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
    let client = client(&bus).await;
    let promoted = client
        .promote(ThreadId::current(), NICE, OTHER, RealtimePolicy::default(), || fresh(&bus))
        .await;
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
    let promoted = client
        .promote(thread, NICE, OTHER, RealtimePolicy::default(), || fresh(&bus))
        .await
        .unwrap();
    assert_eq!(
        fake.resume(),
        [(this_thread(), 10)],
        "recorded while real-time, as rtkit 0.14 does"
    );
    client.revert(thread, promoted.restore).await.unwrap();
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(this_thread(), 10)), "the promotion");
    assert_eq!(calls.try_recv(), Ok(Call::HighPriority(this_thread(), NICE)), "the revert");
    assert_eq!(fake.resume(), [], "forgotten, so a resume after a suspend leaves it alone");
}

#[tokio::test]
async fn the_revert_asks_for_the_nice_level_the_thread_had_floored_at_the_minimum() {
    let bus = PrivateBus::start();
    let (fake, mut calls) = FakeRtkit::new(Limits { min_nice_level: NICE + 2, ..DEFAULTS }, None);
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake.clone()).await;
    let client = client(&bus).await;
    let thread = ThreadId::current();
    let promoted =
        client.promote(thread, NICE, OTHER, RealtimePolicy::default(), || fresh(&bus)).await;
    assert_eq!(
        promoted,
        Ok(Promoted { promotion: Promotion::Realtime { priority: 10 }, restore: NICE + 2 }),
        "rtkit refuses any level below its minimum, the thread's own included"
    );
    client.revert(thread, NICE + 2).await.unwrap();
    assert_eq!(calls.try_recv(), Ok(Call::Realtime(this_thread(), 10)), "the promotion");
    assert_eq!(calls.try_recv(), Ok(Call::HighPriority(this_thread(), NICE + 2)), "the revert");
    assert_eq!(fake.resume(), [], "granted, so the thread is off real-time and forgotten");
}

#[tokio::test]
async fn a_request_answered_after_its_method_timeout_is_reverted_behind_it() {
    let bus = PrivateBus::start();
    let gate = Arc::new(Notify::new());
    // A minimum above the thread's level, which rtkit refuses: the revert must ask for the
    // minimum, or the thread stays real-time.
    let limits = Limits { min_nice_level: NICE + 2, ..DEFAULTS };
    let (fake, mut calls) = FakeRtkit::new(limits, Some(Gate::Realtime(Arc::clone(&gate))));
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake.clone()).await;
    let client = RealtimeKit::new(&bus.connect_within(METHOD_TIMEOUT).await);
    let mut promoted = pin!(client.promote(
        ThreadId::current(),
        NICE,
        OTHER,
        RealtimePolicy::default(),
        || fresh(&bus)
    ));
    // In real time until the fake holds the request, so the three limit reads before it have the
    // default 5 s method timeout, not a short one they must beat. From then on the fake answers
    // nothing until its gate opens, so only method timeouts are left to fire (the request's, the
    // revert's and the read's), and tokio's paused clock reaches each as soon as the runtime is
    // idle.
    tokio::select! {
        held = calls.recv() => {
            assert_eq!(held, Some(Call::Realtime(this_thread(), 10)), "the request it holds");
        }
        early = &mut promoted => panic!("promote answered before its request was held: {early:?}"),
    }
    tokio::time::pause();
    assert_eq!(
        promoted.await,
        Err(DbusError::DeadlineExceeded {
            call: "org.freedesktop.RealtimeKit1.MakeThreadRealtime"
        }),
        "rtkit holds its answer past the method timeout"
    );
    fake.report_reads.store(true, Ordering::SeqCst);
    gate.notify_one();
    assert_eq!(
        calls.recv().await,
        Some(Call::HighPriority(this_thread(), NICE + 2)),
        "the revert, to the minimum, which rtkit serves after the request it granted late"
    );
    assert_eq!(
        calls.recv().await,
        Some(Call::Read("RTTimeUSecMax")),
        "and the read after it, sent although the revert's own answer never came"
    );
    assert_eq!(fake.resume(), [], "so the thread is off real-time and forgotten");
}

#[tokio::test]
async fn a_request_whose_connection_failed_is_reverted_over_a_fresh_one() {
    let bus = PrivateBus::start();
    let gate = Arc::new(Notify::new());
    let (fake, mut calls) = FakeRtkit::new(DEFAULTS, Some(Gate::Realtime(Arc::clone(&gate))));
    let _rtkit = serve(&bus, RTKIT, RTKIT_PATH, fake.clone()).await;
    let connection = bus.connect().await;
    let rtkit = RealtimeKit::new(&connection);
    let policy = RealtimePolicy::default();
    let mut promoted =
        pin!(rtkit.promote(ThreadId::current(), NICE, OTHER, policy, || fresh(&bus)));
    tokio::select! {
        held = calls.recv() => {
            assert_eq!(held, Some(Call::Realtime(this_thread(), 10)), "the request it holds");
        }
        early = &mut promoted => panic!("promote answered before its request was held: {early:?}"),
    }
    // The connection fails under the request, which the fake already has and grants once its
    // gate opens, as rtkit may once polkit agrees.
    connection.close().await.unwrap();
    gate.notify_one();
    let promoted = promoted.await;
    assert!(
        matches!(
            &promoted,
            Err(DbusError::Io { what: "org.freedesktop.RealtimeKit1.MakeThreadRealtime", .. })
        ),
        "the failed connection's error: {promoted:?}"
    );
    // The fake serves its calls in order, so these reads come after anything promote sent.
    client(&bus).await.limits().await.unwrap();
    assert_eq!(
        calls.try_recv(),
        Ok(Call::HighPriority(this_thread(), NICE)),
        "the revert, after the grant, over a connection other than the failed one"
    );
    assert_eq!(fake.resume(), [], "so the thread is off real-time and forgotten");
}
