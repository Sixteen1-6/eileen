//! Path probes: every OS read of the runtime's path classification (BUILD.md sections 2.12
//! and 3.4, decision D86), and the two rules about a path's form that every probe applies
//! before it reads anything.
//!
//! Paths are strings as Windows writes them, not `std::path` values, so the classifier's table
//! tests run on Linux over [`FakePathProbe`].

use crate::{Deadline, PlatformError};

/// The prefix of a path Windows takes as written.
const VERBATIM: &str = r"\\?\";
/// The object manager's spelling of the same prefix.
const OBJECT: &str = r"\??\";
/// The prefix of the device namespace.
const DEVICE: &str = r"\\.\";

/// What [`path_fully_qualified`] answers for any other path.
const NOT_QUALIFIED: PlatformError =
    PlatformError::Refused { what: "path", reason: "not fully qualified" };

/// What [`path_allow_listed`] answers for any other path.
const OFF_ALLOW_LIST: PlatformError =
    PlatformError::Refused { what: "path", reason: "not a drive or volume path" };

/// The OS reads behind path classification (`eileen-shell-win`; Linux later:
/// `eileen-shell-linux`). The classifier in `eileen-runtime` reads nothing itself.
///
/// Every method that opens a file opens only a path [`path_allow_listed`] passes and answers
/// [`PlatformError::Refused`] for any other without opening it, because opening a pipe's name
/// connects to its server and opening a UNC path starts a connection with the user's
/// credentials. A caller treats every error, a passed deadline included, as a path it cannot
/// classify.
pub trait PathProbe: Send + Sync {
    /// `path` as Windows reads it (`GetFullPathNameW`): `..`, `.`, separators and trailing
    /// dots and spaces applied.
    ///
    /// Only a path [`path_fully_qualified`] passes is taken; any other is
    /// [`PlatformError::Refused`], so the answer never depends on the core's own current
    /// directory or drive. The caller joins a relative path to the tracked working folder,
    /// resolves a rooted path with no drive against that folder's root, and refuses a
    /// drive-relative one first. It touches no disk, so it takes no deadline.
    fn full_path(&self, path: &str) -> Result<String, PlatformError>;

    /// The final path of the deepest part of `path` that exists, and the parts after it that
    /// do not.
    ///
    /// It walks the existing part one component at a time from its root and reads the target
    /// of each symbolic link or junction instead of following it; a target off the allow-list
    /// is [`PlatformError::Refused`] unopened. Only then does it open the deepest part, so the
    /// links it checked and short names are followed.
    fn final_path(&self, path: &str, deadline: Deadline) -> Result<FinalPath, PlatformError>;

    /// Every name of an existing file, as full paths: more than one when it has hard links,
    /// because a file's final path is only the name it was opened by.
    fn link_names(&self, path: &str, deadline: Deadline) -> Result<Vec<String>, PlatformError>;

    /// The short (8.3) form of an existing path; `None` where the volume keeps no short names.
    fn short_path(&self, path: &str, deadline: Deadline) -> Result<Option<String>, PlatformError>;

    /// What `\\host\share` is, from names alone, never contacting the host.
    ///
    /// [`ShareKind::NotThisMachine`] unless the host is `localhost`, a literal loopback address
    /// or one of this computer's own names, compared as text with no name lookup of any kind.
    /// The classifier calls it before any method that opens a file.
    fn share(
        &self,
        host: &str,
        share: &str,
        deadline: Deadline,
    ) -> Result<ShareKind, PlatformError>;

    /// A `.lnk` file's target as stored, its variables unexpanded; `None` for a shortcut that
    /// names no file. Never resolved, because resolving searches the disk and can rewrite the
    /// shortcut.
    fn shortcut_target(
        &self,
        path: &str,
        deadline: Deadline,
    ) -> Result<Option<String>, PlatformError>;

    /// The program and command line that opening a file of this extension runs, for the
    /// default verb; `None` when nothing is registered for it.
    fn handler(
        &self,
        extension: &str,
        deadline: Deadline,
    ) -> Result<Option<FileHandler>, PlatformError>;
}

/// What [`PathProbe::final_path`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalPath {
    /// The final path of the deepest part that exists.
    pub existing: String,
    /// The parts after it, as written, outermost first.
    pub missing: Vec<String>,
}

/// What a UNC share is to this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareKind {
    /// The host is not this machine by any of its names.
    NotThisMachine,
    /// An administrative drive share of this machine, such as `C$`, with its drive letter.
    AdminDrive(char),
    /// Any other share of this machine.
    OtherShare,
}

/// What opening a file of some extension runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHandler {
    /// The program.
    pub program: String,
    /// Its command line for the default verb.
    pub command: String,
}

/// Passes a fully qualified path, the only kind [`PathProbe::full_path`] takes: a drive and
/// root (`C:\x`, `C:/x`), a UNC share (`\\host\share`, in either separator), or a path under
/// `\\?\`, `\??\` or `\\.\`.
///
/// Any other path is [`PlatformError::Refused`]: a relative one (`x\y`), a rooted one with no
/// drive (`\x`) and a drive-relative one (`C:foo`) all take their meaning from a current
/// directory, which a probe must never read.
pub fn path_fully_qualified(path: &str) -> Result<(), PlatformError> {
    let bytes = path.as_bytes();
    let object_path = path.strip_prefix(OBJECT).is_some_and(|rest| !rest.is_empty());
    if drive_root(bytes, is_separator) || names_a_share(bytes) || object_path {
        Ok(())
    } else {
        Err(NOT_QUALIFIED)
    }
}

/// Passes a path on section 2.12's allow-list, the only kind a probe opens: a drive path
/// (`X:\`, `\\?\X:\`, `\??\X:\`, `\\.\X:\`) or a volume path (`\\?\Volume{GUID}\`).
///
/// Any other path is [`PlatformError::Refused`]: a UNC path in any spelling, a pipe under any
/// of the three prefixes, `GLOBALROOT`, a console, and a drive or volume named without its
/// root (`\\.\C:` is the raw volume, not a folder).
///
/// The rule reads the path's form only, so it is applied to [`PathProbe::full_path`]'s
/// answer: a DOS device name such as `C:\x\NUL` has a drive path's form until the full path
/// turns it into `\\.\NUL`. One form is refused here all the same, because nothing after
/// this rule would catch it: under `\\.\` Windows still applies `..`, and the drive is an
/// ordinary component there, so `\\.\C:\..\pipe\x` names a pipe. A full path holds no `..`,
/// so the refusal costs no path a probe should open.
pub fn path_allow_listed(path: &str) -> Result<(), PlatformError> {
    let backslash = |byte| byte == b'\\';
    let allowed = if let Some(rest) = path.strip_prefix(DEVICE) {
        drive_root(rest.as_bytes(), backslash) && stays_below_its_root(rest.as_bytes())
    } else if let Some(rest) = path.strip_prefix(VERBATIM) {
        drive_root(rest.as_bytes(), backslash) || volume_root(rest.as_bytes())
    } else if let Some(rest) = path.strip_prefix(OBJECT) {
        drive_root(rest.as_bytes(), backslash)
    } else {
        drive_root(path.as_bytes(), backslash)
    };
    if allowed { Ok(()) } else { Err(OFF_ALLOW_LIST) }
}

/// Whether no component of `path` is `..`, read with either separator as Windows reads a
/// path under `\\.\`.
fn stays_below_its_root(path: &[u8]) -> bool {
    path.split(|byte| is_separator(*byte)).all(|component| component != b"..")
}

/// Either separator `GetFullPathNameW` reads.
const fn is_separator(byte: u8) -> bool {
    byte == b'\\' || byte == b'/'
}

/// Whether `path` starts with a drive letter, a colon and a separator.
fn drive_root(path: &[u8], separator: impl Fn(u8) -> bool) -> bool {
    matches!(path, [letter, b':', root, ..] if letter.is_ascii_alphabetic() && separator(*root))
}

/// Whether `path` starts with two separators, a host and a share. A path under `\\?\` or
/// `\\.\` has the same form, which is how those two prefixes pass.
fn names_a_share(path: &[u8]) -> bool {
    let [first, second, rest @ ..] = path else { return false };
    let mut parts = rest.split(|byte| is_separator(*byte));
    let named = |part: Option<&[u8]>| part.is_some_and(|name| !name.is_empty());
    is_separator(*first) && is_separator(*second) && named(parts.next()) && named(parts.next())
}

/// Whether `rest`, the part after `\\?\`, starts with `Volume{GUID}` and a backslash.
fn volume_root(rest: &[u8]) -> bool {
    const NAME: &[u8] = b"volume";
    // The braces, 32 hex digits and four hyphens.
    const GUID: usize = 38;
    let Some((name, after)) = rest.split_at_checked(NAME.len()) else { return false };
    let Some((guid, tail)) = after.split_at_checked(GUID) else { return false };
    let well_formed = guid.iter().enumerate().all(|(at, byte)| match at {
        0 => *byte == b'{',
        9 | 14 | 19 | 24 => *byte == b'-',
        37 => *byte == b'}',
        _ => byte.is_ascii_hexdigit(),
    });
    name.eq_ignore_ascii_case(NAME) && well_formed && tail.first() == Some(&b'\\')
}

#[cfg(any(test, feature = "fake"))]
pub use fake::FakePathProbe;

#[cfg(any(test, feature = "fake"))]
mod fake {
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::time::Duration;

    use super::{
        FileHandler, FinalPath, PathProbe, ShareKind, path_allow_listed, path_fully_qualified,
    };
    use crate::fake_util::lock;
    use crate::{Deadline, PlatformError};

    /// The answers set for one method, by path or name.
    type Answers<T> = BTreeMap<String, Result<T, PlatformError>>;

    /// The answers and the record behind a [`FakePathProbe`].
    #[derive(Debug, Default)]
    struct State {
        full: Answers<String>,
        finals: Answers<FinalPath>,
        names: Answers<Vec<String>>,
        short: Answers<Option<String>>,
        shares: Answers<ShareKind>,
        shortcuts: Answers<Option<String>>,
        handlers: Answers<Option<FileHandler>>,
        file_time: Duration,
        calls: Vec<(&'static str, String)>,
    }

    /// A probe that holds the answers a test sets and records every call.
    ///
    /// It applies the two rules of form as a real probe does: [`PathProbe::full_path`] refuses
    /// a path that is not fully qualified, and every method that opens a file refuses a path
    /// off the allow-list, whatever answer is set for it.
    ///
    /// A path with no answer set is an ordinary file that exists: it is its own full and final
    /// path, has one name and no short name, and as a shortcut names no file. A host and share
    /// with no answer set are not this machine, and an extension with none has no handler.
    /// Paths are matched as written, with no case folding; hosts, shares and extensions are
    /// matched without ASCII case.
    ///
    /// Time is simulated without a clock: [`Self::set_file_time`] says how long each call that
    /// takes a deadline runs, and a call that would outlast its deadline answers
    /// [`PlatformError::DeadlineExceeded`] at once.
    #[derive(Debug, Default)]
    pub struct FakePathProbe {
        state: Mutex<State>,
    }

    /// The key of a host and share: `\\host\share` in lower case.
    fn share_key(host: &str, share: &str) -> String {
        format!(r"\\{host}\{share}").to_ascii_lowercase()
    }

    impl FakePathProbe {
        /// Sets what [`PathProbe::full_path`] answers for `path`.
        pub fn set_full_path(&self, path: &str, answer: Result<String, PlatformError>) {
            lock(&self.state).full.insert(path.to_owned(), answer);
        }

        /// Sets what [`PathProbe::final_path`] answers for `path`.
        pub fn set_final_path(&self, path: &str, answer: Result<FinalPath, PlatformError>) {
            lock(&self.state).finals.insert(path.to_owned(), answer);
        }

        /// Sets what [`PathProbe::link_names`] answers for `path`.
        pub fn set_link_names(&self, path: &str, answer: Result<Vec<String>, PlatformError>) {
            lock(&self.state).names.insert(path.to_owned(), answer);
        }

        /// Sets what [`PathProbe::short_path`] answers for `path`.
        pub fn set_short_path(&self, path: &str, answer: Result<Option<String>, PlatformError>) {
            lock(&self.state).short.insert(path.to_owned(), answer);
        }

        /// Sets what [`PathProbe::share`] answers for `\\host\share`.
        pub fn set_share(&self, host: &str, share: &str, answer: Result<ShareKind, PlatformError>) {
            lock(&self.state).shares.insert(share_key(host, share), answer);
        }

        /// Sets what [`PathProbe::shortcut_target`] answers for `path`.
        pub fn set_shortcut_target(
            &self,
            path: &str,
            answer: Result<Option<String>, PlatformError>,
        ) {
            lock(&self.state).shortcuts.insert(path.to_owned(), answer);
        }

        /// Sets what [`PathProbe::handler`] answers for `extension`.
        pub fn set_handler(
            &self,
            extension: &str,
            answer: Result<Option<FileHandler>, PlatformError>,
        ) {
            lock(&self.state).handlers.insert(extension.to_ascii_lowercase(), answer);
        }

        /// Makes every call that takes a deadline run for `takes`: one whose deadline is
        /// shorter answers [`PlatformError::DeadlineExceeded`], and one that takes exactly its
        /// deadline still answers.
        pub fn set_file_time(&self, takes: Duration) {
            lock(&self.state).file_time = takes;
        }

        /// Every call in order, as the method's name and its argument (`\\host\share` for
        /// [`PathProbe::share`]), whether it was refused, passed its deadline or answered. A
        /// test reads it to show that no method that opens a file was asked about a path.
        #[must_use]
        pub fn calls(&self) -> Vec<(&'static str, String)> {
            lock(&self.state).calls.clone()
        }

        /// Records the call, then answers in the order a real probe would: `form`'s refusal
        /// first, because it is decided before anything is opened, then the deadline, then
        /// the answer set under `key`, or `unset`.
        fn answer<T>(
            &self,
            (call, asked): (&'static str, &str),
            form: Result<(), PlatformError>,
            deadline: Deadline,
            set: impl FnOnce(&State) -> Option<Result<T, PlatformError>>,
            unset: impl FnOnce() -> T,
        ) -> Result<T, PlatformError> {
            let (set, file_time) = {
                let mut state = lock(&self.state);
                state.calls.push((call, asked.to_owned()));
                (set(&state), state.file_time)
            };
            form?;
            if file_time > deadline.budget() {
                return Err(PlatformError::DeadlineExceeded { call });
            }
            set.unwrap_or_else(|| Ok(unset()))
        }
    }

    impl PathProbe for FakePathProbe {
        fn full_path(&self, path: &str) -> Result<String, PlatformError> {
            let set = {
                let mut state = lock(&self.state);
                state.calls.push(("full_path", path.to_owned()));
                state.full.get(path).cloned()
            };
            path_fully_qualified(path)?;
            set.unwrap_or_else(|| Ok(path.to_owned()))
        }

        fn final_path(&self, path: &str, deadline: Deadline) -> Result<FinalPath, PlatformError> {
            self.answer(
                ("final_path", path),
                path_allow_listed(path),
                deadline,
                |state| state.finals.get(path).cloned(),
                || FinalPath { existing: path.to_owned(), missing: Vec::new() },
            )
        }

        fn link_names(&self, path: &str, deadline: Deadline) -> Result<Vec<String>, PlatformError> {
            self.answer(
                ("link_names", path),
                path_allow_listed(path),
                deadline,
                |state| state.names.get(path).cloned(),
                || vec![path.to_owned()],
            )
        }

        fn short_path(
            &self,
            path: &str,
            deadline: Deadline,
        ) -> Result<Option<String>, PlatformError> {
            self.answer(
                ("short_path", path),
                path_allow_listed(path),
                deadline,
                |state| state.short.get(path).cloned(),
                || None,
            )
        }

        fn share(
            &self,
            host: &str,
            share: &str,
            deadline: Deadline,
        ) -> Result<ShareKind, PlatformError> {
            let key = share_key(host, share);
            self.answer(
                ("share", &format!(r"\\{host}\{share}")),
                Ok(()),
                deadline,
                |state| state.shares.get(&key).cloned(),
                || ShareKind::NotThisMachine,
            )
        }

        fn shortcut_target(
            &self,
            path: &str,
            deadline: Deadline,
        ) -> Result<Option<String>, PlatformError> {
            self.answer(
                ("shortcut_target", path),
                path_allow_listed(path),
                deadline,
                |state| state.shortcuts.get(path).cloned(),
                || None,
            )
        }

        fn handler(
            &self,
            extension: &str,
            deadline: Deadline,
        ) -> Result<Option<FileHandler>, PlatformError> {
            let key = extension.to_ascii_lowercase();
            self.answer(
                ("handler", extension),
                Ok(()),
                deadline,
                |state| state.handlers.get(&key).cloned(),
                || None,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{FakePathProbe, FileHandler, FinalPath, PathProbe, ShareKind};
    use crate::{Deadline, PlatformError};

    /// A deadline no test here passes unless it sets a file time.
    const SOON: Deadline = Deadline::after(Duration::from_secs(5));

    const OFF_ALLOW_LIST: PlatformError =
        PlatformError::Refused { what: "path", reason: "not a drive or volume path" };

    #[test]
    fn an_unset_path_is_an_ordinary_file_that_exists() {
        let probe = FakePathProbe::default();
        let path = r"C:\job\src\main.rs";
        assert_eq!(probe.full_path(path).unwrap(), path, "its own full path");
        let found = probe.final_path(path, SOON).unwrap();
        assert_eq!(found, FinalPath { existing: path.to_owned(), missing: vec![] }, "final");
        assert_eq!(probe.link_names(path, SOON).unwrap(), [path], "one name");
        assert_eq!(probe.short_path(path, SOON).unwrap(), None, "no short name");
        assert_eq!(probe.shortcut_target(path, SOON).unwrap(), None, "names no file");
        assert_eq!(probe.share("fileserver", "team", SOON).unwrap(), ShareKind::NotThisMachine);
        assert_eq!(probe.handler(".txt", SOON).unwrap(), None, "no handler");
    }

    #[test]
    fn set_answers_and_errors_come_back_for_each_path() {
        let probe = FakePathProbe::default();
        probe.set_full_path(r"C:\job\..\other\x.", Ok(r"C:\other\x".to_owned()));
        assert_eq!(probe.full_path(r"C:\job\..\other\x.").unwrap(), r"C:\other\x", "full");

        let through_a_junction = FinalPath {
            existing: r"D:\other\.git".to_owned(),
            missing: vec!["hooks".to_owned(), "pre-commit".to_owned()],
        };
        probe.set_final_path(r"C:\job\ext\.git\hooks\pre-commit", Ok(through_a_junction.clone()));
        let found = probe.final_path(r"C:\job\ext\.git\hooks\pre-commit", SOON).unwrap();
        assert_eq!(found, through_a_junction, "final");

        let both = vec![r"C:\job\a.txt".to_owned(), r"C:\Users\u\.ssh\id_ed25519".to_owned()];
        probe.set_link_names(r"C:\job\a.txt", Ok(both.clone()));
        assert_eq!(probe.link_names(r"C:\job\a.txt", SOON).unwrap(), both, "hard links");

        probe.set_short_path(r"C:\job\.git", Ok(Some(r"C:\job\GIT~1".to_owned())));
        let short = probe.short_path(r"C:\job\.git", SOON).unwrap();
        assert_eq!(short.as_deref(), Some(r"C:\job\GIT~1"), "short name");

        probe.set_shortcut_target(r"C:\job\notes.lnk", Ok(Some(r"%APPDATA%\n.txt".to_owned())));
        let target = probe.shortcut_target(r"C:\job\notes.lnk", SOON).unwrap();
        assert_eq!(target.as_deref(), Some(r"%APPDATA%\n.txt"), "raw target");

        let gone = PlatformError::NotFound { what: "path" };
        probe.set_final_path(r"Q:\gone", Err(gone.clone()));
        assert_eq!(probe.final_path(r"Q:\gone", SOON), Err(gone), "a set error");
    }

    #[test]
    fn hosts_shares_and_extensions_match_without_case() {
        let probe = FakePathProbe::default();
        probe.set_share("DESKTOP-1", "C$", Ok(ShareKind::AdminDrive('C')));
        probe.set_share("localhost", "Public", Ok(ShareKind::OtherShare));
        let admin = probe.share("desktop-1", "c$", SOON).unwrap();
        assert_eq!(admin, ShareKind::AdminDrive('C'), "this machine's drive share");
        let other = probe.share("LOCALHOST", "public", SOON).unwrap();
        assert_eq!(other, ShareKind::OtherShare, "another share of this machine");
        let unknown = probe.share("desktop-1", "d$", SOON).unwrap();
        assert_eq!(unknown, ShareKind::NotThisMachine, "a share nobody set");

        let python = FileHandler {
            program: r"C:\Python\python.exe".to_owned(),
            command: r#""C:\Python\python.exe" "%1" %*"#.to_owned(),
        };
        probe.set_handler(".py", Ok(Some(python.clone())));
        assert_eq!(probe.handler(".PY", SOON).unwrap(), Some(python), "the handler");
    }

    #[test]
    fn full_path_refuses_a_path_that_is_not_fully_qualified_whatever_is_set() {
        let probe = FakePathProbe::default();
        probe.set_full_path(r"src\main.rs", Ok(r"C:\core\src\main.rs".to_owned()));
        let refused = PlatformError::Refused { what: "path", reason: "not fully qualified" };
        assert_eq!(probe.full_path(r"src\main.rs"), Err(refused), "relative");
    }

    #[test]
    fn a_method_that_opens_a_file_refuses_a_path_off_the_allow_list_whatever_is_set() {
        let probe = FakePathProbe::default();
        let pipe = r"\\.\pipe\eileen-dispatch";
        let unc = r"\\fileserver\team\plan.docx";
        let long_unc = r"\\?\UNC\fileserver\team\plan.docx";
        let object_pipe = r"\??\pipe\eileen-dispatch";
        for path in [pipe, unc, long_unc, object_pipe] {
            let found = FinalPath { existing: path.to_owned(), missing: vec![] };
            probe.set_final_path(path, Ok(found));
            probe.set_link_names(path, Ok(vec![path.to_owned()]));
            probe.set_short_path(path, Ok(None));
            probe.set_shortcut_target(path, Ok(None));
            assert_eq!(probe.final_path(path, SOON), Err(OFF_ALLOW_LIST), "final_path {path}");
            assert_eq!(probe.link_names(path, SOON), Err(OFF_ALLOW_LIST), "link_names {path}");
            assert_eq!(probe.short_path(path, SOON), Err(OFF_ALLOW_LIST), "short_path {path}");
            let target = probe.shortcut_target(path, SOON);
            assert_eq!(target, Err(OFF_ALLOW_LIST), "shortcut_target {path}");
        }
    }

    #[test]
    fn a_call_that_outlasts_its_deadline_answers_deadline_exceeded() {
        let probe = FakePathProbe::default();
        let path = r"C:\job\a.txt";
        probe.set_file_time(Duration::from_secs(2));
        let exact = Deadline::after(Duration::from_secs(2));
        let short = Deadline::after(Duration::from_millis(1999));
        let passed = |call| Some(PlatformError::DeadlineExceeded { call });

        assert!(probe.final_path(path, SOON).is_ok(), "within its deadline");
        assert!(probe.final_path(path, exact).is_ok(), "exactly its deadline still answers");
        assert_eq!(probe.final_path(path, short).err(), passed("final_path"));
        assert_eq!(probe.link_names(path, short).err(), passed("link_names"));
        assert_eq!(probe.short_path(path, short).err(), passed("short_path"));
        assert_eq!(probe.share("localhost", "c$", short).err(), passed("share"));
        assert_eq!(probe.shortcut_target(path, short).err(), passed("shortcut_target"));
        assert_eq!(probe.handler(".txt", short).err(), passed("handler"));

        assert_eq!(probe.full_path(path).unwrap(), path, "full_path reads no disk");
        let pipe = probe.final_path(r"\\.\pipe\x", short);
        assert_eq!(pipe, Err(OFF_ALLOW_LIST), "a refusal opens nothing, so it takes no time");
    }

    #[test]
    fn every_call_is_recorded_in_order_refused_or_not() {
        let probe = FakePathProbe::default();
        let unc = r"\\Fileserver\Team\plan.docx";
        assert!(probe.full_path(unc).is_ok(), "a UNC share is fully qualified");
        assert!(probe.share("Fileserver", "Team", SOON).is_ok(), "asked about by name");
        assert!(probe.final_path(unc, SOON).is_err(), "refused");
        assert!(probe.handler(".docx", SOON).is_ok(), "no handler");
        let calls = probe.calls();
        let expected = [
            ("full_path", unc),
            ("share", r"\\Fileserver\Team"),
            ("final_path", unc),
            ("handler", ".docx"),
        ];
        let recorded: Vec<(&str, &str)> =
            calls.iter().map(|(call, asked)| (*call, asked.as_str())).collect();
        assert_eq!(recorded, expected, "as asked, in order");
    }
}
