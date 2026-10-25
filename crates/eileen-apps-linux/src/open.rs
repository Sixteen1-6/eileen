//! Opening web addresses, URIs, files and folders for the user, and starting apps over D-Bus:
//! the Linux side of the `Apps` trait's `open_url`, `open_uri` and `open_path`, and of `launch`
//! for an app whose desktop entry says `DBusActivatable=true` (BUILD.md section 3.4;
//! docs/LINUX.md section 4). The desktop starts each app, so none is Eileen's child, as BUILD.md
//! section 2.1 asks of every app Eileen opens for the user.
//!
//! # What the portal is asked
//!
//! [`Opener`] calls `org.freedesktop.portal.Desktop` at `/org/freedesktop/portal/desktop`
//! ([the `OpenURI` documentation][openuri]).
//!
//! - `OpenURI` takes a web address, `http` or `https`, from [`Opener::open_url`], and any other
//!   URI from [`Opener::open_uri`]. Both are checked first: `open_url` refuses every other scheme,
//!   and `open_uri` refuses web addresses and `file:` URIs. A `file:` URI never reaches the
//!   portal, whose documentation says `OpenURI` does not support them, and `apps_open_path`'s
//!   checks of what a path is (BUILD.md section 2.7) apply only to paths.
//! - `OpenFile` takes a file or a folder from [`Opener::open_path`], opened read-only and passed
//!   as a descriptor. The path is checked before the open, so a device is refused without being
//!   opened, and the descriptor after it, so a path swapped for something else in between is
//!   refused too. The open never waits, as a FIFO's would for a writer. A folder opens in the
//!   file manager, the default app for `inode/directory`. `OpenDirectory` is not used: it shows
//!   the item selected in the folder that holds it (the portal calls
//!   `org.freedesktop.FileManager1`'s `ShowItems`, or opens the parent folder), which no trait
//!   method asks for. The `writable` option stays off, as the portal refuses it with a read-only
//!   descriptor, so a sandboxed default app gets the file read-only through the document portal;
//!   an app on the host opens the path itself.
//!
//! # The request and the deadline
//!
//! Each call is a portal request, made with `eileen-dbus-linux`'s [`Request`]: subscribed to its
//! `Response` before the call, its handle followed when an older portal names another, and its
//! `Response` taken only from the connection that answered the call. The caller's `deadline` is
//! one budget for the request and its `Response`, which may wait on the user's choice in a
//! dialog: the wait gets what the calls before it leave, and past it the request is closed,
//! which dismisses that dialog. Each call keeps the connection's method timeout, as
//! `eileen-dbus-linux`'s deadline model has it, so a call slower than what is left runs past the
//! budget, by up to that timeout. The `Response`'s codes ([the Request documentation][request]):
//! 0 opened, 1 cancelled by the user, 2 not opened (no handler, an invalid target, or an app that
//! failed to start). A call dropped before its answer leaves the request open, since a future
//! cannot send `Close` as it drops: bound the wait with `deadline`, not by dropping the call.
//!
//! [`Opener::open_path`]'s `deadline` bounds the checks of the path and the file too, and the
//! request gets what they leave of it. [`Opener::open_path_with`], which waits for no
//! signal, bounds its check of the path as it bounds its call: by the connection's method
//! timeout, or [`METHOD_TIMEOUT`] on a connection without one. The checks (a stat, and for the
//! portal the open and a stat of the descriptor) wait on another process when a FUSE daemon or a
//! network file server holds the file, and either can hang: BUILD.md section 2.1 (rule 2) gives
//! every call to another process a deadline. Checks past their bound are left to their thread,
//! and the answer is a [`FileError::Io`] that says they timed out. Each runs on a thread of its
//! own, not on the runtime's blocking pool, which a hung check would hold for good: on
//! `eileen-dbus-linux`'s bridge, whose two blocking threads zbus connects a bus with, two such
//! checks would leave no bus able to connect. [`Opener::checking_files_on`] gives them a pool.
//!
//! # D-Bus activation
//!
//! [`Opener::launch`] calls `org.freedesktop.Application.Activate` on the app's well-known name,
//! its desktop file ID without `.desktop`, at the object path made from that name: a `/` before
//! it, each `.` as `/` and each `-` as `_` ([the desktop entry specification][activation]). The
//! bus starts the app if it is not running. The process ID is that of the connection that
//! answered, from the bus's `GetConnectionUnixProcessID`. A Flatpak app whose bus access is
//! filtered reaches the bus through `xdg-dbus-proxy`, so its ID is probably the proxy's, which
//! is yet to be measured on Ubuntu 26.04. [`Opener::open_path_with`] checks the path as
//! [`Opener::open_path`] does, without opening it, and calls the app's `Open` with the file's
//! URI. These are method calls, bounded by the connection's method timeout, as the check of the
//! path is. The trait names the app by its `AppEntry`, whose indexed entry
//! [`entry_for`](crate::entry_for) finds.
//!
//! An app that is not D-Bus activatable starts from its `Exec` line, which needs
//! `eileen_supervisor::user_launch` (docs/LINUX.md section 5), so these calls answer
//! `Unavailable` for it. So does opening a file with a packaged app: a sandboxed app reads files
//! through the document portal, which the `Exec` line's file forwarding arranges and a plain
//! `Open` does not.
//!
//! # Errors
//!
//! A failed request or call is a [`DbusError`], and so is an address, a URI or a desktop file ID
//! refused before it. The path and the file it names fail as the crate's other local files do,
//! with a [`FileError`]; [`OpenError`] holds one or the other. None carries the address, the URI
//! or the path, nor a service's message text.
//!
//! [openuri]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.OpenURI.html
//! [request]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Request.html
//! [activation]: https://specifications.freedesktop.org/desktop-entry/latest/dbus.html

use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::time::Duration;

use eileen_dbus_linux::{DbusError, METHOD_TIMEOUT, Request};
use eileen_platform::PlatformError;
use rustix::fs::OFlags;
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio::time::Instant;
use zbus::names::WellKnownName;
use zbus::zvariant::{Fd, OwnedObjectPath, Value};

use crate::{AppKind, DesktopEntry, FileError};

/// The portal's bus name.
const PORTAL: &str = "org.freedesktop.portal.Desktop";
/// The portal's object.
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
/// The `OpenURI` portal's interface.
const OPEN_URI: &str = "org.freedesktop.portal.OpenURI";

/// The interface a D-Bus-activatable app serves.
const APPLICATION: &str = "org.freedesktop.Application";
/// The bus's name, which is also its interface's.
const BUS: &str = "org.freedesktop.DBus";
/// The bus's object.
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// The requests and calls, as errors name them.
const OPEN_URI_REQUEST: &str = "OpenURI request";
const OPEN_FILE_REQUEST: &str = "OpenFile request";
const ACTIVATE_CALL: &str = "app Activate call";
const OPEN_CALL: &str = "app Open call";
const PROCESS_ID_LOOKUP: &str = "process ID lookup";
/// The file or folder being opened, as errors name it.
const FILE: &str = "file to open";
/// The name of the threads the checks of paths and files run on.
const CHECK_THREAD: &str = "eileen-file-chk";
/// What a check that ended without answering (it panicked) says.
const CHECK_ENDED: &str = "the check ended without answering";

/// The flags the file to open, `recently-used.xbel` and the desktop entries are opened with
/// beside read-only: `O_NONBLOCK`, so the open returns at once where it would wait for a FIFO's
/// writer, and `O_NOCTTY`, so opening a terminal never makes it the process's controlling
/// terminal. std names neither; rustix has each architecture's value.
pub(crate) const OPEN_FLAGS: i32 = OFlags::NONBLOCK.union(OFlags::NOCTTY).bits().cast_signed();

/// Why a file or folder was not opened: the path or the file it names, or the request to open
/// it. Each holds an error that maps one to one onto `eileen-platform`'s `PlatformError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// The path, or the file or folder it names.
    File(FileError),
    /// The request to open it.
    Bus(DbusError),
}

/// The text of the error it holds.
impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(error) => error.fmt(f),
            Self::Bus(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<FileError> for OpenError {
    fn from(error: FileError) -> Self {
        Self::File(error)
    }
}

impl From<DbusError> for OpenError {
    fn from(error: DbusError) -> Self {
        Self::Bus(error)
    }
}

/// The `PlatformError` twin of the error it holds.
impl From<OpenError> for PlatformError {
    fn from(error: OpenError) -> Self {
        match error {
            OpenError::File(error) => error.into(),
            OpenError::Bus(error) => error.into(),
        }
    }
}

/// Opens web addresses, URIs, files and folders through the desktop's `OpenURI` portal, and
/// starts D-Bus-activatable apps, over the session bus its caller gives (a private bus in tests).
#[derive(Debug, Clone)]
pub struct Opener {
    conn: zbus::Connection,
    /// The runtime whose blocking pool checks paths and files, if not a thread for each.
    pool: Option<Handle>,
}

impl Opener {
    /// An opener on `conn`, the user's session bus, whose checks of paths and files each run on
    /// a thread of their own.
    #[must_use]
    pub fn new(conn: &zbus::Connection) -> Self {
        Self { conn: conn.clone(), pool: None }
    }

    /// The same opener, its checks of paths and files run on `pool`'s blocking pool instead: it
    /// caps the threads that checks hung on a file system hold, at the cost of every later check
    /// waiting once they hold them all, and tokio's paused time stands still while a check runs
    /// there, as it does not for a thread of its own. Dropping that runtime, as `#[tokio::main]`
    /// does on return, waits for each check it is running, a hung one included: end it with
    /// `Runtime::shutdown_background` or `shutdown_timeout`. Never the D-Bus bridge's runtime,
    /// whose blocking threads zbus needs to connect a bus.
    #[must_use]
    pub fn checking_files_on(mut self, pool: Handle) -> Self {
        self.pool = Some(pool);
        self
    }

    /// Opens a web address in the default browser (`Apps::open_url`), within `deadline` for the
    /// request and the portal's answer together. Only `http` and `https` addresses with a host
    /// are taken; anything else is [`DbusError::Invalid`] before the portal is asked.
    pub async fn open_url(&self, url: &str, deadline: Duration) -> Result<(), DbusError> {
        check_url(url)?;
        self.ask_portal(Target::Uri(url), deadline).await
    }

    /// Opens a URI, such as a `mailto:` link or an app's deep link, with its registered handler
    /// (`Apps::open_uri`), within `deadline` for the request and the portal's answer together. A
    /// web address belongs to [`Self::open_url`] and a `file:` URI to [`Self::open_path`]: both
    /// are [`DbusError::Invalid`] here, before the portal is asked.
    pub async fn open_uri(&self, uri: &str, deadline: Duration) -> Result<(), DbusError> {
        check_uri(uri)?;
        self.ask_portal(Target::Uri(uri), deadline).await
    }

    /// Opens a file or folder with its default app (`Apps::open_path` without an app), within
    /// `deadline` for the checks of the path and the file, the request and the portal's answer
    /// together. A path that is not absolute, or that names neither a file nor a folder, is
    /// [`FileError::Invalid`], one that cannot be opened is the [`FileError`] its I/O error
    /// gives, and checks past the deadline are a [`FileError::Io`] that says they timed out,
    /// before the portal is asked.
    pub async fn open_path(&self, path: &Path, deadline: Duration) -> Result<(), OpenError> {
        let started = Instant::now();
        let path = path.to_owned();
        let file = self.checked(deadline, move || open_read_only(&path)).await?;
        let left = deadline.saturating_sub(started.elapsed());
        Ok(self.ask_portal(Target::File(&file), left).await?)
    }

    /// Opens a file or folder with `app` (`Apps::open_path` with an app) through the app's
    /// `Open`, when the app is D-Bus activatable and not packaged; any other app is
    /// [`DbusError::Unavailable`], as it needs `eileen_supervisor::user_launch`. The path is
    /// checked as [`Self::open_path`] checks it, but not opened: the app opens it. The check is
    /// bounded as the call is, by the connection's method timeout ([`METHOD_TIMEOUT`] when it
    /// has none); past it, a [`FileError::Io`] that says it timed out.
    pub async fn open_path_with(&self, path: &Path, app: &DesktopEntry) -> Result<(), OpenError> {
        let address = address(app, "opening files with apps that are not D-Bus activatable")?;
        if app.kind == AppKind::Packaged {
            let what = "opening files with packaged apps over D-Bus";
            return Err(DbusError::Unavailable { what }.into());
        }
        let bound = self.conn.method_timeout().unwrap_or(METHOD_TIMEOUT);
        let checked = path.to_owned();
        self.checked(bound, move || check_path(&checked)).await?;
        let uris = [file_uri(path)];
        let body = (uris.as_slice(), platform_data());
        self.conn
            .call_method(
                Some(address.name.as_str()),
                &address.path,
                Some(APPLICATION),
                "Open",
                &body,
            )
            .await
            .map_err(|error| DbusError::from_zbus(OPEN_CALL, &error))?;
        Ok(())
    }

    /// Starts a D-Bus-activatable app, or brings it forward when it runs already, and returns
    /// its process ID (`Apps::launch`). An app that is not D-Bus activatable is
    /// [`DbusError::Unavailable`]: it needs `eileen_supervisor::user_launch`.
    pub async fn launch(&self, app: &DesktopEntry) -> Result<u32, DbusError> {
        let address = address(app, "launching apps that are not D-Bus activatable")?;
        let body = (platform_data(),);
        let reply = self
            .conn
            .call_method(
                Some(address.name.as_str()),
                &address.path,
                Some(APPLICATION),
                "Activate",
                &body,
            )
            .await
            .map_err(|error| DbusError::from_zbus(ACTIVATE_CALL, &error))?;
        self.process_id(&reply).await
    }

    /// The process ID of the connection that sent `reply`, from the bus, which sets the sender
    /// of every message it routes, so a later change of the name's owner cannot race it.
    async fn process_id(&self, reply: &zbus::Message) -> Result<u32, DbusError> {
        let fail = |error: zbus::Error| DbusError::from_zbus(PROCESS_ID_LOOKUP, &error);
        let header = reply.header();
        let sender = header.sender().ok_or_else(|| DbusError::Invalid {
            what: ACTIVATE_CALL,
            detail: "the reply has no sender".to_owned(),
        })?;
        let body = (sender.as_str(),);
        let reply = self
            .conn
            .call_method(Some(BUS), BUS_PATH, Some(BUS), "GetConnectionUnixProcessID", &body)
            .await
            .map_err(fail)?;
        reply.body().deserialize().map_err(fail)
    }

    /// One portal request for `target`, and what its `Response` says, within `deadline` for
    /// both: the wait for the `Response` gets what the calls before it leave.
    async fn ask_portal(&self, target: Target<'_>, deadline: Duration) -> Result<(), DbusError> {
        let started = Instant::now();
        let what = target.what();
        let mut request = Request::new(&self.conn)?;
        request.responses().await?;
        let options = HashMap::from([("handle_token", Value::from(request.token()))]);
        let (conn, portal, interface) = (&self.conn, Some(PORTAL), Some(OPEN_URI));
        let reply = match target {
            Target::Uri(uri) => {
                let body = ("", uri, &options);
                conn.call_method(portal, PORTAL_PATH, interface, "OpenURI", &body).await
            }
            Target::File(file) => {
                let body = ("", Fd::from(file), &options);
                conn.call_method(portal, PORTAL_PATH, interface, "OpenFile", &body).await
            }
        }
        .map_err(|error| DbusError::from_zbus(what, &error))?;
        let portal = request.follow(&reply).await?;
        let left = deadline.saturating_sub(started.elapsed());
        outcome(what, request.wait(&portal, left).await?)
    }

    /// Runs `check`, of a path and the file it names, on a thread of its own, or on the pool
    /// that [`Self::checking_files_on`] gave, and waits at most `bound` for its answer. Past it
    /// the check is left to finish there, and the answer is a [`FileError::Io`] that says it
    /// timed out.
    async fn checked<T: Send + 'static>(
        &self,
        bound: Duration,
        check: impl FnOnce() -> Result<T, FileError> + Send + 'static,
    ) -> Result<T, FileError> {
        let (answer, answered) = oneshot::channel();
        let job = move || {
            let _ignored = answer.send(check());
        };
        // Detached either way: the check answers through `answered`, or no one waits for it.
        match &self.pool {
            Some(pool) => drop(pool.spawn_blocking(job)),
            None => drop(
                eileen_stdx::thread::spawn_named(CHECK_THREAD, job)
                    .map_err(|error| FileError::io(FILE, &error))?,
            ),
        }
        match tokio::time::timeout(bound, answered).await {
            Ok(answer) => answer.unwrap_or_else(|_ended| {
                Err(FileError::Io { what: FILE, detail: CHECK_ENDED.to_owned() })
            }),
            Err(_elapsed) => Err(FileError::io(FILE, &io::Error::from(io::ErrorKind::TimedOut))),
        }
    }
}

/// What the portal is asked to open.
#[derive(Debug, Clone, Copy)]
enum Target<'a> {
    /// A URI, through `OpenURI`.
    Uri(&'a str),
    /// A file or folder opened read-only, through `OpenFile`.
    File(&'a File),
}

impl Target<'_> {
    /// The request, as errors name it.
    const fn what(self) -> &'static str {
        match self {
            Self::Uri(_) => OPEN_URI_REQUEST,
            Self::File(_) => OPEN_FILE_REQUEST,
        }
    }
}

/// What a request's `Response` code means.
fn outcome(what: &'static str, code: u32) -> Result<(), DbusError> {
    match code {
        0 => Ok(()),
        1 => Err(DbusError::Refused { what, reason: "cancelled by the user" }),
        2 => Err(DbusError::Refused { what, reason: "not opened" }),
        other => Err(DbusError::Invalid { what, detail: format!("response code {other}") }),
    }
}

/// Refuses `url` unless it is an `http` or `https` address with a host.
fn check_url(url: &str) -> Result<(), DbusError> {
    let what = "web address";
    let (scheme, rest) = parse(url, what)?;
    if scheme != "http" && scheme != "https" {
        return Err(invalid(what, "only http and https open as web addresses"));
    }
    let host = rest.strip_prefix("//").and_then(|after| after.split(['/', '?', '#']).next());
    if host.is_none_or(str::is_empty) {
        return Err(invalid(what, "no host"));
    }
    Ok(())
}

/// Refuses `uri` when it is not a URI, or when its scheme is one the other calls own.
fn check_uri(uri: &str) -> Result<(), DbusError> {
    let what = "URI";
    let (scheme, _rest) = parse(uri, what)?;
    match scheme.as_str() {
        "http" | "https" => Err(invalid(what, "a web address opens with open_url")),
        "file" => Err(invalid(what, "a file: URI opens with open_path")),
        _ => Ok(()),
    }
}

/// The lowercased scheme of `uri` and what follows its `:`. RFC 3986 allows no space or control
/// character anywhere in a URI, and a scheme is a letter followed by letters, digits, `+`, `-`
/// and `.` (section 3.1).
fn parse<'u>(uri: &'u str, what: &'static str) -> Result<(String, &'u str), DbusError> {
    if uri.chars().any(|c| c == ' ' || c.is_ascii_control()) {
        return Err(invalid(what, "holds a space or a control character"));
    }
    let (scheme, rest) = uri.split_once(':').ok_or_else(|| invalid(what, "no scheme"))?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return Err(invalid(what, "no scheme"));
    }
    Ok((scheme.to_ascii_lowercase(), rest))
}

/// An address or URI refused before the portal is asked.
fn invalid(what: &'static str, detail: &str) -> DbusError {
    DbusError::Invalid { what, detail: detail.to_owned() }
}

/// Opens `path` read-only for the portal, when it is absolute and names a file or a folder. The
/// path is checked first, so a device is refused without being opened (opening one can act on
/// it, as a serial port raises its modem lines); then [`open_checked`] opens it.
fn open_read_only(path: &Path) -> Result<File, FileError> {
    check_path(path)?;
    open_checked(path)
}

/// Refuses `path` unless it is absolute and names a file or a folder, without opening it.
fn check_path(path: &Path) -> Result<(), FileError> {
    if !path.is_absolute() {
        return Err(not_a_target("not absolute"));
    }
    let meta = std::fs::metadata(path).map_err(|error| FileError::io(FILE, &error))?;
    file_or_folder(&meta)
}

/// Opens `path` read-only without waiting (see [`OPEN_FLAGS`]), and refuses the descriptor
/// unless it is a file or a folder: the path may name something else by now.
fn open_checked(path: &Path) -> Result<File, FileError> {
    let file = File::options()
        .read(true)
        .custom_flags(OPEN_FLAGS)
        .open(path)
        .map_err(|error| FileError::io(FILE, &error))?;
    let meta = file.metadata().map_err(|error| FileError::io(FILE, &error))?;
    file_or_folder(&meta)?;
    Ok(file)
}

/// Refuses what is neither a file nor a folder: a FIFO, which an open would wait on for a
/// writer, a socket or a device.
fn file_or_folder(meta: &std::fs::Metadata) -> Result<(), FileError> {
    if meta.is_file() || meta.is_dir() { Ok(()) } else { Err(not_a_target("not a file or folder")) }
}

/// A path refused before the portal or the app is asked.
fn not_a_target(detail: &str) -> FileError {
    FileError::Invalid { what: "path", detail: detail.to_owned() }
}

/// Where a D-Bus-activatable app answers: its well-known name and its object path.
#[derive(Debug)]
struct Address {
    name: String,
    path: OwnedObjectPath,
}

/// Where `app` answers on the bus, when its entry says it is D-Bus activatable; otherwise
/// [`DbusError::Unavailable`] for `unavailable`. Its desktop file ID without `.desktop` must be
/// a well-known name, as the specification requires of such an app.
fn address(app: &DesktopEntry, unavailable: &'static str) -> Result<Address, DbusError> {
    if !app.dbus_activatable {
        return Err(DbusError::Unavailable { what: unavailable });
    }
    let name = app.id.strip_suffix(".desktop").unwrap_or(&app.id);
    let not_a_name = || invalid("desktop file ID", "not a D-Bus well-known name");
    WellKnownName::try_from(name).map_err(|_error| not_a_name())?;
    let path = OwnedObjectPath::try_from(object_path(name)).map_err(|_error| not_a_name())?;
    Ok(Address { name: name.to_owned(), path })
}

/// The object path of the app with the well-known name `name`: a `/` before it, each `.` as `/`
/// and each `-` as `_`.
fn object_path(name: &str) -> String {
    let mut path = String::with_capacity(name.len().saturating_add(1));
    path.push('/');
    path.extend(name.chars().map(|c| match c {
        '.' => '/',
        '-' => '_',
        other => other,
    }));
    path
}

/// The `platform_data` sent with `Activate` and `Open`: empty, as Eileen has no startup
/// notification ID or activation token to pass on.
fn platform_data() -> HashMap<&'static str, Value<'static>> {
    HashMap::new()
}

/// The `file:` URI of the absolute path `path`, with every byte outside RFC 3986's unreserved
/// characters and `/` percent-encoded, so a name with spaces, `#`, `%` or bytes that are not
/// UTF-8 stays one path.
fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            uri.push(char::from(byte));
        } else {
            let _ignored = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use eileen_platform::PlatformError;
    use rustix::fs::{CWD, Mode, mkfifoat};

    use super::{
        OpenError, check_uri, check_url, file_uri, object_path, open_checked, outcome, parse,
    };
    use crate::{DbusError, FileError};

    #[test]
    fn app_object_paths_follow_the_specification() {
        assert_eq!(object_path("org.example.FooViewer"), "/org/example/FooViewer", "dots");
        assert_eq!(object_path("org.example.foo-viewer"), "/org/example/foo_viewer", "a dash");
    }

    #[test]
    fn file_uris_escape_everything_but_unreserved_bytes() {
        assert_eq!(
            file_uri(Path::new("/home/sam/Notes 1#%?.txt")),
            "file:///home/sam/Notes%201%23%25%3F.txt",
            "space, hash, percent and question mark"
        );
        assert_eq!(
            file_uri(Path::new(OsStr::from_bytes(b"/tmp/caf\xc3\xa9-\xff~"))),
            "file:///tmp/caf%C3%A9-%FF~",
            "UTF-8 and bytes that are not"
        );
    }

    #[test]
    fn schemes_follow_rfc_3986() {
        let scheme = |uri: &str| parse(uri, "URI").map(|(scheme, _)| scheme).ok();
        assert_eq!(scheme("HTTPS://example.com").as_deref(), Some("https"), "lowercased");
        assert_eq!(scheme("mailto:sam@example.com").as_deref(), Some("mailto"), "no slashes");
        assert_eq!(scheme("x-app+v1.2:open").as_deref(), Some("x-app+v1.2"), "+, - and .");
        assert_eq!(scheme("1password://open"), None, "a digit first");
        assert_eq!(scheme(":nothing"), None, "empty");
        assert_eq!(scheme("example.com/page"), None, "no colon");
        assert_eq!(scheme("https://example.com/a b"), None, "a space");
        assert_eq!(scheme("https://example.com/\n"), None, "a control character");
    }

    #[test]
    fn web_addresses_need_http_or_https_and_a_host() {
        assert_eq!(check_url("https://www.youtube.com/watch?v=abc&t=200"), Ok(()), "https");
        assert_eq!(check_url("http://localhost:8080"), Ok(()), "http with a port");
        for url in ["ftp://example.com", "mailto:sam@example.com", "file:///etc/hosts"] {
            assert!(
                matches!(check_url(url), Err(DbusError::Invalid { what: "web address", .. })),
                "{url} is not a web address"
            );
        }
        for url in ["https:", "https:example.com", "https:///path", "http://?q", "http://#top"] {
            assert!(matches!(check_url(url), Err(DbusError::Invalid { .. })), "{url} has no host");
        }
    }

    #[test]
    fn uris_leave_web_addresses_and_files_to_the_other_calls() {
        assert_eq!(check_uri("mailto:sam@example.com"), Ok(()), "mail");
        assert_eq!(check_uri("steam://run/570"), Ok(()), "an app's deep link");
        for uri in ["https://example.com", "HTTP://example.com", "file:///etc/hosts", "File:x"] {
            assert!(matches!(check_uri(uri), Err(DbusError::Invalid { what: "URI", .. })), "{uri}");
        }
    }

    #[test]
    fn response_codes_follow_the_request_documentation() {
        assert_eq!(outcome("call", 0), Ok(()), "success");
        assert_eq!(
            outcome("call", 1),
            Err(DbusError::Refused { what: "call", reason: "cancelled by the user" }),
            "cancelled"
        );
        assert_eq!(
            outcome("call", 2),
            Err(DbusError::Refused { what: "call", reason: "not opened" }),
            "ended another way"
        );
        assert_eq!(
            outcome("call", 3),
            Err(DbusError::Invalid { what: "call", detail: "response code 3".to_owned() }),
            "not a code the documentation gives"
        );
    }

    #[test]
    fn an_open_error_reads_as_the_error_it_holds() {
        let file = FileError::NotFound { what: "file to open" };
        assert_eq!(OpenError::from(file.clone()).to_string(), file.to_string(), "a file's");
        let bus = DbusError::Unavailable { what: "OpenFile request" };
        assert_eq!(OpenError::from(bus.clone()).to_string(), bus.to_string(), "a request's");
    }

    #[test]
    fn an_open_error_becomes_the_platform_twin_of_the_error_it_holds() {
        let file = FileError::Refused { what: "file to open", reason: "not readable" };
        assert_eq!(
            PlatformError::from(OpenError::File(file.clone())),
            PlatformError::from(file),
            "a file's"
        );
        let bus = DbusError::Refused { what: "OpenFile request", reason: "not opened" };
        assert_eq!(
            PlatformError::from(OpenError::Bus(bus.clone())),
            PlatformError::from(bus),
            "a request's"
        );
    }

    /// A FIFO with no writer, in a fresh folder of the temporary directory that goes when it
    /// drops.
    struct Fifo(PathBuf);

    impl Fifo {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("eileen-fifo-{}", std::process::id()));
            let _ignored = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let fifo = Self(dir);
            mkfifoat(CWD, fifo.path(), Mode::RUSR | Mode::WUSR).unwrap();
            fifo
        }

        fn path(&self) -> PathBuf {
            self.0.join("pipe")
        }
    }

    impl Drop for Fifo {
        fn drop(&mut self) {
            let _ignored = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_path_swapped_for_a_fifo_after_the_check_is_refused_without_waiting() {
        // `open_checked` opens what the path names after the check, which a swap can make a
        // FIFO. It has no writer, so an open that waited for one would never return.
        let fifo = Fifo::new();
        assert_eq!(
            open_checked(&fifo.path()).map(|_file| ()),
            Err(FileError::Invalid { what: "path", detail: "not a file or folder".to_owned() }),
            "opened without waiting, then refused for what the descriptor is"
        );
    }
}
