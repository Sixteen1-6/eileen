//! Apps: COM hosts, the app index and launch, media sessions and recent files.

use eileen_contracts::desktop::{MediaState, WindowRef};
use eileen_contracts::ids::{Personal, WallTime};
use eileen_contracts::tools::apps::{ExplorerOp, FileKind, MediaAction, OfficeOp, PhotoshopOp};

use crate::{Deadline, PlatformError, RgbaImage, Subscription};

/// An operation for one app's host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppOp {
    /// Word or Excel.
    Office(OfficeOp),
    /// File Explorer.
    Explorer(ExplorerOp),
    /// Photoshop.
    Photoshop(PhotoshopOp),
}

/// A running app's process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppProcess {
    /// Its process id.
    pub pid: u32,
    /// Its version, when the host can read it.
    pub version: Option<String>,
}

/// One scriptable app (`eileen-apps-win`: Word, Excel, Explorer and Photoshop over COM, and
/// the Resolve process gate; Linux later: per app).
pub trait AppHost: Send + Sync {
    /// The app's name, for example `"word"`.
    fn app(&self) -> &'static str;
    /// Its process and version, or `None` when it is not running.
    fn running(&self) -> Result<Option<AppProcess>, PlatformError>;
    /// Runs `op` within `deadline`.
    fn call(&self, op: &AppOp, deadline: Deadline) -> Result<serde_json::Value, PlatformError>;
}

/// How an app is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppKind {
    /// A packaged app, started by its AUMID.
    Packaged,
    /// A classic desktop app, started from its path or shortcut.
    Desktop,
}

/// One app in the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// Its AUMID, or its executable's path for a desktop app.
    pub id: String,
    /// The name the Start menu shows.
    pub name: String,
    /// How it is installed.
    pub kind: AppKind,
    /// Where its executable or shortcut lives, which may be under the user's profile.
    pub path: Option<Personal<String>>,
}

/// The app index and launch.
pub trait Apps: Send + Sync {
    /// Every app the Start menu knows, read within `deadline`: the shortcuts may sit in a Start
    /// menu redirected to a server (on Linux, desktop entries under the home folder).
    fn index(&self, deadline: Deadline) -> Result<Vec<AppEntry>, PlatformError>;
    /// Starts `app` through the shell within `deadline`, since its shortcut may sit in the
    /// redirected Start menu; returns its process id.
    fn launch(&self, app: &AppEntry, deadline: Deadline) -> Result<u32, PlatformError>;
    /// The main window of `pid`, waiting up to `deadline` for it to appear.
    fn main_window(&self, pid: u32, deadline: Deadline)
    -> Result<Option<WindowRef>, PlatformError>;
    /// Opens a web address in the default browser.
    fn open_url(&self, url: &str) -> Result<(), PlatformError>;
    /// Opens a URI (a settings page, a deep link) with its registered handler.
    fn open_uri(&self, uri: &str) -> Result<(), PlatformError>;
    /// Opens a file or folder, with `with_app` when given, within `deadline`: the user's files
    /// may sit in a folder redirected to a server or on a network mount.
    fn open_path(
        &self,
        path: &str,
        with_app: Option<&AppEntry>,
        deadline: Deadline,
    ) -> Result<(), PlatformError>;
}

/// Media sessions (`eileen-apps-win`: SMTC; Linux later: MPRIS).
pub trait MediaSessions: Send + Sync {
    /// Every session the OS lists.
    fn sessions(&self) -> Result<Vec<MediaState>, PlatformError>;
    /// Sends `action` to `session`.
    fn control(&self, session: &str, action: &MediaAction) -> Result<(), PlatformError>;
    /// Calls `sink` with every change.
    fn subscribe(
        &self,
        sink: Box<dyn Fn(MediaState) + Send + Sync>,
    ) -> Result<Subscription, PlatformError>;
}

/// A recently used file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentItem {
    /// Its path.
    pub path: Personal<String>,
    /// The app that opened it, when known.
    pub app: Option<String>,
    /// What kind of file.
    pub kind: Option<FileKind>,
    /// When it was last opened.
    pub opened_at: WallTime,
}

/// Recent files. Every method reads files the user's profile may keep on another machine (the
/// Recent folder in the roaming profile, `recently-used.xbel` under a Linux home folder, the
/// files themselves), so each waits at most its `deadline`.
pub trait RecentSources: Send + Sync {
    /// The most recent `limit` items, newest first.
    fn recent(&self, limit: usize, deadline: Deadline) -> Result<Vec<RecentItem>, PlatformError>;
    /// Calls `sink` for every file opened or saved under `roots`. `deadline` bounds opening the
    /// roots, not the subscription, which lasts until it drops.
    fn watch(
        &self,
        roots: &[String],
        sink: Box<dyn Fn(RecentItem) + Send + Sync>,
        deadline: Deadline,
    ) -> Result<Subscription, PlatformError>;
    /// The shell's thumbnail of `item`, at most `edge_px` on its longer side.
    fn thumbnail(
        &self,
        item: &RecentItem,
        edge_px: u32,
        deadline: Deadline,
    ) -> Result<Option<RgbaImage>, PlatformError>;
}

#[cfg(any(test, feature = "fake"))]
pub use fake::{FakeAppHost, FakeApps, FakeMediaSessions, FakeRecentSources};

#[cfg(any(test, feature = "fake"))]
mod fake {
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use eileen_contracts::desktop::{MediaState, WindowRef};
    use eileen_contracts::tools::apps::MediaAction;
    use eileen_stdx::clock::ManualClock;

    use super::{
        AppEntry, AppHost, AppOp, AppProcess, Apps, MediaSessions, RecentItem, RecentSources,
    };
    use crate::fake_util::{FileTime, LateOperation, Sinks, lock};
    use crate::{Deadline, PlatformError, RgbaImage, Subscription};

    /// What `FakeApps::index` names when it passes its deadline.
    const INDEX: &str = "app index";
    /// What `FakeApps::launch` names when it passes its deadline.
    const LAUNCH: &str = "app launch";
    /// What `FakeApps::open_path` names when it passes its deadline.
    const OPEN_PATH: &str = "opening a file or folder";
    /// What `FakeRecentSources::recent` names when it passes its deadline.
    const RECENT: &str = "recent files";
    /// What `FakeRecentSources::watch` names when it passes its deadline.
    const WATCH: &str = "watching recent files";
    /// What `FakeRecentSources::thumbnail` names when it passes its deadline.
    const THUMBNAIL: &str = "a recent file's thumbnail";

    /// A host that answers queued replies and records every operation.
    #[derive(Debug)]
    pub struct FakeAppHost {
        app: &'static str,
        process: Mutex<Option<AppProcess>>,
        replies: Mutex<VecDeque<Result<serde_json::Value, PlatformError>>>,
        calls: Mutex<Vec<AppOp>>,
    }

    impl FakeAppHost {
        /// A host for `app`, not running.
        #[must_use]
        pub const fn new(app: &'static str) -> Self {
            Self {
                app,
                process: Mutex::new(None),
                replies: Mutex::new(VecDeque::new()),
                calls: Mutex::new(Vec::new()),
            }
        }

        /// Sets the app's process (`None`: not running).
        pub fn set_running(&self, process: Option<AppProcess>) {
            *lock(&self.process) = process;
        }

        /// Queues the answer to the next call.
        pub fn push_reply(&self, reply: Result<serde_json::Value, PlatformError>) {
            lock(&self.replies).push_back(reply);
        }

        /// The operations called, in order.
        #[must_use]
        pub fn calls(&self) -> Vec<AppOp> {
            lock(&self.calls).clone()
        }
    }

    impl AppHost for FakeAppHost {
        fn app(&self) -> &'static str {
            self.app
        }

        fn running(&self) -> Result<Option<AppProcess>, PlatformError> {
            Ok(lock(&self.process).clone())
        }

        fn call(&self, op: &AppOp, _: Deadline) -> Result<serde_json::Value, PlatformError> {
            lock(&self.calls).push(op.clone());
            lock(&self.replies)
                .pop_front()
                .unwrap_or(Err(PlatformError::Unavailable { what: "app reply" }))
        }
    }

    /// An app index in memory; launches get process ids from 1000 up. `index`, `launch` and
    /// `open_path` take no time until [`Self::set_file_time`] gives them some.
    #[derive(Debug, Default)]
    pub struct FakeApps {
        entries: Mutex<Vec<AppEntry>>,
        windows: Mutex<BTreeMap<u32, WindowRef>>,
        launched: Mutex<Vec<String>>,
        opened: Mutex<Vec<String>>,
        files: FileTime,
    }

    impl FakeApps {
        /// Sets the index.
        pub fn set_index(&self, entries: Vec<AppEntry>) {
            *lock(&self.entries) = entries;
        }

        /// Makes each `index`, `launch` and `open_path` take `takes` on `clock`, as on a Start
        /// menu or a file a server holds: within the caller's deadline the call moves the clock
        /// by `takes` and answers; past it, the clock moves by the deadline's budget and the call
        /// answers `DeadlineExceeded`. A late launch or open is recorded as done when `late` is
        /// [`LateOperation::Lands`], as a real one abandoned at its deadline may still start the
        /// app or open the path, and is not recorded when it is [`LateOperation::Lost`].
        pub fn set_file_time(&self, clock: Arc<ManualClock>, takes: Duration, late: LateOperation) {
            self.files.set(clock, takes, late);
        }

        /// Gives `pid` a main window.
        pub fn set_window(&self, pid: u32, window: WindowRef) {
            lock(&self.windows).insert(pid, window);
        }

        /// The ids launched, in order.
        #[must_use]
        pub fn launched(&self) -> Vec<String> {
            lock(&self.launched).clone()
        }

        /// The URLs, URIs and paths opened, in order.
        #[must_use]
        pub fn opened(&self) -> Vec<String> {
            lock(&self.opened).clone()
        }
    }

    impl Apps for FakeApps {
        fn index(&self, deadline: Deadline) -> Result<Vec<AppEntry>, PlatformError> {
            self.files.run(INDEX, deadline, || lock(&self.entries).clone())
        }

        fn launch(&self, app: &AppEntry, deadline: Deadline) -> Result<u32, PlatformError> {
            self.files.run(LAUNCH, deadline, || {
                let count = {
                    let mut launched = lock(&self.launched);
                    launched.push(app.id.clone());
                    launched.len()
                };
                1000_u32.saturating_add(u32::try_from(count).unwrap_or(u32::MAX))
            })
        }

        fn main_window(&self, pid: u32, _: Deadline) -> Result<Option<WindowRef>, PlatformError> {
            Ok(lock(&self.windows).get(&pid).copied())
        }

        fn open_url(&self, url: &str) -> Result<(), PlatformError> {
            lock(&self.opened).push(url.to_owned());
            Ok(())
        }

        fn open_uri(&self, uri: &str) -> Result<(), PlatformError> {
            lock(&self.opened).push(uri.to_owned());
            Ok(())
        }

        fn open_path(
            &self,
            path: &str,
            _: Option<&AppEntry>,
            deadline: Deadline,
        ) -> Result<(), PlatformError> {
            self.files.run(OPEN_PATH, deadline, || lock(&self.opened).push(path.to_owned()))
        }
    }

    /// Media sessions a test sets, with recorded controls.
    #[derive(Debug)]
    pub struct FakeMediaSessions {
        sessions: Mutex<Vec<MediaState>>,
        controls: Mutex<Vec<(String, MediaAction)>>,
        sinks: Sinks<MediaState>,
    }

    impl Default for FakeMediaSessions {
        fn default() -> Self {
            Self { sessions: Mutex::default(), controls: Mutex::default(), sinks: Sinks::new() }
        }
    }

    impl FakeMediaSessions {
        /// Sets the sessions and tells every subscriber about each.
        pub fn set_sessions(&self, sessions: Vec<MediaState>) {
            for session in &sessions {
                self.sinks.emit(session);
            }
            *lock(&self.sessions) = sessions;
        }

        /// The controls sent, in order.
        #[must_use]
        pub fn controls(&self) -> Vec<(String, MediaAction)> {
            lock(&self.controls).clone()
        }
    }

    impl MediaSessions for FakeMediaSessions {
        fn sessions(&self) -> Result<Vec<MediaState>, PlatformError> {
            Ok(lock(&self.sessions).clone())
        }

        fn control(&self, session: &str, action: &MediaAction) -> Result<(), PlatformError> {
            lock(&self.controls).push((session.to_owned(), *action));
            Ok(())
        }

        fn subscribe(
            &self,
            sink: Box<dyn Fn(MediaState) + Send + Sync>,
        ) -> Result<Subscription, PlatformError> {
            Ok(self.sinks.add(sink))
        }
    }

    /// Recent files a test adds. Its methods take no time until [`Self::set_file_time`] gives
    /// them some.
    #[derive(Debug)]
    pub struct FakeRecentSources {
        items: Mutex<Vec<RecentItem>>,
        sinks: Sinks<RecentItem>,
        files: FileTime,
    }

    impl Default for FakeRecentSources {
        fn default() -> Self {
            Self { items: Mutex::default(), sinks: Sinks::new(), files: FileTime::default() }
        }
    }

    impl FakeRecentSources {
        /// Adds a newly opened file and tells the watchers.
        pub fn open(&self, item: RecentItem) {
            self.sinks.emit(&item);
            lock(&self.items).insert(0, item);
        }

        /// Makes each `recent`, `watch` and `thumbnail` take `takes` on `clock`, as on a profile
        /// a server holds: within the caller's deadline the call moves the clock by `takes` and
        /// answers; past it, the clock moves by the deadline's budget and the call answers
        /// `DeadlineExceeded`, leaving no watcher behind.
        ///
        /// Unlike the other fakes' `set_file_time`, it takes no [`LateOperation`], because the
        /// outcome cannot differ here: `recent` and `thumbnail` only read, and a late `watch`
        /// that lands has no caller to hold its subscription, so the subscription ends at once,
        /// as when an implementation's helper thread finds its caller gone. The fake lets every
        /// late call land.
        pub fn set_file_time(&self, clock: Arc<ManualClock>, takes: Duration) {
            self.files.set(clock, takes, LateOperation::Lands);
        }
    }

    impl RecentSources for FakeRecentSources {
        fn recent(
            &self,
            limit: usize,
            deadline: Deadline,
        ) -> Result<Vec<RecentItem>, PlatformError> {
            self.files
                .run(RECENT, deadline, || lock(&self.items).iter().take(limit).cloned().collect())
        }

        fn watch(
            &self,
            _roots: &[String],
            sink: Box<dyn Fn(RecentItem) + Send + Sync>,
            deadline: Deadline,
        ) -> Result<Subscription, PlatformError> {
            self.files.run(WATCH, deadline, || self.sinks.add(sink))
        }

        fn thumbnail(
            &self,
            _: &RecentItem,
            _: u32,
            deadline: Deadline,
        ) -> Result<Option<RgbaImage>, PlatformError> {
            self.files.run(THUMBNAIL, deadline, || None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use eileen_contracts::ids::{MonoTime, Personal, WallTime};
    use eileen_contracts::tools::apps::OfficeOp;
    use eileen_stdx::clock::{Clock, ManualClock};

    use super::{
        AppEntry, AppHost, AppKind, AppOp, Apps, FakeAppHost, FakeApps, FakeRecentSources,
        RecentItem, RecentSources,
    };
    use crate::{Deadline, LateOperation, PlatformError};

    /// The callers' deadline in these tests: 5 s, which the file times fall either side of.
    const FIVE_SECONDS: Deadline = Deadline::after(Duration::from_secs(5));

    fn notepad() -> AppEntry {
        AppEntry {
            id: "notepad".to_owned(),
            name: "Notepad".to_owned(),
            kind: AppKind::Packaged,
            path: None,
        }
    }

    fn notes() -> RecentItem {
        RecentItem {
            path: Personal::new("/home/sam/notes.txt".to_owned()),
            app: Some("Text Editor".to_owned()),
            kind: None,
            opened_at: WallTime(1_000),
        }
    }

    #[test]
    fn a_file_call_within_its_deadline_answers_after_its_own_time() {
        let clock = Arc::new(ManualClock::new(WallTime(0)));
        let recent = FakeRecentSources::default();
        recent.open(notes());
        recent.set_file_time(Arc::clone(&clock), Duration::from_secs(2));
        assert_eq!(recent.recent(10, FIVE_SECONDS), Ok(vec![notes()]), "the file, within 5 s");
        assert_eq!(clock.mono(), MonoTime(2_000_000_000), "the read took its own 2 s");
        assert_eq!(recent.thumbnail(&notes(), 96, FIVE_SECONDS), Ok(None), "no thumbnail");
        let apps = FakeApps::default();
        apps.set_file_time(Arc::clone(&clock), Duration::from_secs(5), LateOperation::Lost);
        apps.open_path("/home/sam/notes.txt", None, FIVE_SECONDS).unwrap();
        assert_eq!(
            apps.opened(),
            ["/home/sam/notes.txt"],
            "a call taking its whole budget answers"
        );
        assert_eq!(clock.mono(), MonoTime(9_000_000_000), "2 s, 2 s and 5 s, each its own time");
    }

    #[test]
    fn a_file_call_past_its_deadline_answers_deadline_exceeded_once_its_budget_has_passed() {
        let clock = Arc::new(ManualClock::new(WallTime(0)));
        let recent = FakeRecentSources::default();
        recent.open(notes());
        // A mount whose server has stalled: each read would take a minute.
        recent.set_file_time(Arc::clone(&clock), Duration::from_secs(60));
        assert_eq!(
            recent.recent(10, FIVE_SECONDS),
            Err(PlatformError::DeadlineExceeded { call: "recent files" }),
            "the read passed its deadline"
        );
        assert_eq!(clock.mono(), MonoTime(5_000_000_000), "the caller waited its 5 s, no longer");
        let seen = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&seen);
        let sink: Box<dyn Fn(RecentItem) + Send + Sync> = Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(
            recent.watch(&["/home/sam".to_owned()], sink, FIVE_SECONDS).unwrap_err(),
            PlatformError::DeadlineExceeded { call: "watching recent files" },
            "opening the roots passed its deadline"
        );
        recent.open(notes());
        assert_eq!(seen.load(Ordering::SeqCst), 0, "no watcher outlived the call");

        let apps = FakeApps::default();
        apps.set_file_time(Arc::clone(&clock), Duration::from_secs(60), LateOperation::Lost);
        assert_eq!(
            apps.index(FIVE_SECONDS),
            Err(PlatformError::DeadlineExceeded { call: "app index" }),
            "the index read passed its deadline"
        );
        assert_eq!(
            apps.launch(&notepad(), FIVE_SECONDS),
            Err(PlatformError::DeadlineExceeded { call: "app launch" }),
            "the launch passed its deadline"
        );
        assert_eq!(
            apps.open_path("/home/sam/notes.txt", None, FIVE_SECONDS),
            Err(PlatformError::DeadlineExceeded { call: "opening a file or folder" }),
            "the open passed its deadline"
        );
        assert!(apps.launched().is_empty(), "the lost launch started nothing");
        assert!(apps.opened().is_empty(), "the lost open opened nothing");
        assert_eq!(clock.mono(), MonoTime(25_000_000_000), "five calls, each its 5 s budget");
    }

    #[test]
    fn a_launch_or_open_past_its_deadline_may_still_land() {
        let clock = Arc::new(ManualClock::new(WallTime(0)));
        let apps = FakeApps::default();
        // The server answers in the end, a minute late, after both callers gave up.
        apps.set_file_time(Arc::clone(&clock), Duration::from_secs(60), LateOperation::Lands);
        assert_eq!(
            apps.launch(&notepad(), FIVE_SECONDS),
            Err(PlatformError::DeadlineExceeded { call: "app launch" }),
            "the caller was told the launch passed its deadline"
        );
        assert_eq!(
            apps.open_path("/home/sam/notes.txt", None, FIVE_SECONDS),
            Err(PlatformError::DeadlineExceeded { call: "opening a file or folder" }),
            "and the open"
        );
        assert_eq!(apps.launched(), ["notepad"], "the abandoned launch started the app anyway");
        assert_eq!(apps.opened(), ["/home/sam/notes.txt"], "the abandoned open opened the file");
        assert_eq!(clock.mono(), MonoTime(10_000_000_000), "each caller waited its 5 s, no longer");
    }

    #[test]
    fn a_host_answers_in_order_and_records_calls() {
        let word = FakeAppHost::new("word");
        assert!(word.running().unwrap().is_none(), "not running");
        word.push_reply(Ok(serde_json::json!({"ok": true})));
        let op = AppOp::Office(OfficeOp::WordSelection);
        assert_eq!(word.call(&op, Deadline::UIA).unwrap()["ok"], true, "queued reply");
        assert!(word.call(&op, Deadline::UIA).is_err(), "no reply left");
        assert_eq!(word.calls().len(), 2, "both recorded");
    }

    #[test]
    fn launches_get_increasing_pids() {
        let apps = FakeApps::default();
        let first = apps.launch(&notepad(), FIVE_SECONDS).unwrap();
        let second = apps.launch(&notepad(), FIVE_SECONDS).unwrap();
        assert!(second > first, "increasing");
        assert_eq!(apps.launched(), ["notepad", "notepad"], "recorded");
    }
}
