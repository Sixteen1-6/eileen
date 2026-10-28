//! The calling thread's kernel id, nice level and scheduling policy, and the bound on the
//! process's `RLIMIT_RTTIME` that `RealtimeKit` asks for before it makes a thread real-time.
//!
//! Neither std nor rustix has `sched_getscheduler` or `sched_getparam`, and the crate has no
//! unsafe code, so the policy comes from `/proc/self/task/<tid>/stat`, whose fields are stable
//! ABI (`proc_pid_stat(5)`) and give the real-time priority with the policy. The `policy` line of
//! `/proc/<pid>/sched` was the other choice: a scheduler debugging file whose format the kernel
//! does not keep stable, and which has no real-time priority. The read needs no bound of its own
//! (BUILD.md section 2.1 rule 2 bounds calls to another process): procfs makes the file in memory
//! from the thread's own task, under short kernel locks that no other process can hold, as
//! `/proc/<pid>/cmdline` can by holding its memory map's lock.
//!
//! `RLIMIT_RTTIME` counts the CPU time a thread under a real-time policy uses without a blocking
//! system call. Past the soft limit the process gets `SIGXCPU`, whose default action ends it,
//! and `SIGKILL` at the hard limit, which comes first when the two are equal. Linux's default is
//! unlimited, and `rtkit-daemon` refuses `MakeThreadRealtime` unless the hard limit in
//! `/proc/<pid>/limits` is a number no larger than its `RTTimeUSecMax` (`verify_process_rttime`
//! in rtkit 0.13 and 0.14).

use eileen_platform::PlatformError;
use rustix::io::Errno;
use rustix::process::{Resource, Rlimit, getpriority_process, getrlimit, setrlimit};
use rustix::thread::Pid;

/// What a failed read of a thread's scheduling names.
const STAT: &str = "/proc/self/task/<tid>/stat";

/// The fields of a `stat` file that say how a thread is scheduled, counted from 1 as
/// `proc_pid_stat(5)` counts them: 40, `rt_priority`, then 41, `policy`. Field 2 is the name in
/// parentheses, so field 3 is the first after it.
const RT_PRIORITY: usize = 40;
const AFTER_NAME: usize = 3;

/// A thread's kernel id, `gettid`, as `RealtimeKit` names threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(Pid);

impl ThreadId {
    /// The calling thread's.
    #[must_use]
    pub fn current() -> Self {
        Self(rustix::thread::gettid())
    }

    /// The id as `RealtimeKit`'s methods take it.
    #[must_use]
    pub fn get(self) -> u64 {
        u64::from(self.0.as_raw_pid().unsigned_abs())
    }

    /// The thread's nice level: `getpriority(PRIO_PROCESS, tid)`, since Linux keeps one per
    /// thread.
    pub fn nice(self) -> Result<i32, PlatformError> {
        getpriority_process(Some(self.0)).map_err(|errno| os_error("getpriority", errno))
    }

    /// How the kernel schedules the thread, from `/proc/self/task/<tid>/stat` (the module's doc
    /// says why that file, and why the read has no bound of its own). A thread of another process
    /// is not under `/proc/self`, and reads as [`PlatformError::Io`].
    pub fn scheduling(self) -> Result<Scheduling, PlatformError> {
        let path = format!("/proc/self/task/{}/stat", self.get());
        let stat = std::fs::read(path).map_err(|error| PlatformError::io(STAT, &error))?;
        parse_stat(&stat).map_err(|detail| PlatformError::Invalid { what: STAT, detail })
    }
}

/// How the kernel schedules a thread: its policy, by `sched_setscheduler`'s names and numbers,
/// with its real-time priority where it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheduling {
    /// `SCHED_OTHER` (0), every thread's default: by nice level.
    Other,
    /// `SCHED_FIFO` (1).
    Fifo {
        /// Its real-time priority, from 1 to 99.
        priority: u32,
    },
    /// `SCHED_RR` (2), which `RealtimeKit` grants.
    RoundRobin {
        /// Its real-time priority, from 1 to 99.
        priority: u32,
    },
    /// `SCHED_DEADLINE` (6), which runs before every `SCHED_FIFO` and `SCHED_RR` thread.
    Deadline,
    /// Any other policy, none of them real-time: `SCHED_BATCH` (3), `SCHED_IDLE` (5),
    /// `SCHED_EXT` (7, Linux 6.12), or one a later kernel adds.
    Another {
        /// The policy's number.
        policy: u32,
    },
}

impl Scheduling {
    /// The policy numbered `policy` at real-time priority `priority`, which only `SCHED_FIFO`
    /// and `SCHED_RR` have (the kernel reports 0 for the others).
    #[must_use]
    pub const fn new(policy: u32, priority: u32) -> Self {
        match policy {
            0 => Self::Other,
            1 => Self::Fifo { priority },
            2 => Self::RoundRobin { priority },
            6 => Self::Deadline,
            policy => Self::Another { policy },
        }
    }

    /// Whether the policy is a real-time one: `SCHED_FIFO`, `SCHED_RR` or `SCHED_DEADLINE`.
    #[must_use]
    pub const fn is_realtime(self) -> bool {
        matches!(self, Self::Fifo { .. } | Self::RoundRobin { .. } | Self::Deadline)
    }
}

/// The scheduling a `stat` file's text gives, from its fields 40 and 41. Field 2, the thread's
/// name in parentheses, may hold any byte, spaces and parentheses included, but no field after it
/// holds a `)`, so they are counted from the last one. The error says what is missing.
pub(crate) fn parse_stat(stat: &[u8]) -> Result<Scheduling, String> {
    let name_end = stat.iter().rposition(|&byte| byte == b')').ok_or("no name in parentheses")?;
    let after_name = stat.get(name_end + 1..).unwrap_or_default();
    let after_name = std::str::from_utf8(after_name)
        .map_err(|_not_utf8| "fields after the name that are not text".to_owned())?;
    let mut fields = after_name.split_ascii_whitespace().skip(RT_PRIORITY - AFTER_NAME);
    let mut number = |field: usize| match fields.next() {
        Some(text) => text.parse::<u32>().map_err(|_nan| format!("field {field}, {text:?}")),
        None => Err(format!("no field {field}")),
    };
    let priority = number(RT_PRIORITY)?;
    let policy = number(RT_PRIORITY + 1)?;
    Ok(Scheduling::new(policy, priority))
}

/// Bounds the process's `RLIMIT_RTTIME` to at most `max_us` microseconds, as [`bounded`] says,
/// with `setrlimit` when that changes it. The bound is process-wide, the processes started after
/// it inherit it, and it is permanent: raising a hard limit again needs `CAP_SYS_RESOURCE`.
pub(crate) fn bound_rttime(max_us: u64) -> Result<(), PlatformError> {
    match bounded(getrlimit(Resource::Rttime), max_us) {
        Some(limit) => {
            setrlimit(Resource::Rttime, limit).map_err(|errno| os_error("setrlimit", errno))
        }
        None => Ok(()),
    }
}

/// The limit `current` becomes, bounded to `max`: the hard limit at most `max` (an unlimited one
/// becomes `max`), and the soft limit at most the hard one (an unlimited one becomes the hard
/// one). `None` when that changes nothing. Neither limit ever rises.
pub(crate) fn bounded(current: Rlimit, max: u64) -> Option<Rlimit> {
    let hard = current.maximum.map_or(max, |hard| hard.min(max));
    let soft = current.current.map_or(hard, |soft| soft.min(hard));
    let limit = Rlimit { current: Some(soft), maximum: Some(hard) };
    (limit != current).then_some(limit)
}

/// The error of the OS call `call`, with its `errno`.
fn os_error(call: &'static str, errno: Errno) -> PlatformError {
    PlatformError::Os { call, code: i64::from(errno.raw_os_error()) }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rustix::process::Rlimit;

    use super::{Scheduling, bounded, parse_stat};

    /// rtkit's default `RTTimeUSecMax`, 200 ms.
    const MAX: u64 = 200_000;

    /// Whether `limit` is at most `bound`, `None` being unlimited.
    fn at_most(limit: Option<u64>, bound: Option<u64>) -> bool {
        match (limit, bound) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(limit), Some(bound)) => limit <= bound,
        }
    }

    #[test]
    fn an_unlimited_hard_limit_is_lowered_to_the_maximum() {
        let unlimited = Rlimit { current: None, maximum: None };
        assert_eq!(
            bounded(unlimited, MAX),
            Some(Rlimit { current: Some(MAX), maximum: Some(MAX) }),
            "Linux's default: both limits become the maximum"
        );
        let soft = Rlimit { current: Some(50_000), maximum: None };
        assert_eq!(
            bounded(soft, MAX),
            Some(Rlimit { current: Some(50_000), maximum: Some(MAX) }),
            "a lower soft limit stays"
        );
        let within = Rlimit { current: Some(50_000), maximum: Some(100_000) };
        assert_eq!(bounded(within, MAX), None, "already within: nothing to set");
    }

    /// `stat` lines read on Ubuntu 24.04 under WSL2 (Linux 6.18): `cat` as it was started, under
    /// `chrt -b 0` and under `chrt -i 0`, a copy of it named `a) (b c)`, and a thread of a
    /// running program whose name has spaces.
    const OTHER: &str = "4762 (cat) R 4761 4762 4761 0 -1 4194304 102 0 1 0 0 0 0 0 20 0 1 0 64090 \
        3362816 417 18446744073709551615 109562377342976 109562377360561 140732007719200 0 0 0 0 0 \
        0 0 0 0 17 4 0 0 0 0 0 109562377374352 109562377375848 109562832474112 140732007722499 \
        140732007722519 140732007722519 140732007727083 0\n";
    const BATCH: &str = "6705 (cat) R 6704 6705 6704 0 -1 4194304 179 0 1 0 0 0 0 0 20 0 1 0 75857 \
        3362816 416 18446744073709551615 104712213430272 104712213447857 140725172750288 0 0 0 0 0 \
        0 0 0 0 17 4 0 3 0 0 0 104712213461648 104712213463144 104712870686720 140725172751874 \
        140725172751894 140725172751894 140725172756459 0\n";
    const IDLE: &str = "6706 (cat) R 6704 6706 6704 0 -1 4194304 180 0 0 0 0 0 0 0 20 0 1 0 75857 \
        3362816 417 18446744073709551615 98073544179712 98073544197297 140722022775824 0 0 0 0 0 0 \
        0 0 0 17 6 0 5 0 0 0 98073544211088 98073544212584 98073962049536 140722022784514 \
        140722022784534 140722022784534 140722022789099 0\n";
    const PARENTHESES: &str = "6710 (a) (b c)) R 6704 6710 6704 0 -1 4194304 101 0 0 0 0 0 0 0 20 \
        0 1 0 75858 3362816 401 18446744073709551615 108833630445568 108833630463153 \
        140731445380576 0 0 0 0 0 0 0 0 0 17 9 0 0 0 0 0 108833630476944 108833630478440 \
        108833837305856 140731445382410 140731445382519 140731445382519 140731445387163 0\n";
    const SPACES: &str = "2595 (s Signal Sender) S 180 109 109 34817 109 4194368 0 576072 0 312 0 \
        0 424 314 20 0 26 0 42204 5812989952 95901 18446744073709551615 29231616 115312736 \
        140735781683216 0 0 0 0 4096 1937796351 1 0 0 -1 10 0 0 0 0 0 115316832 295825408 \
        648056832 140735781690685 140735781691083 140735781691083 140735781695371 0\n";

    /// [`OTHER`] as the kernel writes it for a thread under `policy` at real-time priority
    /// `priority`: field 18 is then `-1 - priority`, 40 the priority and 41 the policy. A test
    /// cannot make a thread real-time without privilege, so none was read.
    fn realtime(policy: u32, priority: u32) -> String {
        let mut fields: Vec<String> = OTHER.split_ascii_whitespace().map(str::to_owned).collect();
        fields[17] = format!("-{}", priority + 1);
        fields[39] = priority.to_string();
        fields[40] = policy.to_string();
        fields.join(" ")
    }

    #[test]
    fn real_stat_lines_give_their_policy_and_priority() {
        let cases = [
            (OTHER.to_owned(), Scheduling::Other),
            (BATCH.to_owned(), Scheduling::Another { policy: 3 }),
            (IDLE.to_owned(), Scheduling::Another { policy: 5 }),
            (PARENTHESES.to_owned(), Scheduling::Other),
            (SPACES.to_owned(), Scheduling::Other),
            (realtime(1, 5), Scheduling::Fifo { priority: 5 }),
            (realtime(2, 20), Scheduling::RoundRobin { priority: 20 }),
            (realtime(6, 0), Scheduling::Deadline),
        ];
        for (line, scheduling) in cases {
            assert_eq!(parse_stat(line.as_bytes()), Ok(scheduling), "{line}");
        }
        let realtime = [Scheduling::Fifo { priority: 5 }, Scheduling::RoundRobin { priority: 20 }];
        for scheduling in realtime.into_iter().chain([Scheduling::Deadline]) {
            assert!(scheduling.is_realtime(), "{scheduling:?}");
        }
        for policy in [0, 3, 5, 7] {
            assert!(!Scheduling::new(policy, 0).is_realtime(), "policy {policy}");
        }
    }

    #[test]
    fn a_line_without_its_name_or_its_fields_is_invalid() {
        let fields: Vec<&str> = OTHER.split_ascii_whitespace().collect();
        let cases = [
            (String::new(), "no name in parentheses".to_owned()),
            ("4762 cat R 4761".to_owned(), "no name in parentheses".to_owned()),
            (fields[..39].join(" "), "no field 40".to_owned()),
            (fields[..40].join(" "), "no field 41".to_owned()),
            (realtime(1, 5).replace(" 5 1 ", " 5 x "), "field 41, \"x\"".to_owned()),
        ];
        for (line, detail) in cases {
            assert_eq!(parse_stat(line.as_bytes()), Err(detail), "{line}");
        }
    }

    /// Where field 40 sits among the fields after the name, the first of which is field 3.
    const RT_PRIORITY_INDEX: usize = 40 - 3;

    /// A thread's name: up to 15 bytes, as the kernel keeps, often spaces and parentheses.
    fn name() -> impl Strategy<Value = Vec<u8>> {
        let byte = prop_oneof![Just(b')'), Just(b'('), Just(b' '), Just(b'\n'), any::<u8>()];
        proptest::collection::vec(byte, 0..=15)
    }

    proptest! {
        #[test]
        fn the_policy_is_read_after_the_name_whatever_the_name_holds(
            name in name(),
            policy in 0..=7_u32,
            priority in 0..=99_u32,
            numbers in proptest::collection::vec(any::<i64>(), 52 - 2),
        ) {
            let mut fields: Vec<String> = numbers.iter().map(i64::to_string).collect();
            fields[RT_PRIORITY_INDEX] = priority.to_string();
            fields[RT_PRIORITY_INDEX + 1] = policy.to_string();
            let mut line = b"4762 (".to_vec();
            line.extend_from_slice(&name);
            line.extend_from_slice(b") ");
            line.extend_from_slice(fields.join(" ").as_bytes());
            line.push(b'\n');
            prop_assert_eq!(parse_stat(&line), Ok(Scheduling::new(policy, priority)));
        }
    }

    proptest! {
        #[test]
        fn the_bounded_limit_never_rises_and_keeps_soft_within_hard(
            soft in proptest::option::of(any::<u64>()),
            hard in proptest::option::of(any::<u64>()),
            max in 1..=u64::MAX,
        ) {
            let current = Rlimit { current: soft, maximum: hard };
            let limit = bounded(current, max).unwrap_or(current);
            prop_assert!(limit.maximum.is_some_and(|hard| hard <= max), "within RTTimeUSecMax");
            prop_assert!(at_most(limit.current, limit.maximum), "soft within hard: {:?}", limit);
            prop_assert!(at_most(limit.maximum, current.maximum), "the hard limit never rises");
            prop_assert!(at_most(limit.current, current.current), "the soft limit never rises");
        }
    }
}
