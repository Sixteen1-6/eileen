//! An element as `Grounding` gives it ([`ElementInfo`]), and the search for the focused one.
//!
//! An element's control type is its role's name in at-spi2-core 2.52's own table
//! (`atspi_role_get_name`, roles 0 to 129), never the name an app gives its role. Its box is
//! window-relative, the one kind of coordinate Wayland gives a meaning to (docs/LINUX.md section
//! 3), and its patterns are the short names of the AT-SPI2 interfaces it implements.

use std::time::Duration;

use eileen_contracts::desktop::{ElementInfo, Rect, WindowRef};
use eileen_dbus_linux::DbusError;
use eileen_platform::TreeLimits;
use tokio::time::Instant;

use crate::atspi::{Accessible, Atspi, Interfaces, Role, State, Walk, WalkNode};
use crate::toplevel::{toplevels, window_id};

/// What a focus search's errors name, and `Grounding::focused_element`'s.
pub(crate) const FOCUS: &str = "focused element";
/// The longest a focus search reads, whatever its deadline: a walk within its limits ends long
/// before.
const LONGEST_SEARCH: Duration = Duration::from_secs(3_600);

/// `atspi_role_get_name` of each role at-spi2-core 2.52 defines, from 0 to 129, in order.
const ROLE_NAMES: &str = "invalid,accelerator label,alert,animation,arrow,calendar,canvas,\
    check box,check menu item,color chooser,column header,combo box,date editor,desktop icon,\
    desktop frame,dial,dialog,directory pane,drawing area,file chooser,filler,focus traversable,\
    font chooser,frame,glass pane,html container,icon,image,internal frame,label,layered pane,list,\
    list item,menu,menu bar,menu item,option pane,page tab,page tab list,panel,password text,\
    popup menu,progress bar,push button,radio button,radio menu item,root pane,row header,\
    scroll bar,scroll pane,separator,slider,spin button,split pane,status bar,table,table cell,\
    table column header,table row header,tearoff menu item,terminal,text,toggle button,tool bar,\
    tool tip,tree,tree table,unknown,viewport,window,extended,header,footer,paragraph,ruler,\
    application,autocomplete,editbar,embedded,entry,chart,caption,document frame,heading,page,\
    section,redundant object,form,link,input method window,table row,tree item,\
    document spreadsheet,document presentation,document text,document web,document email,comment,\
    list box,grouping,image map,notification,info bar,level bar,title bar,block quote,audio,video,\
    definition,article,landmark,log,marquee,math,rating,timer,static,math fraction,math root,\
    subscript,superscript,description list,description term,description value,footnote,\
    content deletion,content insertion,mark,suggestion,push button menu";

/// `role`'s name in at-spi2-core 2.52's table, such as `push button`; `unknown`, as libatspi
/// says, for a number past the table.
pub(crate) fn role_name(role: Role) -> &'static str {
    let name = usize::try_from(role.0).ok().and_then(|index| ROLE_NAMES.split(',').nth(index));
    name.unwrap_or("unknown")
}

/// `element` as `Grounding` gives it, in `window`: its role's name, its name, its automation id,
/// its box in its window (all zero without `Component`) and its interfaces' short names. Any
/// failure to read the automation id leaves it out: an app that predates `AccessibleId` answers
/// `UnknownProperty` (libdbus) or `InvalidArgs` (`GDBus`), and the id is only a hint.
pub(crate) async fn element_info(
    atspi: &Atspi,
    element: &Accessible,
    window: WindowRef,
) -> Result<ElementInfo, DbusError> {
    let rect = if element.interfaces.contains(Interfaces::COMPONENT) {
        atspi.extents(&element.object).await?
    } else {
        Rect::default()
    };
    Ok(ElementInfo {
        control_type: role_name(element.role).to_owned(),
        name: element.name.clone(),
        automation_id: atspi.accessible_id(&element.object).await.ok().flatten(),
        rect,
        patterns: element.interfaces.short_names().map(str::to_owned).collect(),
        is_password: element.role == Role::PASSWORD_TEXT,
        window,
    })
}

/// The focused element of the foreground window, with that window: the walk of the window's
/// tree within `limits`, which starts no read once four fifths of `budget` have passed, and in
/// it the first `FOCUSED` object, breadth first, then its `FOCUSED` child while it has one.
///
/// - `None` when no window is active, or when the whole tree was read and nothing in it is
///   focused.
/// - A walk cut short, by its limits, its budget or a failed read, that found nothing focused is
///   [`DbusError::DeadlineExceeded`]: the focus is unknown, not absent.
pub(crate) async fn focus(
    atspi: &Atspi,
    limits: TreeLimits,
    budget: Duration,
) -> Result<Option<(Accessible, WindowRef)>, DbusError> {
    let until = Instant::now() + search_budget(budget);
    let windows = toplevels(atspi).await?;
    let Some(front) = windows.first().filter(|front| front.is_active()) else { return Ok(None) };
    let window = WindowRef { hwnd: window_id(&front.window.object), pid: front.pid };
    let walk = atspi.walk(&front.window.object, limits, until).await?;
    match deepest_focus(&walk) {
        Some(node) => Ok(Some((node.accessible.clone(), window))),
        None if walk.truncated => Err(DbusError::DeadlineExceeded { call: FOCUS }),
        None => Ok(None),
    }
}

/// How long a focus search within a call's `budget` reads: four fifths of it, which leaves the
/// rest for the read under way when it ends, at most one call timeout, and the element's own
/// reads; at most [`LONGEST_SEARCH`].
fn search_budget(budget: Duration) -> Duration {
    (budget.saturating_mul(4) / 5).min(LONGEST_SEARCH)
}

/// The first `FOCUSED` node of `walk`, then its first `FOCUSED` child while it has one.
fn deepest_focus(walk: &Walk) -> Option<&WalkNode> {
    let focused = |node: &WalkNode| node.accessible.states.contains(State::FOCUSED);
    let mut at = walk.nodes.iter().position(focused)?;
    while let Some(child) = walk.nodes.iter().position(|node| {
        node.parent.is_some_and(|parent| usize::try_from(parent) == Ok(at)) && focused(node)
    }) {
        at = child;
    }
    walk.nodes.get(at)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{LONGEST_SEARCH, ROLE_NAMES, Role, role_name, search_budget};

    #[test]
    fn a_focus_search_reads_for_four_fifths_of_its_deadline() {
        let millis = Duration::from_millis;
        assert_eq!(search_budget(millis(5_500)), millis(4_400), "the default deadline's");
        assert_eq!(search_budget(millis(100)), millis(80), "a short one's");
        assert_eq!(search_budget(Duration::MAX), LONGEST_SEARCH, "never past the longest");
    }

    #[test]
    fn control_types_are_at_spi_role_names() {
        assert_eq!(ROLE_NAMES.split(',').count(), 130, "roles 0 to 129");
        assert_eq!(role_name(Role(40)), "password text", "ATSPI_ROLE_PASSWORD_TEXT");
        assert_eq!(role_name(Role(23)), "frame", "ATSPI_ROLE_FRAME");
        assert_eq!(role_name(Role(0)), "invalid", "the first");
        assert_eq!(role_name(Role(129)), "push button menu", "the last");
        assert_eq!(role_name(Role(130)), "unknown", "ATSPI_ROLE_LAST_DEFINED is no role");
        assert_eq!(role_name(Role(200)), "unknown", "past the table");
    }
}
