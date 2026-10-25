//! The desktop's top-level windows, as AT-SPI2 lists them: the children of each app's root, an
//! app's windows, read one app after another, each app's reads bounded by the call timeout.
//!
//! A window's id is [`window_id`] of its object, a hash that every process computes the same,
//! since `WindowInfo::hwnd` must name the same window wherever it is read (`Apps::main_window`
//! answers one too). Its frame is window-relative: Wayland gives a client no window positions,
//! so a frame is `(0, 0, width, height)`.

use std::collections::BTreeMap;
use std::path::Path;

use eileen_contracts::desktop::{Rect, WindowInfo};
use eileen_dbus_linux::DbusError;

use crate::atspi::{Accessible, Atspi, Interfaces, ObjectRef, State};
use crate::element::role_name;

/// The most windows one app's root lists that a listing reads.
const WINDOWS_PER_APP: u32 = 64;
/// FNV-1a's 64-bit offset basis and prime.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// What the kernel adds to `/proc/<pid>/exe`'s target once the file has been replaced, as a
/// package update replaces it.
const DELETED: &str = " (deleted)";

/// The id of the window `object` is: FNV-1a 64 over its bus name, a zero byte and its path, with
/// the sign bit cleared, and 1 in place of 0. It is the same in every process and every run, for
/// as long as the app keeps its connection.
#[must_use]
pub fn window_id(object: &ObjectRef) -> i64 {
    let bytes = object.bus.as_bytes().iter().chain(&[0]).chain(object.path.as_bytes());
    let hash =
        bytes.fold(FNV_OFFSET, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME));
    i64::try_from(hash & (u64::MAX >> 1)).unwrap_or(i64::MAX).max(1)
}

/// A window a listing found, as Eileen keeps it to reach the window again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownWindow {
    /// Its object, on the accessibility bus.
    pub object: ObjectRef,
    /// Its app's process, as the bus's daemon gives it.
    pub pid: u32,
    /// That process's name, as `WindowInfo::process` gives it.
    pub process: String,
}

/// A top-level window as a listing read it: plain data, which a bridge job can answer.
#[derive(Debug, Clone)]
pub(crate) struct Toplevel {
    /// What it is.
    pub(crate) window: Accessible,
    /// Its box in its own coordinates, all zero without `Component`.
    pub(crate) rect: Rect,
    /// Its app's process.
    pub(crate) pid: u32,
}

impl Toplevel {
    /// Whether it is the active window.
    pub(crate) const fn is_active(&self) -> bool {
        self.window.states.contains(State::ACTIVE)
    }
}

/// The windows of every app the registry lists, the first active one first and the rest in the
/// registry's order, without the defunct ones.
///
/// - An app whose root, listing or process cannot be read, or one of whose windows answers
///   anything but [`DbusError::NotFound`], is left out with all its windows, and the others are
///   listed. A window gone since its app's listing is left out alone.
/// - No registry is [`DbusError::Unavailable`].
pub(crate) async fn toplevels(atspi: &Atspi) -> Result<Vec<Toplevel>, DbusError> {
    let mut windows = Vec::new();
    for app in atspi.applications().await? {
        windows.extend(app_windows(atspi, &app).await.unwrap_or_default());
    }
    if let Some(active) = windows.iter().position(Toplevel::is_active)
        && let Some(front) = windows.get_mut(..=active)
    {
        front.rotate_right(1);
    }
    Ok(windows)
}

/// The windows of the app whose root is `app`, at most [`WINDOWS_PER_APP`].
async fn app_windows(atspi: &Atspi, app: &ObjectRef) -> Result<Vec<Toplevel>, DbusError> {
    let root = atspi.accessible(app).await?;
    let pid = atspi.pid(&app.bus).await?;
    let mut windows = Vec::new();
    for object in atspi.children(&root, WINDOWS_PER_APP).await? {
        match toplevel(atspi, &object, pid).await {
            Ok(window) if !window.window.states.contains(State::DEFUNCT) => windows.push(window),
            Ok(_) | Err(DbusError::NotFound { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(windows)
}

/// The window `object` is, of the app whose process is `pid`.
async fn toplevel(atspi: &Atspi, object: &ObjectRef, pid: u32) -> Result<Toplevel, DbusError> {
    let window = atspi.accessible(object).await?;
    let rect = if window.interfaces.contains(Interfaces::COMPONENT) {
        atspi.extents(object).await?
    } else {
        Rect::default()
    };
    Ok(Toplevel { window, rect, pid })
}

/// The windows `toplevels` lists, in its order, as `Grounding` gives them, each with what Eileen
/// keeps of it. Each process is named once, by [`process_name`]; a window is Eileen's when its
/// process is one of `own_processes`.
pub(crate) fn window_infos(
    toplevels: Vec<Toplevel>,
    own_processes: &[String],
) -> Vec<(WindowInfo, KnownWindow)> {
    let mut names = BTreeMap::new();
    let listed = toplevels.into_iter().zip(0_u32..).map(|(Toplevel { window, rect, pid }, z)| {
        let process: &String = names.entry(pid).or_insert_with(|| process_name(pid));
        let info = WindowInfo {
            hwnd: window_id(&window.object),
            pid,
            process: process.clone(),
            exe_version: None,
            title: window.name,
            class: role_name(window.role).to_owned(),
            monitor_ordinal: None,
            frame_rect: Rect { x: 0, y: 0, ..rect },
            z_order: z,
            is_minimized: window.states.contains(State::ICONIFIED),
            is_cloaked: false,
            is_foreground: z == 0 && window.states.contains(State::ACTIVE),
            is_eileen: own_processes.contains(process),
            is_private: false,
            url_host: None,
        };
        (info, KnownWindow { object: window.object, pid, process: process.clone() })
    });
    listed.collect()
}

/// The name of process `pid`: the file name of its executable, as `/proc/<pid>/exe` links it,
/// else its `comm`, which the kernel shows for another user's process too; empty once it has
/// gone. The kernel answers both reads, so they need no deadline (the WP10 file-deadline record).
fn process_name(pid: u32) -> String {
    let process = Path::new("/proc").join(pid.to_string());
    if let Ok(exe) = std::fs::read_link(process.join("exe"))
        && let Some(name) = exe_name(&exe)
    {
        return name;
    }
    let comm = std::fs::read_to_string(process.join("comm")).unwrap_or_default();
    comm.trim_end_matches('\n').to_owned()
}

/// The file name of an executable whose path `/proc/<pid>/exe` links to, without the
/// [`DELETED`] mark an update leaves, so a window keeps its process's name once its file has been
/// replaced.
fn exe_name(exe: &Path) -> Option<String> {
    let name = exe.file_name()?.to_str()?;
    Some(name.strip_suffix(DELETED).unwrap_or(name).to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use zbus::names::OwnedUniqueName;
    use zbus::zvariant::OwnedObjectPath;

    use super::{ObjectRef, exe_name, process_name, window_id};

    /// The object at `path` of the app at `bus`.
    fn object(bus: &str, path: &str) -> ObjectRef {
        let bus = OwnedUniqueName::try_from(bus).unwrap();
        ObjectRef { bus, path: OwnedObjectPath::try_from(path).unwrap() }
    }

    #[test]
    fn a_window_id_is_fnv_1a_of_its_bus_and_path() {
        // FNV-1a 64 of ":1.42\0/org/a11y/atspi/accessible/1" is 0xb5d4539254fb2dcb, whose sign
        // bit is set; computed apart from this code, which also gives 0xaf63dc4c8601ec8c for "a".
        let window = object(":1.42", "/org/a11y/atspi/accessible/1");
        assert_eq!(window_id(&window), 0x35d4_5392_54fb_2dcb, "the hash, its sign bit cleared");
        assert!(window_id(&window) > 0, "positive, so never a null handle");
        let other = object(":1.43", "/org/a11y/atspi/accessible/1");
        assert_ne!(window_id(&other), window_id(&window), "another app, another id");
    }

    #[test]
    fn a_process_is_named_by_its_executable_even_once_replaced() {
        let exe = std::env::current_exe().unwrap();
        let own = exe.file_name().unwrap().to_str().unwrap();
        assert_eq!(process_name(std::process::id()), own, "this test's own process");
        assert_eq!(process_name(u32::MAX), "", "no such process");
        let name = |path: &str| exe_name(Path::new(path));
        assert_eq!(name("/usr/bin/eileen-hud").as_deref(), Some("eileen-hud"), "the file name");
        assert_eq!(
            name("/usr/bin/eileen-hud (deleted)").as_deref(),
            Some("eileen-hud"),
            "an executable an update replaced"
        );
        assert_eq!(name("/").as_deref(), None, "no file name");
    }
}
