//! `eileen-platform`'s `Grounding` for [`LinuxDesktop`]: what GNOME on Wayland lets AT-SPI2
//! answer (docs/LINUX.md section 3), each call one bridge job as `src/adapter.rs` says.
//!
//! - `windows` and `foreground`: each app's top-level objects (`src/toplevel.rs`).
//! - `focused_element`: the focused object of the foreground window (`src/element.rs`).
//! - `selection`: that object's selected text, from its `Text` alone.
//! - The rest answer `PlatformError::Unavailable`, each method's doc saying why and what brings
//!   it.

use eileen_contracts::desktop::{
    DesktopChange, ElementInfo, Monitor, Point, PresentationState, Selection, SelectionSource,
    WindowInfo, WindowRef,
};
use eileen_dbus_linux::DbusError;
use eileen_platform::{Deadline, Grounding, PlatformError, Subscription};

use crate::LinuxDesktop;
use crate::atspi::Interfaces;
use crate::element::{FOCUS, element_info, focus};

/// The most characters of a selection kept, as `Selection::text` allows.
const SELECTION_CHARS: usize = 4_000;

/// What the calls' errors name.
const WINDOWS: &str = "desktop windows";
const FOREGROUND: &str = "foreground window";
const SELECTION: &str = "selection";
const MONITORS: &str = "monitors";
const WINDOW_AT: &str = "window under a point";
const ELEMENT_AT: &str = "element under a point";
const PRESENTATION: &str = "presentation state";
const MIC: &str = "microphone use elsewhere";
const CHANGES: &str = "desktop changes";

impl Grounding for LinuxDesktop {
    /// `Unavailable`: Wayland gives a client no monitor layout. GNOME's
    /// `org.gnome.Mutter.DisplayConfig` or `wl_output` brings it, which phase L2 measures.
    fn monitors(&self) -> Result<Vec<Monitor>, PlatformError> {
        Err(PlatformError::Unavailable { what: MONITORS })
    }

    /// Each app's top-level objects, front to back: the first active window, then the rest in
    /// the registry's order, the minimized ones only when asked for. A window's class is its
    /// role's name (`frame`, `dialog`), its frame `(0, 0, width, height)` in its own
    /// coordinates, its process the name of its app's executable, and `z_order` counts the
    /// minimized windows too. No monitor, executable version, cloaking or page host is known:
    /// `url_host` is `None`, which keeps BUILD.md section 2.12's URL rule closed. The windows of
    /// an app that does not answer are left out, and while one is, no window is
    /// `is_foreground`: the front window is unknown, as [`Self::foreground`] says.
    ///
    /// A window is minimized when its toolkit reports `ICONIFIED`, which a native Wayland
    /// window may never do: `xdg_toplevel` tells no client it is minimized, GTK 3 does not
    /// report it there and GTK 4 has no such state. So `include_minimized: false` keeps a GTK
    /// window the user minimized, listed as not minimized; an X11 app under Xwayland can say so.
    ///
    /// `is_private` is true for a window whose name ends as an English Firefox's or Chromium's
    /// private window's does (`Private Browsing`, `(Incognito)`). False means only that no mark
    /// was found, as for another browser's or language's private window, or one whose title
    /// runs past the 1,000 characters kept. So BUILD.md section 2.10's rule that private
    /// windows are never recorded, and every rule that reads `is_private`, hold on Linux for
    /// those two marks alone. Nor does a class name a terminal, as section 2.6's terminal tier
    /// reads it on Windows: here that needs the `terminal` role or the terminal's process.
    fn windows(&self, include_minimized: bool) -> Result<Vec<WindowInfo>, PlatformError> {
        let mut windows = self.list(WINDOWS)?.windows;
        windows.retain(|window| include_minimized || !window.is_minimized);
        Ok(windows)
    }

    /// The first active window that is not defunct. `None` without one, which is also the
    /// answer when the app in front puts no tree on the accessibility bus. While an app's
    /// windows cannot be read (it does not answer in time, or answers what this crate cannot
    /// read), the front window is unknown, `DeadlineExceeded`: it may be that app's, and another
    /// app's window that still says it is active would otherwise take its place.
    fn foreground(&self) -> Result<Option<WindowRef>, PlatformError> {
        let listed = self.list(FOREGROUND)?;
        if listed.unread {
            return Err(PlatformError::DeadlineExceeded { call: FOREGROUND });
        }
        let front = listed.windows.iter().find(|window| window.is_foreground);
        Ok(front.map(|window| WindowRef { hwnd: window.hwnd, pid: window.pid }))
    }

    /// `Unavailable`: Wayland gives a client no screen coordinates, and a point in a window
    /// needs a contract amendment (docs/LINUX.md section 9).
    fn window_from_point(&self, _p: Point) -> Result<Option<WindowRef>, PlatformError> {
        Err(PlatformError::Unavailable { what: WINDOW_AT })
    }

    /// The focused element of the foreground window, its box in that window. `None` when no
    /// window is active, or nothing in the window's whole tree is focused. A search the settings'
    /// focus limits or four fifths of `deadline` cut short is `DeadlineExceeded`: the focus is
    /// unknown, not absent. So is the focus while an app's windows cannot be read, since the
    /// front window is then unknown ([`Self::foreground`]).
    fn focused_element(&self, deadline: Deadline) -> Result<Option<ElementInfo>, PlatformError> {
        let limits = self.settings().focus_limits;
        self.on_bus(FOCUS, deadline, move |atspi| async move {
            let Some((element, window)) = focus(&atspi, limits, deadline.budget()).await? else {
                return Ok(None);
            };
            Ok(Some(element_info(&atspi, &element, window).await?))
        })
    }

    /// `Unavailable`, as [`Self::window_from_point`] is.
    fn element_from_point(
        &self,
        _p: Point,
        _deadline: Deadline,
    ) -> Result<Option<ElementInfo>, PlatformError> {
        Err(PlatformError::Unavailable { what: ELEMENT_AT })
    }

    /// The selected text of the focused element, found as [`Self::focused_element`] finds it,
    /// with the same `DeadlineExceeded` when the focus is unknown: its `Text`'s first selected
    /// range, at most 4,000 characters, as `SelectionSource::TextPattern`. `None` without a
    /// focused element, for one without `Text` or with nothing selected, and for a password
    /// field, whose text the client refuses to read. `allow_clipboard` changes nothing: on
    /// Wayland only the focused surface may read the clipboard, so it is never read.
    fn selection(
        &self,
        _allow_clipboard: bool,
        deadline: Deadline,
    ) -> Result<Option<Selection>, PlatformError> {
        let limits = self.settings().focus_limits;
        self.on_bus(SELECTION, deadline, move |atspi| async move {
            let Some((element, _window)) = focus(&atspi, limits, deadline.budget()).await? else {
                return Ok(None);
            };
            if !element.interfaces.contains(Interfaces::TEXT) {
                return Ok(None);
            }
            match atspi.selection(&element.object, SELECTION_CHARS).await {
                Ok(text) => {
                    Ok(text.map(|text| Selection { text, source: SelectionSource::TextPattern }))
                }
                Err(DbusError::Refused { .. }) => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
    }

    /// `Unavailable`: GNOME tells clients no presentation or do-not-disturb state over a D-Bus
    /// interface this crate reads. Phase L2 measures the Settings portal.
    fn presentation_state(&self) -> Result<PresentationState, PlatformError> {
        Err(PlatformError::Unavailable { what: PRESENTATION })
    }

    /// `Unavailable`: `PipeWire`'s capture streams tell it, which `eileen-audio-linux` reads.
    fn mic_in_use_elsewhere(&self) -> Result<bool, PlatformError> {
        Err(PlatformError::Unavailable { what: MIC })
    }

    /// `Unavailable` until a later pull request brings AT-SPI's event listener: the registry's
    /// `RegisterEvent`, and the `Event.Object` and `Event.Window` signals.
    fn subscribe(
        &self,
        _sink: Box<dyn Fn(DesktopChange) + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        Err(PlatformError::Unavailable { what: CHANGES })
    }
}
