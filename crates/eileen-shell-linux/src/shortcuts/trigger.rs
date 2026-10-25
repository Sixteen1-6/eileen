//! The triggers a shortcut is offered with, in the form of the XDG shortcuts specification
//! ([version 0.1][spec]), which GNOME's and KDE's portal backends both read: modifiers, each
//! named `CTRL`, `ALT`, `SHIFT` or `LOGO`, then one xkbcommon keysym name, joined by `+`, such
//! as `CTRL+ALT+m`. A trigger is only the app's preference: the desktop's dialog lets the user
//! choose another, and the desktop keeps that choice.
//!
//! - [`trigger_of_chord`] reads a hotkey chord as Eileen's settings write it (`ctrl+alt+m`):
//!   parts split on `+`, trimmed, in any case. `ctrl` or `control`, `alt`, `shift`, and `super`,
//!   `win`, `logo` or `meta` are the modifiers, each at most once, then exactly one key: a
//!   letter, a digit, `F1` to `F24`, or one of the keysyms in [`KEYS`], with or without its `_`.
//!   The trigger names the modifiers in the order above and a letter in lower case, as the
//!   specification's base layer does, so a chord's spelling never changes its trigger.
//! - [`trigger_of_key`] names the Windows virtual-key code of a dictation key alone, without
//!   modifiers; a code it does not know gives no trigger, and the user picks one in the dialog.
//!
//! [spec]: https://specifications.freedesktop.org/shortcuts/latest/

use eileen_platform::VirtualKey;

use crate::DbusError;

/// What a chord's errors name.
const CHORD: &str = "hotkey chord";

/// The modifiers in the order a trigger names them, each with the words a chord may give it.
const MODIFIERS: [(&str, &[&str]); 4] = [
    ("CTRL", &["ctrl", "control"]),
    ("ALT", &["alt"]),
    ("SHIFT", &["shift"]),
    ("LOGO", &["super", "win", "logo", "meta"]),
];

/// The keys a chord may name besides letters, digits and F-keys: their keysyms, separated by
/// spaces.
const KEYS: &str = "space Return Tab Escape BackSpace Delete Insert Home End Page_Up Page_Down \
                    Up Down Left Right Pause Print Menu Caps_Lock Scroll_Lock";

/// The keysyms of keys whose virtual-key codes run in order: the navigation keys from 0x21, the
/// Windows and menu keys from 0x5B, and the left and right Shift, Ctrl and Alt from 0xA0.
const NAVIGATION: [&str; 8] =
    ["Page_Up", "Page_Down", "End", "Home", "Left", "Up", "Right", "Down"];
const WINDOWS: [&str; 3] = ["Super_L", "Super_R", "Menu"];
const SIDES: [&str; 6] = ["Shift_L", "Shift_R", "Control_L", "Control_R", "Alt_L", "Alt_R"];

/// The trigger a hotkey chord stands for, such as `CTRL+ALT+m` for `ctrl+alt+m`; the module's
/// doc gives the rules. A chord that breaks them is [`DbusError::Invalid`], before the portal
/// is asked anything.
pub fn trigger_of_chord(chord: &str) -> Result<String, DbusError> {
    let invalid = |detail: &str| DbusError::Invalid { what: CHORD, detail: detail.to_owned() };
    let mut held = 0_u8;
    let mut key = None;
    for part in chord.split('+').map(|part| part.trim().to_ascii_lowercase()) {
        if let Some(index) = MODIFIERS.iter().position(|(_, words)| words.contains(&&*part)) {
            let bit = 1 << index;
            if key.is_some() || held & bit != 0 {
                return Err(invalid("a modifier after the key, or twice"));
            }
            held |= bit;
        } else if key.is_some() {
            return Err(invalid("more than one key"));
        } else {
            key = Some(keysym(&part).ok_or_else(|| invalid("a key no trigger can name"))?);
        }
    }
    let key = key.ok_or_else(|| invalid("no key"))?;
    let mut trigger = String::new();
    for (index, (name, _)) in MODIFIERS.iter().enumerate() {
        if held & (1 << index) != 0 {
            trigger.push_str(name);
            trigger.push('+');
        }
    }
    trigger.push_str(&key);
    Ok(trigger)
}

/// The keysym a chord's key `word`, in lower case, names.
fn keysym(word: &str) -> Option<String> {
    let folded: String = word.chars().filter(|c| *c != '_').collect();
    if let [b'a'..=b'z' | b'0'..=b'9'] = folded.as_bytes() {
        return Some(folded);
    }
    let function = folded.strip_prefix('f').and_then(|number| number.parse::<u8>().ok());
    if let Some(number) = function.filter(|number| (1..=24).contains(number)) {
        return Some(format!("F{number}"));
    }
    let named = |name: &&str| name.to_ascii_lowercase().replace('_', "") == folded;
    KEYS.split(' ').find(named).map(str::to_owned)
}

/// The keysym of the Windows virtual-key code `key`, as the dictation key alone: letters,
/// digits, F1 to F24, Space, Caps Lock, Scroll Lock, Pause, Insert, the navigation keys, the
/// Windows and menu keys, and the left and right Ctrl, Alt and Shift. `None` for any other.
#[must_use]
pub fn trigger_of_key(key: VirtualKey) -> Option<String> {
    let code = u8::try_from(key.0).ok()?;
    let name = match code {
        b'0'..=b'9' | b'A'..=b'Z' => return Some(char::from(code.to_ascii_lowercase()).into()),
        0x70..=0x87 => return Some(format!("F{}", code - 0x6F)),
        0x21..=0x28 => *NAVIGATION.get(usize::from(code - 0x21))?,
        0x5B..=0x5D => *WINDOWS.get(usize::from(code - 0x5B))?,
        0xA0..=0xA5 => *SIDES.get(usize::from(code - 0xA0))?,
        0x13 => "Pause",
        0x14 => "Caps_Lock",
        0x20 => "space",
        0x2D => "Insert",
        0x91 => "Scroll_Lock",
        _ => return None,
    };
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use eileen_platform::VirtualKey;
    use proptest::prelude::*;

    use super::{MODIFIERS, trigger_of_chord, trigger_of_key};
    use crate::DbusError;

    #[test]
    fn chords_become_xdg_triggers_with_modifiers_in_order() {
        let cases = [
            ("ctrl+alt+m", "CTRL+ALT+m"),
            ("Ctrl+Alt+Shift+B", "CTRL+ALT+SHIFT+b"),
            ("super+space", "LOGO+space"),
            ("ctrl+f13", "CTRL+F13"),
            (" Control + Win + Page_Up ", "CTRL+LOGO+Page_Up"),
            ("meta+RETURN", "LOGO+Return"),
            ("alt+capslock", "ALT+Caps_Lock"),
            ("7", "7"),
        ];
        for (chord, trigger) in cases {
            assert_eq!(trigger_of_chord(chord).as_deref(), Ok(trigger), "{chord}");
        }
    }

    /// Every key of the table, named in lower case without its `_`, gives its exact xkbcommon
    /// keysym name, whose case the desktop's parser keeps.
    #[test]
    fn each_named_key_gives_its_exact_keysym() {
        let keysyms = [
            ("space", "space"),
            ("return", "Return"),
            ("tab", "Tab"),
            ("escape", "Escape"),
            ("backspace", "BackSpace"),
            ("delete", "Delete"),
            ("insert", "Insert"),
            ("home", "Home"),
            ("end", "End"),
            ("pageup", "Page_Up"),
            ("pagedown", "Page_Down"),
            ("up", "Up"),
            ("down", "Down"),
            ("left", "Left"),
            ("right", "Right"),
            ("pause", "Pause"),
            ("print", "Print"),
            ("menu", "Menu"),
            ("capslock", "Caps_Lock"),
            ("scrolllock", "Scroll_Lock"),
        ];
        assert_eq!(keysyms.len(), super::KEYS.split(' ').count(), "one case for each key");
        for (key, keysym) in keysyms {
            let trigger = format!("CTRL+{keysym}");
            assert_eq!(trigger_of_chord(&format!("ctrl+{key}")), Ok(trigger), "{key}");
        }
    }

    #[test]
    fn a_chord_without_exactly_one_known_key_is_invalid() {
        let cases = [
            ("ctrl+alt", "no key"),
            ("ctrl+m+k", "more than one key"),
            ("ctrl+ctrl+m", "a modifier after the key, or twice"),
            ("m+ctrl", "a modifier after the key, or twice"),
            ("hyper+m", "a key no trigger can name"),
            ("ctrl+f25", "a key no trigger can name"),
            ("", "a key no trigger can name"),
        ];
        for (chord, detail) in cases {
            let invalid = DbusError::Invalid { what: "hotkey chord", detail: detail.to_owned() };
            assert_eq!(trigger_of_chord(chord), Err(invalid), "{chord:?}");
        }
    }

    #[test]
    fn windows_key_codes_name_their_keysyms() {
        let named = [
            // Pause, Caps Lock, Space, Insert and Scroll Lock.
            (0x13, "Pause"),
            (0x14, "Caps_Lock"),
            (0x20, "space"),
            (0x2D, "Insert"),
            (0x91, "Scroll_Lock"),
            // VK_PRIOR to VK_DOWN.
            (0x21, "Page_Up"),
            (0x22, "Page_Down"),
            (0x23, "End"),
            (0x24, "Home"),
            (0x25, "Left"),
            (0x26, "Up"),
            (0x27, "Right"),
            (0x28, "Down"),
            // VK_LWIN, VK_RWIN and VK_APPS.
            (0x5B, "Super_L"),
            (0x5C, "Super_R"),
            (0x5D, "Menu"),
            // VK_LSHIFT to VK_RMENU.
            (0xA0, "Shift_L"),
            (0xA1, "Shift_R"),
            (0xA2, "Control_L"),
            (0xA3, "Control_R"),
            (0xA4, "Alt_L"),
            (0xA5, "Alt_R"),
            // Both ends of the F-keys, the letters and the digits.
            (0x70, "F1"),
            (0x7C, "F13"),
            (0x87, "F24"),
            (0x41, "a"),
            (0x5A, "z"),
            (0x30, "0"),
            (0x39, "9"),
        ];
        for (code, keysym) in named {
            assert_eq!(trigger_of_key(VirtualKey(code)).as_deref(), Some(keysym), "{code:#x}");
        }
        // No name: an unassigned code, Backspace, the codes just past F24, the menu key and the
        // right Alt, and one past a byte, though its low byte is Caps Lock's.
        for code in [0x07, 0x08, 0x88, 0x5E, 0xA6, 0x0114] {
            assert_eq!(trigger_of_key(VirtualKey(code)), None, "{code:#x}");
        }
    }

    /// Chord text: words a chord uses and fragments around them, joined by `+`, or any text.
    fn chord_text() -> impl Strategy<Value = String> {
        let words = vec![
            "ctrl", "Control", "ALT", "shift", "super", "Win", "logo", "meta", "m", "B", "7", "f1",
            "F24", "f25", "space", "page_up", "Enter", "hyper", "", " ",
        ];
        let part =
            prop_oneof![prop::sample::select(words).prop_map(str::to_owned), "[ a-zA-Z0-9_]{0,6}"];
        let joined = prop::collection::vec(part, 0..6).prop_map(|parts| parts.join("+"));
        prop_oneof![joined, any::<String>()]
    }

    /// Whether `trigger` is canonical: it reads back as itself, names its modifiers in the
    /// specification's order, each at most once, and ends in one keysym name.
    fn canonical(trigger: &str) -> bool {
        let parts: Vec<&str> = trigger.split('+').collect();
        let Some((key, modifiers)) = parts.split_last() else { return false };
        let mut order = MODIFIERS.iter().map(|(name, _)| *name);
        let in_order = modifiers.iter().all(|modifier| order.any(|name| name == *modifier));
        let named = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        trigger_of_chord(trigger).as_deref() == Ok(trigger) && in_order && named
    }

    proptest! {
        #[test]
        fn no_text_makes_the_chord_parser_panic_and_every_trigger_is_canonical(
            chord in chord_text()
        ) {
            if let Ok(trigger) = trigger_of_chord(&chord) {
                prop_assert!(canonical(&trigger), "{:?} gave {:?}", chord, trigger);
            }
        }
    }
}
