//! Which daily log files a process keeps ([`TraceConfig::max_log_files`]): its own
//! `<process>.<YYYY-MM-DD>.jsonl` files, the newest by the date in the name with today's
//! counted, pruned when the stack is built and again on the first line written after each UTC
//! midnight. Names match exactly, never by prefix: the core's `eileen` begins every other
//! process's file name. Nothing here logs or prints, since it runs before the subscriber exists
//! and on the log writer's thread; a file it cannot delete stays for the next prune, and the
//! core's janitor still caps the whole folder (BUILD.md section 2.15).
//!
//! [`TraceConfig::max_log_files`]: crate::TraceConfig::max_log_files

use std::cmp::Reverse;
use std::io::{self, Write};
use std::path::PathBuf;

use eileen_contracts::ids::Date;
use eileen_stdx::clock::Clock;

/// One process's retention rule over its log folder.
#[derive(Debug, Clone)]
pub(crate) struct Retention {
    /// The log folder.
    pub(crate) dir: PathBuf,
    /// The process name, which begins its file names.
    pub(crate) process: &'static str,
    /// Daily files kept, today's included.
    pub(crate) keep: usize,
}

impl Retention {
    /// The date in `name` when it is exactly `<process>.<YYYY-MM-DD>.jsonl`.
    fn date_of(&self, name: &str) -> Option<Date> {
        name.strip_prefix(self.process)?.strip_prefix('.')?.strip_suffix(".jsonl")?.parse().ok()
    }

    /// Keeps `keep` days of the process's files, today's counted whether or not its file exists
    /// yet: of the files dated before `today` it deletes all but the newest `keep - 1` (so 0
    /// and 1 both keep today's alone). Files dated today or later stay, and so does every other
    /// name, folder and link. Answers how many it deleted; a missing folder has none.
    pub(crate) fn prune(&self, today: Date) -> io::Result<usize> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut older: Vec<(Date, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let date = self.date_of(entry.file_name().to_str()?)?;
                let is_file = entry.metadata().is_ok_and(|m| m.is_file());
                (date < today && is_file).then(|| (date, entry.path()))
            })
            .collect();
        older.sort_unstable_by_key(|(date, _)| Reverse(*date));
        let mut deleted = 0;
        for (_, path) in older.iter().skip(self.keep.saturating_sub(1)) {
            // One held open elsewhere stays for the next prune.
            if std::fs::remove_file(path).is_ok() {
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}

/// The process's daily file with its retention: it prunes when made, then on the first write
/// of each new UTC day, after the inner writer has moved to that day's file.
pub(crate) struct Retained<W> {
    inner: W,
    rule: Retention,
    clock: &'static dyn Clock,
    pruned_on: Date,
}

impl<W> Retained<W> {
    /// Wraps `inner` and prunes once, for the day `clock` reads.
    pub(crate) fn new(inner: W, rule: Retention, clock: &'static dyn Clock) -> Self {
        let today = clock.wall().date();
        let _ignored: io::Result<usize> = rule.prune(today);
        Self { inner, rule, clock, pruned_on: today }
    }
}

impl<W: Write> Write for Retained<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        let today = self.clock.wall().date();
        if today != self.pruned_on {
            self.pruned_on = today;
            let _ignored: io::Result<usize> = self.rule.prune(today);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use eileen_contracts::ids::{Date, WallTime};
    use eileen_stdx::clock::{Clock as _, ManualClock};

    use super::{Retained, Retention};

    /// A folder the test owns, removed even when it panics.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("eileen-trace-retention-{name}-{}", std::process::id()));
            let _ignored = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ignored = std::fs::remove_dir_all(&self.0);
        }
    }

    fn rule(dir: &Path, keep: usize) -> Retention {
        Retention { dir: dir.to_path_buf(), process: "eileen", keep }
    }

    /// `process`'s daily file name `back` days before `day`.
    fn daily(process: &str, day: Date, back: i64) -> String {
        format!("{process}.{}.jsonl", Date::from_days_since_epoch(day.days_since_epoch() - back))
    }

    fn touch(dir: &Path, name: &str) {
        std::fs::File::create(dir.join(name)).unwrap();
    }

    fn names(dir: &Path) -> BTreeSet<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect()
    }

    #[test]
    fn only_a_processs_exact_daily_names_carry_a_date() {
        let core = rule(Path::new("logs"), 14);
        let day = Date::new(2026, 10, 25).unwrap();
        assert_eq!(core.date_of("eileen.2026-10-25.jsonl"), Some(day), "the core's own file");
        for other in [
            "eileen-speech.2026-10-25.jsonl",
            "eileen-cli.2026-10-25.jsonl",
            "eileen.2026-10-25.jsonl.old",
            "eileen.2026-10-25-07.jsonl",
            "eileen.2026-02-30.jsonl",
            "eileen.26-10-25.jsonl",
            "eileen.jsonl",
            "eileen..jsonl",
            "Eileen.2026-10-25.jsonl",
            "old-eileen.2026-10-25.jsonl",
            "eileen.2026-10-25.json",
        ] {
            assert_eq!(core.date_of(other), None, "{other} is not one of the core's daily files");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_or_a_folder_with_a_daily_name_stays() {
        let dir = TempDir::new("links");
        let day = Date::new(2026, 10, 25).unwrap();
        touch(&dir.0, "elsewhere.txt");
        std::os::unix::fs::symlink(
            dir.0.join("elsewhere.txt"),
            dir.0.join(daily("eileen", day, 5)),
        )
        .unwrap();
        std::fs::create_dir_all(dir.0.join(daily("eileen", day, 6))).unwrap();
        touch(&dir.0, &daily("eileen", day, 7));
        assert_eq!(rule(&dir.0, 1).prune(day).unwrap(), 1, "only the plain file went");
        let left = BTreeSet::from([
            "elsewhere.txt".to_owned(),
            daily("eileen", day, 5),
            daily("eileen", day, 6),
        ]);
        assert_eq!(names(&dir.0), left, "the link and the folder stay");
    }

    #[test]
    fn a_missing_folder_has_nothing_to_prune() {
        let parent = TempDir::new("missing");
        let dir = parent.0.join("logs");
        let today = Date::new(2026, 10, 25).unwrap();
        assert_eq!(rule(&dir, 14).prune(today).unwrap(), 0, "nothing to prune");
        assert!(!dir.exists(), "and no folder made");
    }

    #[test]
    fn a_long_running_process_prunes_again_after_utc_midnight() {
        // 2026-10-25T23:59:59Z, a second before midnight.
        static CLOCK: ManualClock = ManualClock::new(WallTime(1_792_972_799_000_000));
        let day = CLOCK.wall().date();
        assert_eq!(day, Date::new(2026, 10, 25).unwrap(), "the clock's day");
        let dir = TempDir::new("midnight");
        let mut speech = BTreeSet::new();
        for back in 0..=3 {
            // Today's own file stands for the one the appender is writing.
            touch(&dir.0, &daily("eileen", day, back));
            speech.insert(daily("eileen-speech", day, back));
        }
        for name in &speech {
            touch(&dir.0, name);
        }
        let own = |dir: &Path| -> BTreeSet<String> {
            names(dir).into_iter().filter(|name| name.starts_with("eileen.")).collect()
        };

        let mut writer = Retained::new(Vec::new(), rule(&dir.0, 2), &CLOCK);
        let kept = BTreeSet::from([daily("eileen", day, 0), daily("eileen", day, 1)]);
        assert_eq!(own(&dir.0), kept, "made: today's and the newest day before it stay");

        touch(&dir.0, &daily("eileen", day, 3));
        writer.write_all(b"before midnight\n").unwrap();
        assert!(dir.0.join(daily("eileen", day, 3)).exists(), "one prune a day, not one a line");

        CLOCK.advance(Duration::from_secs(1));
        writer.write_all(b"after midnight\n").unwrap();
        // The appender opens the new day's file by its own clock, which may not have reached
        // midnight yet, so today's place counts whether or not its file exists.
        assert_eq!(
            own(&dir.0),
            BTreeSet::from([daily("eileen", day, 0)]),
            "pruned for the new day"
        );
        assert_eq!(
            names(&dir.0)
                .into_iter()
                .filter(|name| !name.starts_with("eileen."))
                .collect::<BTreeSet<_>>(),
            speech,
            "another process's files stay"
        );
        assert_eq!(writer.inner, b"before midnight\nafter midnight\n", "every line written");
    }
}
