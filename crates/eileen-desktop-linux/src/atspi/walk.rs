//! [`Atspi::walk`]: an object's tree, breadth first, within the platform's `TreeLimits` and a
//! time budget, as `Automation::tree` reads a window's on Windows.

use std::collections::HashSet;

use eileen_dbus_linux::DbusError;
use eileen_platform::TreeLimits;
use tokio::time::Instant;

use super::{Accessible, Atspi, ObjectRef, State, StateSet};

/// An object's tree as a walk read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walk {
    /// The objects read, breadth first: the root first, parents before their children.
    pub nodes: Vec<WalkNode>,
    /// True when a limit, the time budget or a failed read left part of the tree out.
    pub truncated: bool,
}

/// One object of a [`Walk`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkNode {
    /// What it is.
    pub accessible: Accessible,
    /// The index of its parent in the walk's nodes; `None` for the root.
    pub parent: Option<u32>,
    /// Its level: the root's is 0.
    pub depth: u16,
}

impl Atspi {
    /// The tree under `root`, read breadth first, at most `limits.max_nodes` objects of it.
    ///
    /// - The root is read whatever the budget, and its failure is the walk's error, as
    ///   [`Atspi::accessible`] gives it: [`DbusError::NotFound`] when its app has gone. It is
    ///   entered whatever its states, since an app's root reports none.
    /// - Below the root, an object is read but not entered when it is not `SHOWING` or is
    ///   `DEFUNCT`: hidden menus and pages, and what an app has let go, stay out.
    /// - No object at `limits.max_depth` is entered.
    /// - An object's children are listed as [`Atspi::children`] lists them, within the room the
    ///   node limit leaves. An object the walk already holds is not read again, so a tree that
    ///   loops still ends.
    /// - A descendant whose read fails (gone, unknown, past its call timeout) is left out with
    ///   its subtree, and the walk goes on.
    /// - Once `until` has passed, no read starts. That is a check between reads, not a timer
    ///   around a call, so the walk ends at most one call timeout after `until`.
    ///
    /// The walk is `truncated` when a limit, the budget or a failed read left out part of the
    /// tree; an object that is not showing or is defunct leaves out nothing on screen.
    pub async fn walk(
        &self,
        root: &ObjectRef,
        limits: TreeLimits,
        until: Instant,
    ) -> Result<Walk, DbusError> {
        if limits.max_nodes == 0 {
            return Ok(Walk { nodes: Vec::new(), truncated: true });
        }
        let first = WalkNode { accessible: self.accessible(root).await?, parent: None, depth: 0 };
        let mut walk = Walk { nodes: vec![first], truncated: false };
        let mut seen = HashSet::from([root.clone()]);
        let mut next = 0;
        while let Some(node) = walk.nodes.get(next) {
            let Ok(parent) = u32::try_from(next) else { break };
            next += 1;
            let (object, depth) = (&node.accessible, node.depth);
            if object.child_count == 0 || (depth > 0 && !enterable(object.states)) {
                continue;
            }
            if depth >= limits.max_depth {
                walk.truncated = true;
                continue;
            }
            let held = u32::try_from(walk.nodes.len()).unwrap_or(u32::MAX);
            let left = limits.max_nodes.saturating_sub(held);
            if left == 0 || Instant::now() >= until {
                walk.truncated = true;
                break;
            }
            let Ok(listing) = self.list(object, left, Some(until)).await else {
                walk.truncated = true;
                continue;
            };
            walk.truncated |= listing.cut;
            for child in listing.refs {
                if !seen.insert(child.clone()) {
                    continue;
                }
                if Instant::now() >= until {
                    walk.truncated = true;
                    return Ok(walk);
                }
                match self.accessible(&child).await {
                    Ok(accessible) => {
                        let depth = depth + 1;
                        walk.nodes.push(WalkNode { accessible, parent: Some(parent), depth });
                    }
                    Err(_) => walk.truncated = true,
                }
            }
        }
        Ok(walk)
    }
}

/// Whether a walk enters an object in `states`: it is on screen and still there.
const fn enterable(states: StateSet) -> bool {
    states.contains(State::SHOWING) && !states.contains(State::DEFUNCT)
}
