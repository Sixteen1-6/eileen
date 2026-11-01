//! Finding the real `claude` executable (BUILD.md sections 4.2 and 6.5): through npm's shim
//! (`claude.cmd` or `claude.ps1`, or npm's link on Unix) to the package's own binary, or the
//! native installer's layout, and never the copy bundled with the Claude desktop app.
//!
//! The search reads only what it is given ([`LocateEnv`]), so both layouts are tested in a
//! temporary folder on every OS. It walks `PATH` in order, because the first `claude` there is
//! the one the user runs, then npm's global folder under `%APPDATA%` and the native installer's
//! `~/.local/bin`. Only those two layouts are accepted (section 2.5): any other `claude` found,
//! the desktop app's copy or a stray executable earlier on `PATH`, is skipped and named, so the
//! user learns why it was passed over, and a `PATH` folder under a `.claude` folder is refused
//! before anything in it is looked at.
//!
//! The native layout, from the installer's documentation: the binary at
//! `~/.local/bin/claude.exe` (on Unix `~/.local/bin/claude`, a link), and each downloaded
//! release under `~/.local/share/claude/versions/<version>`.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use crate::session::under_claude_folder;

/// The oldest CLI Eileen runs: 2.1.284 neutralises invisible characters and markup imitation in
/// notes (D77, section 2.10 item 15).
pub const MIN_CLI: ClaudeVersion = ClaudeVersion { major: 2, minor: 1, patch: 284 };

/// The newest CLI the contract tests have run against (WP3's approval contract test). A newer
/// one is allowed; the nightly drift job checks each release.
pub const TESTED_CLI: ClaudeVersion = ClaudeVersion { major: 2, minor: 1, patch: 304 };

/// The largest `package.json` or shim this module reads; real ones are under 2 KB.
const MAX_SMALL_FILE: u64 = 64 * 1024;

/// The package's folder inside npm's `node_modules`.
const NPM_PACKAGE: [&str; 2] = ["@anthropic-ai", "claude-code"];

/// A claude CLI version, compared by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClaudeVersion {
    /// The major number.
    pub major: u32,
    /// The minor number.
    pub minor: u32,
    /// The patch number.
    pub patch: u32,
}

impl ClaudeVersion {
    /// Parses `2.1.304`: exactly three numbers.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.').map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                None
            } else {
                part.parse::<u32>().ok()
            }
        });
        let version = Self { major: parts.next()??, minor: parts.next()??, patch: parts.next()?? };
        parts.next().is_none().then_some(version)
    }

    /// Reads what `claude --version` prints, `2.1.304 (Claude Code)`: its first word.
    #[must_use]
    pub fn parse_version_output(text: &str) -> Option<Self> {
        text.split_whitespace().next().and_then(Self::parse)
    }
}

impl fmt::Display for ClaudeVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// What the search may look at, read from the user's machine by the caller.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocateEnv {
    /// The folders of the `PATH` a worker sees, in order.
    pub path_dirs: Vec<PathBuf>,
    /// `%APPDATA%`, which holds npm's global folder on Windows.
    pub appdata: Option<PathBuf>,
    /// The user's home folder (`%USERPROFILE%`, or `$HOME`), which holds the native layout.
    pub home: Option<PathBuf>,
    /// Whether to look for Windows names (`claude.cmd`, `claude.ps1`, `claude.exe`) rather than
    /// Unix ones (`claude`). The caller sets it to `cfg!(windows)`; it is a field so both are
    /// tested on every OS.
    pub windows: bool,
}

/// How the CLI was installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Through npm: the shim the user runs, which leads to the package's binary.
    NpmShim {
        /// `claude.cmd` or `claude.ps1` (on Unix, npm's link).
        shim: PathBuf,
    },
    /// The native installer's binary in `~/.local/bin`, or one of its releases.
    Native,
}

/// Why a candidate was passed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    /// It, or the binary its shim leads to, is the Claude desktop app's bundled copy.
    DesktopApp,
    /// The folder lies under a `.claude` folder, which Eileen never opens.
    UnderClaudeFolder,
    /// A shim that runs a script through node, or leads to no binary that exists.
    ShimWithoutBinary,
    /// An executable that is neither npm's package binary nor the native installer's.
    UnknownInstall,
}

impl fmt::Display for RefusalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DesktopApp => "the Claude desktop app's own copy, which Eileen never runs",
            Self::UnderClaudeFolder => "under a .claude folder, which Eileen never opens",
            Self::ShimWithoutBinary => "a shim with no claude executable behind it",
            Self::UnknownInstall => "not an npm or native-installer copy",
        })
    }
}

/// A candidate passed over, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The file, or the `PATH` folder, passed over.
    pub path: PathBuf,
    /// Why.
    pub reason: RefusalReason,
}

/// The CLI the search settled on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeInstall {
    /// How it was installed, with the shim when npm installed it.
    pub kind: InstallKind,
    /// The real executable Eileen spawns.
    pub exe: PathBuf,
    /// Its version, when the layout says (npm's `package.json`, the native versions folder).
    pub version: Option<ClaudeVersion>,
    /// Candidates found before it and passed over.
    pub skipped: Vec<Refusal>,
}

/// Why no CLI could be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocateError {
    /// No `claude` in any folder searched.
    #[error("no claude CLI was found (searched {} folders)", .searched.len())]
    NotFound {
        /// Every folder searched, in order.
        searched: Vec<PathBuf>,
    },
    /// Every `claude` found was passed over.
    #[error("no usable claude CLI was found ({} passed over)", .refused.len())]
    NoUsableCli {
        /// What was found, and why each was passed over.
        refused: Vec<Refusal>,
    },
}

/// Finds the real `claude` executable: the first usable one on `PATH`, then in npm's global
/// folder, then in the native installer's layout.
pub fn locate_claude(env: &LocateEnv) -> Result<ClaudeInstall, LocateError> {
    let mut searched = Vec::new();
    let mut refused = Vec::new();
    for dir in search_dirs(env) {
        // A folder that is, or links into, a `.claude` folder is refused before anything in it
        // is looked at. A desktop app folder is searched: its `claude` is refused by name and
        // named, so the user learns why it was passed over.
        if resolve(&dir) == Err(RefusalReason::UnderClaudeFolder) {
            refused.push(Refusal { path: dir, reason: RefusalReason::UnderClaudeFolder });
            continue;
        }
        let found = if env.windows { in_windows_dir(&dir) } else { in_unix_dir(&dir) };
        searched.push(dir);
        match found {
            Some(Ok(mut install)) => {
                install.skipped = refused;
                return Ok(install);
            }
            Some(Err(refusal)) => refused.push(refusal),
            None => {}
        }
    }
    if refused.is_empty() {
        Err(LocateError::NotFound { searched })
    } else {
        Err(LocateError::NoUsableCli { refused })
    }
}

/// `PATH` in order, then npm's global folder and the native installer's, each once.
fn search_dirs(env: &LocateEnv) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let extra = [
        env.appdata.as_ref().filter(|_| env.windows).map(|appdata| appdata.join("npm")),
        env.home.as_ref().map(|home| home.join(".local").join("bin")),
    ];
    for dir in env.path_dirs.iter().cloned().chain(extra.into_iter().flatten()) {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// The most links followed from one candidate before it is refused.
const MAX_LINKS: usize = 8;

/// `claude.exe`, `claude.cmd` and then `claude.ps1` in `dir`, the order `cmd.exe` takes from
/// the default `PATHEXT`; npm's folder holds the two shims and no `claude.exe`.
fn in_windows_dir(dir: &Path) -> Option<Result<ClaudeInstall, Refusal>> {
    let exe = dir.join("claude.exe");
    if is_here(&exe) {
        return Some(direct(&exe));
    }
    ["claude.cmd", "claude.ps1"]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|shim| is_here(shim))
        .map(|shim| through_shim(&shim))
}

/// `claude` in `dir`: npm's link into its package, or the native installer's.
fn in_unix_dir(dir: &Path) -> Option<Result<ClaudeInstall, Refusal>> {
    let found = dir.join("claude");
    if !is_here(&found) {
        return None;
    }
    let refuse = |reason| Refusal { path: found.clone(), reason };
    let target = match resolve(&found) {
        Ok(target) => target,
        Err(reason) => return Some(Err(refuse(reason))),
    };
    if target.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("js")) {
        return Some(Err(refuse(RefusalReason::ShimWithoutBinary)));
    }
    Some(checked(&found, target, Reached::Directly))
}

/// Whether something, a dangling link included, is at `path`; nothing a link leads to is
/// looked at.
fn is_here(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// A `claude` executable found as it is, or a link to one.
fn direct(found: &Path) -> Result<ClaudeInstall, Refusal> {
    let exe = resolve(found).map_err(|reason| Refusal { path: found.to_path_buf(), reason })?;
    checked(found, exe, Reached::Directly)
}

/// Refuses `exe`, already resolved, when it or `found` is the desktop app's, lies under a
/// `.claude` folder, is no file, or belongs to neither layout; otherwise the install, with its
/// version from the layout. A refusal names `found`, what the user has.
fn checked(found: &Path, exe: PathBuf, reached: Reached) -> Result<ClaudeInstall, Refusal> {
    let refuse = |reason| Refusal { path: found.to_path_buf(), reason };
    refuse_by_name(found).and_then(|()| refuse_by_name(&exe)).map_err(refuse)?;
    if !exe.is_file() {
        return Err(refuse(RefusalReason::ShimWithoutBinary));
    }
    let kind =
        install_kind(found, &exe, reached).ok_or_else(|| refuse(RefusalReason::UnknownInstall))?;
    let version = layout_version(&exe);
    Ok(ClaudeInstall { kind, exe, version, skipped: Vec::new() })
}

/// How a candidate led to its executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reached {
    /// The candidate is the executable, or a link to it.
    Directly,
    /// A `claude.cmd` or `claude.ps1` names it; only npm writes those.
    ThroughShim,
}

/// Which of the two layouts section 2.5 accepts `exe`, found as `found`, belongs to: npm's
/// package binary, or the native installer's `~/.local/bin` executable or one of its releases.
/// `None` for anything else, which the search passes over. Judged by `exe`, where the links
/// led, never by where `found` lies: a link in `~/.local/bin` can lead anywhere, and the native
/// installer writes no shims.
fn install_kind(found: &Path, exe: &Path, reached: Reached) -> Option<InstallKind> {
    if in_npm_package(exe) {
        Some(InstallKind::NpmShim { shim: found.to_path_buf() })
    } else if is_native_release(exe) || (reached == Reached::Directly && is_native_bin(exe)) {
        Some(InstallKind::Native)
    } else {
        None
    }
}

/// The desktop app's folders and `.claude` folders, by name alone, before anything there is
/// looked at.
fn refuse_by_name(path: &Path) -> Result<(), RefusalReason> {
    if under_claude_folder(path) {
        Err(RefusalReason::UnderClaudeFolder)
    } else if is_desktop_app_copy(path) {
        Err(RefusalReason::DesktopApp)
    } else {
        Ok(())
    }
}

/// Follows the links (symbolic links and junctions) in `path`, in any of its folders, the way
/// the OS would, reading each link rather than opening what it leads to. Every path on the way
/// passes [`refuse_by_name`] before any of it is looked at, so a link into a `.claude` folder
/// or the desktop app's is refused without being followed. At most [`MAX_LINKS`] links.
fn resolve(path: &Path) -> Result<PathBuf, RefusalReason> {
    let mut current = lexical(path);
    for _ in 0..=MAX_LINKS {
        refuse_by_name(&current)?;
        match first_link(&current) {
            Some(next) => current = next,
            None => return Ok(current),
        }
    }
    Err(RefusalReason::ShimWithoutBinary)
}

/// `path` with its first link, counted from the root, replaced by the link's target.
fn first_link(path: &Path) -> Option<PathBuf> {
    let parts: Vec<Component<'_>> = path.components().collect();
    let mut prefix = PathBuf::new();
    for (at, part) in parts.iter().enumerate() {
        prefix.push(part);
        if !matches!(part, Component::Normal(_)) {
            continue;
        }
        let is_link = fs::symlink_metadata(&prefix).is_ok_and(|meta| meta.file_type().is_symlink());
        if is_link {
            let target = fs::read_link(&prefix).ok()?;
            let base = prefix.parent().map_or_else(PathBuf::new, Path::to_path_buf);
            let rest: PathBuf = parts.iter().skip(at + 1).collect();
            return Some(lexical(&base.join(target).join(rest)));
        }
    }
    None
}

/// `path` with `.` dropped and each `..` taking off the folder before it, without asking the
/// file system. [`resolve`] follows a link before it applies the `..` after it.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                let at_root = matches!(
                    out.components().next_back(),
                    None | Some(Component::RootDir | Component::Prefix(_) | Component::ParentDir)
                );
                if at_root {
                    out.push(part);
                } else {
                    out.pop();
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => out.push(part),
        }
    }
    out
}

/// Reads an npm shim for the binary it starts: the first quoted path, relative to the shim's
/// folder, whose file name is `claude.exe`. A shim that names none (it runs `cli.js` through
/// node) falls back to the package's binary beside it, if that exists.
fn through_shim(shim: &Path) -> Result<ClaudeInstall, Refusal> {
    let refuse = |reason| Refusal { path: shim.to_path_buf(), reason };
    let resolved = resolve(shim).map_err(refuse)?;
    let dir = shim.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let text = read_small(&resolved).map_err(|_| refuse(RefusalReason::ShimWithoutBinary))?;
    let named = shim_targets(&text, &dir).into_iter().find(|target| {
        target
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("claude.exe"))
    });
    let exe = named.unwrap_or_else(|| npm_package_dir(&dir).join("bin").join("claude.exe"));
    let exe = resolve(&exe).map_err(refuse)?;
    checked(shim, exe, Reached::ThroughShim)
}

/// Every path a shim starts relative to its own folder: `%dp0%\...` or `%~dp0\...` in
/// `claude.cmd`, `$basedir/...` in `claude.ps1`, each up to its closing quote.
fn shim_targets(text: &str, dir: &Path) -> Vec<PathBuf> {
    const MARKERS: [&str; 3] = ["%dp0%", "%~dp0", "$basedir"];
    let mut targets = Vec::new();
    for marker in MARKERS {
        for (at, _) in text.match_indices(marker) {
            let rest = text.get(at + marker.len()..).unwrap_or_default();
            let end = rest.find(['"', '\r', '\n']).unwrap_or(rest.len());
            let relative = rest.get(..end).unwrap_or_default();
            let mut target = dir.to_path_buf();
            for part in relative.split(['\\', '/']).filter(|part| !part.is_empty()) {
                target.push(part);
            }
            if target != dir {
                targets.push(target);
            }
        }
    }
    targets
}

/// `<npm folder>/node_modules/@anthropic-ai/claude-code`.
fn npm_package_dir(npm_dir: &Path) -> PathBuf {
    let mut dir = npm_dir.join("node_modules");
    dir.extend(NPM_PACKAGE);
    dir
}

/// Whether `path` lies inside npm's `@anthropic-ai/claude-code` package.
fn in_npm_package(path: &Path) -> bool {
    let names = names(path);
    names.windows(2).any(|pair| {
        pair.first().is_some_and(|scope| scope.eq_ignore_ascii_case(NPM_PACKAGE[0]))
            && pair.get(1).is_some_and(|package| package.eq_ignore_ascii_case(NPM_PACKAGE[1]))
    })
}

/// Whether `path` is in the native installer's `.local/bin`.
fn is_native_bin(path: &Path) -> bool {
    let names = names(path);
    let mut from_end = names.iter().rev().skip(1);
    from_end.next().is_some_and(|bin| bin.eq_ignore_ascii_case("bin"))
        && from_end.next().is_some_and(|local| local.eq_ignore_ascii_case(".local"))
}

/// Whether `path` is one of the native installer's releases,
/// `.local/share/claude/versions/<version>`.
fn is_native_release(path: &Path) -> bool {
    let names = names(path);
    let mut from_end = names.iter().rev();
    let wanted = ["versions", "claude", "share", ".local"];
    from_end.next().is_some_and(|release| ClaudeVersion::parse(release).is_some())
        && wanted.iter().all(|want| from_end.next().is_some_and(|name| named(name, want)))
}

/// The version the layout records: `version` in npm's `package.json` for the package's binary,
/// or the name of the native installer's release a Unix link leads to
/// (`~/.local/share/claude/versions/<version>`). `None` when the layout says nothing, as for
/// the native installer's `claude.exe` on Windows: it is a copy of a release, and the newest
/// release in the versions folder need not be the one copied (after a rollback, or while an
/// update waits), so the doctor asks the CLI itself.
fn layout_version(exe: &Path) -> Option<ClaudeVersion> {
    if in_npm_package(exe) {
        let package = exe.parent()?.parent()?.join("package.json");
        let text = read_small(&package).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        return value.get("version")?.as_str().and_then(ClaudeVersion::parse);
    }
    if is_native_release(exe) {
        return exe.file_name()?.to_str().and_then(ClaudeVersion::parse);
    }
    None
}

/// Reads a small text file, refusing one over [`MAX_SMALL_FILE`] and any under `.claude`.
fn read_small(path: &Path) -> io::Result<String> {
    if under_claude_folder(path) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "under a .claude folder"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_SMALL_FILE + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SMALL_FILE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "larger than a shim"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The names of `path`'s folders and file, split on both separators, so a Windows path read
/// on Unix splits too.
fn names(path: &Path) -> Vec<&str> {
    path.components()
        .filter_map(|part| match part {
            Component::Normal(name) => name.to_str(),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .flat_map(|name| name.split(['\\', '/']))
        .filter(|name| !name.is_empty())
        .collect()
}

/// Whether `path` lies in the Claude desktop app's folders, whose bundled CLI Eileen never runs:
/// its data folder (`Claude-Data`), its Squirrel install (`AnthropicClaude`), its MSIX package
/// (`WindowsApps`, or `Packages\Claude_*`), or a `claude-code\<version>` folder, the layout in
/// which it keeps each bundled release.
#[must_use]
pub fn is_desktop_app_copy(path: &Path) -> bool {
    let names = names(path);
    let marked = names.iter().any(|name| {
        named(name, "Claude-Data") || named(name, "AnthropicClaude") || named(name, "WindowsApps")
    });
    marked
        || names.windows(2).any(|pair| match pair {
            [first, second] => {
                (named(first, "Packages")
                    && second.get(..7).is_some_and(|start| named(start, "Claude_")))
                    || (named(first, "claude-code") && ClaudeVersion::parse(second).is_some())
            }
            _ => false,
        })
}

/// Whether two names are one, without regard to ASCII case.
const fn named(name: &str, wanted: &str) -> bool {
    name.eq_ignore_ascii_case(wanted)
}

#[cfg(test)]
pub(crate) mod tests;
