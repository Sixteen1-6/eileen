//! The tray's menu as dbusmenu shows it (`com.canonical.dbusmenu`, version 3): the ids that stay
//! with Eileen's `TrayItem`s, the revision, and the layout and properties a host fetches.
//!
//! - Each `TrayItem::id` gets a dbusmenu id the first time a menu holds it, and keeps it for the
//!   tray's life. New ids count up from 1, 0 being the root, and an id is never given twice, so
//!   a click on a layout the host fetched before a change reaches the same item, or none once it
//!   has gone, never another. Of items that share an id, the first is shown.
//! - The revision moves on every change of the menu, and only then.
//! - The root is `(0, {children-display: "submenu"}, items)`; each item is `(id, {type, label,
//!   enabled, visible}, [])`, with `toggle-type` `checkmark` and `toggle-state` 1 or 0 when it
//!   has a check. A label's `_` is doubled, since dbusmenu reads one as a mnemonic's mark, and a
//!   U+0000 is dropped, since the bus drops a connection that sends one.
//! - Properties go in their names' order, so a host reads the same bytes for the same menu.

use std::collections::{BTreeMap, HashMap};

use eileen_platform::TrayItem;
use zbus::zvariant::Value;

/// The root's id.
pub(crate) const ROOT: i32 = 0;

/// One node's properties, by name.
pub(crate) type Properties = BTreeMap<String, Value<'static>>;
/// A node as `GetLayout` sends it, `(ia{sv}av)`: its id, its properties and its children, each a
/// variant that holds a node.
pub(crate) type Layout = (i32, Properties, Vec<Value<'static>>);

/// The menu: what it shows and the ids it has given.
#[derive(Debug)]
pub(crate) struct Menu {
    /// Every `TrayItem::id` the menu has held, with its dbusmenu id.
    ids: BTreeMap<String, i32>,
    /// The id the next new item gets.
    next: i32,
    /// The items shown, top to bottom, each with its dbusmenu id.
    items: Vec<(i32, TrayItem)>,
    revision: u32,
}

impl Menu {
    /// An empty menu, at revision 0.
    pub(crate) const fn new() -> Self {
        Self { ids: BTreeMap::new(), next: ROOT + 1, items: Vec::new(), revision: 0 }
    }

    /// The layout's revision.
    pub(crate) const fn revision(&self) -> u32 {
        self.revision
    }

    /// Shows `items`, top to bottom; whether that changed the menu.
    pub(crate) fn set(&mut self, items: &[TrayItem]) -> bool {
        let mut shown: Vec<(i32, TrayItem)> = Vec::with_capacity(items.len());
        for item in items {
            if shown.iter().all(|(_, other)| other.id != item.id) {
                shown.push((self.id_of(&item.id), item.clone()));
            }
        }
        if shown == self.items {
            return false;
        }
        self.items = shown;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// The dbusmenu id of the item `item`, given now if it has none. Past `i32::MAX` ids, two
    /// billion items, every new one would share the last.
    fn id_of(&mut self, item: &str) -> i32 {
        if let Some(id) = self.ids.get(item) {
            return *id;
        }
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.ids.insert(item.to_owned(), id);
        id
    }

    /// The item shown under the dbusmenu id `id`; `None` for the root and an id shown no more.
    pub(crate) fn item(&self, id: i32) -> Option<&TrayItem> {
        self.items.iter().find(|(shown, _)| *shown == id).map(|(_, item)| item)
    }

    /// Whether `id` is the root's or a shown item's.
    pub(crate) fn knows(&self, id: i32) -> bool {
        id == ROOT || self.item(id).is_some()
    }

    /// The root's id, then every shown item's.
    pub(crate) fn ids(&self) -> impl Iterator<Item = i32> + '_ {
        std::iter::once(ROOT).chain(self.items.iter().map(|(id, _)| *id))
    }

    /// The node `parent` with its children `depth` levels down (-1 for all), each with the
    /// properties `names` lists (all of them when it lists none); `None` for an unknown id.
    pub(crate) fn layout(&self, parent: i32, depth: i32, names: &[String]) -> Option<Layout> {
        if parent != ROOT {
            let item = self.item(parent)?;
            return Some((parent, item_properties(item, names), Vec::new()));
        }
        let children = if depth == 0 {
            Vec::new()
        } else {
            // A child is a variant, whose map becomes a `Dict`, which keeps its keys in order too.
            let node = |(id, item): &(i32, TrayItem)| {
                let properties: HashMap<_, _> = item_properties(item, names).into_iter().collect();
                Value::from((*id, properties, Vec::<Value<'static>>::new()))
            };
            self.items.iter().map(node).collect()
        };
        Some((ROOT, root_properties(names), children))
    }

    /// The properties of the node `id` that `names` lists (all when it lists none); `None` for an
    /// unknown id.
    pub(crate) fn properties(&self, id: i32, names: &[String]) -> Option<Properties> {
        if id == ROOT {
            return Some(root_properties(names));
        }
        self.item(id).map(|item| item_properties(item, names))
    }
}

/// The root's properties among `names`.
fn root_properties(names: &[String]) -> Properties {
    among(vec![("children-display", Value::from("submenu"))], names)
}

/// `item`'s properties among `names`.
fn item_properties(item: &TrayItem, names: &[String]) -> Properties {
    let label = item.label.replace('\0', "").replace('_', "__");
    let mut all = vec![
        ("type", Value::from("standard")),
        ("label", Value::from(label)),
        ("enabled", Value::from(true)),
        ("visible", Value::from(true)),
    ];
    if let Some(checked) = item.checked {
        all.push(("toggle-type", Value::from("checkmark")));
        all.push(("toggle-state", Value::from(i32::from(checked))));
    }
    among(all, names)
}

/// Those of `all` that `names` lists, or all of them when it lists none.
fn among(all: Vec<(&str, Value<'static>)>, names: &[String]) -> Properties {
    all.into_iter()
        .filter(|(name, _)| names.is_empty() || names.iter().any(|listed| listed == name))
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

#[cfg(test)]
mod tests {
    use eileen_platform::TrayItem;
    use zbus::zvariant::{Structure, Value};

    use super::{Menu, Properties, ROOT};

    /// An item with this id and label, and no check.
    fn item(id: &str, label: &str) -> TrayItem {
        TrayItem { id: id.to_owned(), label: label.to_owned(), checked: None }
    }

    /// The text property `name` of `properties`.
    fn text<'a>(properties: &'a Properties, name: &str) -> Option<&'a str> {
        let value = properties.get(name)?;
        let Value::Str(text) = value else { panic!("{name} is not text: {value:?}") };
        Some(text.as_str())
    }

    /// The ids of the nodes in `children`, as a layout sends them, each in a variant.
    fn ids_of(children: &[Value<'static>]) -> Vec<i32> {
        let id = |child: &Value<'static>| -> i32 {
            let Value::Structure(node) = child else { panic!("a node is a structure: {child:?}") };
            let fields = Structure::fields(node);
            let [Value::I32(id), Value::Dict(_), Value::Array(_)] = fields else {
                panic!("a node is (ia{{sv}}av): {fields:?}")
            };
            *id
        };
        children.iter().map(id).collect()
    }

    #[test]
    fn ids_stay_with_their_items_across_updates() {
        let mut menu = Menu::new();
        assert!(menu.set(&[item("mute", "Mute"), item("quit", "Quit")]), "a change");
        let (_, _, children) = menu.layout(ROOT, -1, &[]).unwrap();
        assert_eq!(ids_of(&children), [1, 2], "counted up from 1, after the root");
        assert!(menu.set(&[item("about", "About"), item("quit", "Quit Eileen")]), "a change");
        let (_, _, children) = menu.layout(ROOT, -1, &[]).unwrap();
        assert_eq!(ids_of(&children), [3, 2], "a new item takes a new id, a kept one its own");
        assert_eq!(menu.item(1), None, "a removed item's id reaches nothing");
        assert_eq!(menu.item(2).map(|item| item.label.as_str()), Some("Quit Eileen"), "relabelled");
        assert!(menu.set(&[item("mute", "Unmute")]), "a change");
        let (_, _, children) = menu.layout(ROOT, -1, &[]).unwrap();
        assert_eq!(ids_of(&children), [1], "an item back again takes its first id");
        assert!(menu.set(&[item("new", "New"), item("new", "Again")]), "a change");
        let (_, _, children) = menu.layout(ROOT, -1, &[]).unwrap();
        assert_eq!(ids_of(&children), [4], "an id is never given twice, and a repeat shows once");
        assert_eq!(menu.item(4).map(|item| item.label.as_str()), Some("New"), "the first");
    }

    #[test]
    fn the_revision_moves_on_each_change_and_only_then() {
        let mut menu = Menu::new();
        assert_eq!(menu.revision(), 0, "a new menu");
        assert!(!menu.set(&[]), "still empty");
        assert_eq!(menu.revision(), 0, "no change");
        assert!(menu.set(&[item("mute", "Mute")]), "an item");
        assert_eq!(menu.revision(), 1, "one change");
        assert!(!menu.set(&[item("mute", "Mute")]), "the same menu");
        assert_eq!(menu.revision(), 1, "no change");
        let checked = TrayItem { checked: Some(false), ..item("mute", "Mute") };
        assert!(menu.set(&[checked]), "a check");
        assert_eq!(menu.revision(), 2, "a check is a change");
    }

    #[test]
    fn a_toggle_item_is_a_checkmark_and_a_plain_one_has_none() {
        let mut menu = Menu::new();
        let on = TrayItem { checked: Some(true), ..item("on", "On") };
        let off = TrayItem { checked: Some(false), ..item("off", "Off") };
        menu.set(&[on, off, item("plain", "Plain")]);
        for (id, state) in [(1, 1), (2, 0)] {
            let properties = menu.properties(id, &[]).unwrap();
            assert_eq!(text(&properties, "toggle-type"), Some("checkmark"), "item {id}");
            assert_eq!(properties.get("toggle-state"), Some(&Value::I32(state)), "item {id}");
        }
        let plain = menu.properties(3, &[]).unwrap();
        assert_eq!(text(&plain, "toggle-type"), None, "no toggle");
        assert_eq!(plain.get("toggle-state"), None, "no state");
        assert_eq!(text(&plain, "type"), Some("standard"), "a standard item");
        assert_eq!(plain.get("enabled"), Some(&Value::Bool(true)), "enabled");
        assert_eq!(plain.get("visible"), Some(&Value::Bool(true)), "visible");
        let names = |id: i32| menu.properties(id, &[]).unwrap().into_keys().collect::<Vec<_>>();
        let toggle = ["enabled", "label", "toggle-state", "toggle-type", "type", "visible"];
        assert_eq!(names(1), toggle, "every property, in their names' order");
        assert_eq!(names(3), ["enabled", "label", "type", "visible"], "a plain item's");
    }

    #[test]
    fn a_label_doubles_its_underscores_and_loses_any_nul() {
        let mut menu = Menu::new();
        menu.set(&[item("save", "Save_a\0 report__now")]);
        let properties = menu.properties(1, &[]).unwrap();
        assert_eq!(
            text(&properties, "label"),
            Some("Save__a report____now"),
            "no mnemonic, no NUL"
        );
    }

    #[test]
    fn the_layout_honours_recursion_depth_and_property_names() {
        let mut menu = Menu::new();
        menu.set(&[item("mute", "Mute"), item("quit", "Quit")]);
        let (id, root, children) = menu.layout(ROOT, 0, &[]).unwrap();
        assert_eq!((id, children.len()), (ROOT, 0), "depth 0: the root alone");
        assert_eq!(text(&root, "children-display"), Some("submenu"), "the root has a submenu");
        for depth in [-1, 1, 5] {
            let (_, _, children) = menu.layout(ROOT, depth, &[]).unwrap();
            assert_eq!(ids_of(&children), [1, 2], "depth {depth}: every item");
        }
        let (id, quit, children) = menu.layout(2, -1, &[]).unwrap();
        assert_eq!((id, children.len()), (2, 0), "an item alone, which has no children");
        assert_eq!(text(&quit, "label"), Some("Quit"), "its properties");
        let names = ["label".to_owned(), "no-such-property".to_owned()];
        let (_, root, _) = menu.layout(ROOT, -1, &names).unwrap();
        assert!(root.is_empty(), "the root has no label: {root:?}");
        let quit = menu.properties(2, &names).unwrap();
        assert_eq!(quit.keys().collect::<Vec<_>>(), ["label"], "the label alone");
        assert!(menu.layout(3, -1, &[]).is_none(), "an unknown parent");
        assert!(menu.properties(3, &[]).is_none(), "an unknown item");
        assert_eq!(menu.ids().collect::<Vec<_>>(), [ROOT, 1, 2], "the root and the items");
        assert!(menu.knows(ROOT) && menu.knows(2) && !menu.knows(3), "the root and items only");
    }
}
