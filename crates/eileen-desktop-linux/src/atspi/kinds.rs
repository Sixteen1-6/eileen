//! What AT-SPI2's numbers and names mean: an object's role, its states and the interfaces it
//! implements. The numbers are libatspi 2.52's own (`AtspiRole` and `AtspiStateType`, read from
//! its enum tables), and the interface names those of at-spi2-core's introspection. These tables
//! are the only source of a role's, a state's or an interface's meaning: an app's own role names
//! (`GetRoleName`) are never read.

use std::ops::BitOr;

/// An object's role: its number in `AtspiRole`, as `GetRole` answers it. Only the roles this
/// crate acts on are named; any other number is kept as it came.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Role(pub u32);

impl Role {
    /// A dialog box.
    pub const DIALOG: Self = Self(16);
    /// A top-level window with a frame, as most apps' main windows are.
    pub const FRAME: Self = Self(23);
    /// A text field that hides its text: a password field, whose text is never read.
    pub const PASSWORD_TEXT: Self = Self(40);
    /// A top-level window without a frame.
    pub const WINDOW: Self = Self(69);
    /// An app's root object, whose children are its windows.
    pub const APPLICATION: Self = Self(75);
}

/// One state an object can be in: its number in `AtspiStateType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct State(pub u32);

impl State {
    /// Its window is the active one.
    pub const ACTIVE: Self = Self(1);
    /// It is gone: the app no longer backs it.
    pub const DEFUNCT: Self = Self(6);
    /// Its text can be edited.
    pub const EDITABLE: Self = Self(7);
    /// It has the keyboard focus.
    pub const FOCUSED: Self = Self(12);
    /// Its window is minimized.
    pub const ICONIFIED: Self = Self(15);
    /// It and every object above it are shown: it is on screen unless something covers it.
    pub const SHOWING: Self = Self(25);
    /// It has many children, built on demand, as a big table has: they are read by index, never
    /// all at once.
    pub const MANAGES_DESCENDANTS: Self = Self(31);
    /// Its value cannot be changed.
    pub const READ_ONLY: Self = Self(43);
}

/// The states an object is in, from `GetState`'s answer: two 32-bit words, the first holding
/// states 0 to 31 and the second states 32 to 63, state `n` at bit `n % 32` of its word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StateSet(u64);

impl StateSet {
    /// The set `words` gives. A missing word holds no state, and words after the second are
    /// ignored.
    #[must_use]
    pub fn from_words(words: &[u32]) -> Self {
        let word = |index: usize| u64::from(words.get(index).copied().unwrap_or(0));
        Self(word(0) | (word(1) << 32))
    }

    /// Whether the set holds `state`.
    #[must_use]
    pub const fn contains(self, state: State) -> bool {
        state.0 < 64 && self.0 & (1 << state.0) != 0
    }
}

/// The interfaces an object implements, as `GetInterfaces` names them, among the fifteen of
/// AT-SPI2 this crate knows; any other name is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Interfaces(u16);

impl Interfaces {
    /// `org.a11y.atspi.Accessible`: the name, role, states and children every object has.
    pub const ACCESSIBLE: Self = Self(1);
    /// `org.a11y.atspi.Action`: actions such as a button's click.
    pub const ACTION: Self = Self(1 << 1);
    /// `org.a11y.atspi.Application`: an app's root.
    pub const APPLICATION: Self = Self(1 << 2);
    /// `org.a11y.atspi.Collection`: searches of the object's subtree.
    pub const COLLECTION: Self = Self(1 << 3);
    /// `org.a11y.atspi.Component`: the object's place on screen.
    pub const COMPONENT: Self = Self(1 << 4);
    /// `org.a11y.atspi.Document`: a document's attributes.
    pub const DOCUMENT: Self = Self(1 << 5);
    /// `org.a11y.atspi.EditableText`: text that can be changed.
    pub const EDITABLE_TEXT: Self = Self(1 << 6);
    /// `org.a11y.atspi.Hyperlink`: a link.
    pub const HYPERLINK: Self = Self(1 << 7);
    /// `org.a11y.atspi.Hypertext`: text that holds links.
    pub const HYPERTEXT: Self = Self(1 << 8);
    /// `org.a11y.atspi.Image`: an image's description and size.
    pub const IMAGE: Self = Self(1 << 9);
    /// `org.a11y.atspi.Selection`: children that can be selected.
    pub const SELECTION: Self = Self(1 << 10);
    /// `org.a11y.atspi.Table`: rows and columns.
    pub const TABLE: Self = Self(1 << 11);
    /// `org.a11y.atspi.TableCell`: one cell of a table.
    pub const TABLE_CELL: Self = Self(1 << 12);
    /// `org.a11y.atspi.Text`: text that can be read.
    pub const TEXT: Self = Self(1 << 13);
    /// `org.a11y.atspi.Value`: a number in a range, as a slider's.
    pub const VALUE: Self = Self(1 << 14);

    /// The set `names` gives, each name compared whole with the table's.
    #[must_use]
    pub fn from_names<S: AsRef<str>>(names: &[S]) -> Self {
        names
            .iter()
            .filter_map(|name| NAMES.iter().find(|(known, _)| *known == name.as_ref()))
            .fold(Self::default(), |set, (_, interface)| set | *interface)
    }

    /// Whether the set holds every interface of `other`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for Interfaces {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Each interface's name, as AT-SPI2 gives it.
const NAMES: [(&str, Interfaces); 15] = [
    ("org.a11y.atspi.Accessible", Interfaces::ACCESSIBLE),
    ("org.a11y.atspi.Action", Interfaces::ACTION),
    ("org.a11y.atspi.Application", Interfaces::APPLICATION),
    ("org.a11y.atspi.Collection", Interfaces::COLLECTION),
    ("org.a11y.atspi.Component", Interfaces::COMPONENT),
    ("org.a11y.atspi.Document", Interfaces::DOCUMENT),
    ("org.a11y.atspi.EditableText", Interfaces::EDITABLE_TEXT),
    ("org.a11y.atspi.Hyperlink", Interfaces::HYPERLINK),
    ("org.a11y.atspi.Hypertext", Interfaces::HYPERTEXT),
    ("org.a11y.atspi.Image", Interfaces::IMAGE),
    ("org.a11y.atspi.Selection", Interfaces::SELECTION),
    ("org.a11y.atspi.Table", Interfaces::TABLE),
    ("org.a11y.atspi.TableCell", Interfaces::TABLE_CELL),
    ("org.a11y.atspi.Text", Interfaces::TEXT),
    ("org.a11y.atspi.Value", Interfaces::VALUE),
];

#[cfg(test)]
mod tests {
    use super::{State, StateSet};

    #[test]
    fn states_read_both_words() {
        let low = (1 << 25) | (1 << 12);
        let high = 1 << (43 - 32);
        let set = StateSet::from_words(&[low, high]);
        for state in [State::SHOWING, State::FOCUSED, State::READ_ONLY] {
            assert!(set.contains(state), "{state:?} is in the set");
        }
        for state in [State::ACTIVE, State::DEFUNCT, State::MANAGES_DESCENDANTS] {
            assert!(!set.contains(state), "{state:?} is not");
        }
        assert!(
            !StateSet::from_words(&[low]).contains(State::READ_ONLY),
            "one word, no high state"
        );
        assert_eq!(StateSet::from_words(&[low, high, u32::MAX]), set, "a third word is ignored");
        assert_eq!(StateSet::from_words(&[]), StateSet::default(), "no words, no states");
        assert!(!StateSet::from_words(&[u32::MAX, u32::MAX]).contains(State(64)), "past the words");
    }
}
