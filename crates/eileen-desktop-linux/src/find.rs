//! How `Automation::find` ranks a window's elements against a `Selector`: a score per element of
//! a walk, from the selector's fields and its ancestor steps. It is provisional until WP5 sets
//! selector identity (BUILD.md section 2.10 item 8), which may weigh the fields otherwise.
//!
//! - `automation_id`, when given, must equal the element's: 8.
//! - `control_type`, when given, must equal its role's name: 2.
//! - `name`, when given, must equal it (4), equal it in another case (3) or be inside it in any
//!   case (1).
//! - Each ancestor step, nearest first, whose given fields match the element's ancestor at that
//!   distance by the same rules: 1. A step that gives no field adds nothing, and a step that does
//!   not match takes nothing away.
//!
//! A selector that gives none of the element's three fields matches nothing: it would match every
//! element alike.

use std::cmp::Reverse;

use eileen_contracts::desktop::{Selector, SelectorStep};
use eileen_contracts::ids::Untrusted;

use crate::atspi::{Accessible, Walk};
use crate::element::role_name;

/// The most candidates `find` answers.
pub(crate) const MOST_CANDIDATES: usize = 16;

/// The fields one element or step of a selector gives.
struct Fields<'a> {
    automation_id: Option<&'a Untrusted<String>>,
    control_type: Option<&'a str>,
    name: Option<&'a Untrusted<String>>,
}

impl Fields<'_> {
    /// Whether it gives none.
    const fn is_empty(&self) -> bool {
        self.automation_id.is_none() && self.control_type.is_none() && self.name.is_none()
    }

    /// The score of `element`, whose automation id is `id`, by the module's rules; `None` when a
    /// given field does not match.
    fn score(&self, element: &Accessible, id: Option<&Untrusted<String>>) -> Option<u16> {
        let mut score = 0;
        if let Some(wanted) = self.automation_id {
            (id.map(Untrusted::get) == Some(wanted.get())).then_some(())?;
            score += 8;
        }
        if let Some(wanted) = self.control_type {
            (role_name(element.role) == wanted).then_some(())?;
            score += 2;
        }
        if let Some(wanted) = self.name {
            score += name_score(element.name.get(), wanted.get())?;
        }
        Some(score)
    }
}

impl<'a> From<&'a SelectorStep> for Fields<'a> {
    fn from(step: &'a SelectorStep) -> Self {
        Self {
            automation_id: step.automation_id.as_ref(),
            control_type: step.control_type.as_deref(),
            name: step.name.as_ref(),
        }
    }
}

/// How well `name` matches `wanted`: 4 equal, 3 equal in another case, 1 holding it in any case.
fn name_score(name: &str, wanted: &str) -> Option<u16> {
    if name == wanted {
        return Some(4);
    }
    let (name, wanted) = (name.to_lowercase(), wanted.to_lowercase());
    if name == wanted {
        Some(3)
    } else if !wanted.is_empty() && name.contains(&wanted) {
        Some(1)
    } else {
        None
    }
}

/// Whether ranking against `selector` needs each element's automation id.
pub(crate) fn needs_ids(selector: &Selector) -> bool {
    selector.automation_id.is_some()
        || selector.ancestors.iter().any(|step| step.automation_id.is_some())
}

/// The nodes of `walk` that match `selector`, as indices with their scores, best first and ties
/// in the walk's order, at most [`MOST_CANDIDATES`]. `ids` holds each node's automation id, by
/// index; a node past its end has none.
pub(crate) fn rank(
    walk: &Walk,
    ids: &[Option<Untrusted<String>>],
    selector: &Selector,
) -> Vec<(usize, u16)> {
    let element = Fields {
        automation_id: selector.automation_id.as_ref(),
        control_type: selector.control_type.as_deref(),
        name: selector.name.as_ref(),
    };
    if element.is_empty() {
        return Vec::new();
    }
    let id = |index: usize| ids.get(index).and_then(Option::as_ref);
    let mut ranked: Vec<(usize, u16)> = (walk.nodes.iter().enumerate())
        .filter_map(|(index, node)| {
            let mut score = element.score(&node.accessible, id(index))?;
            let mut parent = node.parent;
            for step in &selector.ancestors {
                let Some(at) = parent.and_then(|at| usize::try_from(at).ok()) else { break };
                let Some(ancestor) = walk.nodes.get(at) else { break };
                let step = Fields::from(step);
                if !step.is_empty() && step.score(&ancestor.accessible, id(at)).is_some() {
                    score += 1;
                }
                parent = ancestor.parent;
            }
            Some((index, score))
        })
        .collect();
    ranked.sort_by_key(|&(_, score)| Reverse(score));
    ranked.truncate(MOST_CANDIDATES);
    ranked
}

#[cfg(test)]
mod tests {
    use eileen_contracts::desktop::{Selector, SelectorStep};
    use eileen_contracts::ids::Untrusted;
    use zbus::names::OwnedUniqueName;
    use zbus::zvariant::OwnedObjectPath;

    use super::{MOST_CANDIDATES, needs_ids, rank};
    use crate::atspi::{Accessible, Interfaces, ObjectRef, Role, StateSet, Walk, WalkNode};

    /// AT-SPI2's role numbers, written out.
    const FRAME: u32 = 23;
    const PANEL: u32 = 39;
    const PUSH_BUTTON: u32 = 43;

    /// A node of `role` named `name` under `parent`.
    fn node(index: usize, role: u32, name: &str, parent: Option<u32>) -> WalkNode {
        let bus = OwnedUniqueName::try_from(":1.5").unwrap();
        let path = OwnedObjectPath::try_from(format!("/n{index}")).unwrap();
        let accessible = Accessible {
            object: ObjectRef { bus, path },
            name: Untrusted::new(name.to_owned()),
            role: Role(role),
            states: StateSet::default(),
            interfaces: Interfaces::default(),
            child_count: 0,
        };
        WalkNode { accessible, parent, depth: 0 }
    }

    fn text(text: &str) -> Untrusted<String> {
        Untrusted::new(text.to_owned())
    }

    fn selector() -> Selector {
        Selector {
            process: "files".to_owned(),
            window_class: None,
            automation_id: None,
            control_type: None,
            name: None,
            ancestors: Vec::new(),
        }
    }

    #[test]
    fn scores_follow_the_selectors_fields() {
        let nodes = vec![
            node(0, FRAME, "Files", None),
            node(1, PANEL, "Toolbar", Some(0)),
            node(2, PUSH_BUTTON, "Save", Some(1)),
            node(3, PUSH_BUTTON, "save as", Some(1)),
            node(4, PUSH_BUTTON, "Autosave", Some(0)),
            node(5, PANEL, "Save", Some(0)),
        ];
        let walk = Walk { nodes, truncated: false };
        let ids = vec![None, None, Some(text("save")), None, None, Some(text("save"))];
        let by = |selector: &Selector| rank(&walk, &ids, selector);
        let name = Selector { name: Some(text("Save")), ..selector() };
        assert_eq!(by(&name), [(2, 4), (5, 4), (3, 1), (4, 1)], "exact, then contained, any case");
        let button = Selector { control_type: Some("push button".to_owned()), ..name.clone() };
        assert_eq!(by(&button), [(2, 6), (3, 3), (4, 3)], "the control type is required");
        let lower = Selector { name: Some(text("save")), ..selector() };
        assert_eq!(by(&lower)[..2], [(2, 3), (5, 3)], "another case scores 3");
        let id = Selector { automation_id: Some(text("save")), ..selector() };
        assert!(needs_ids(&id), "an automation id is read only when asked for");
        assert!(!needs_ids(&button), "and not otherwise");
        assert_eq!(by(&id), [(2, 8), (5, 8)], "the automation id is required, and exact");
        let step = |name: &str| SelectorStep {
            control_type: Some("panel".to_owned()),
            name: Some(text(name)),
            automation_id: None,
        };
        let nested = Selector { ancestors: vec![step("toolbar"), step("Files")], ..button };
        assert_eq!(
            by(&nested),
            [(2, 7), (3, 4), (4, 3)],
            "the toolbar step matches, the frame not"
        );
        let empty = Selector { ancestors: vec![step("Toolbar")], ..selector() };
        assert!(by(&empty).is_empty(), "no field of the element's own: nothing");
        let many = Walk {
            nodes: (0..40).map(|index| node(index, PUSH_BUTTON, "Save", None)).collect(),
            truncated: false,
        };
        assert_eq!(rank(&many, &[], &name).len(), MOST_CANDIDATES, "at most 16");
    }
}
