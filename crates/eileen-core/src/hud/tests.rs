//! The HUD's line: its limit and its owner.

use eileen_contracts::ids::{JobId, TurnId};

use super::{CUT, Hud, LINE_MAX_BYTES};
use crate::speech::Owner;

fn reply() -> Owner {
    Owner::turn(TurnId::from_parts(1, 1))
}

#[test]
fn a_line_is_cut_at_its_limit_on_a_character_boundary_and_takes_no_more() {
    let mut hud = Hud::default();
    hud.show(reply(), "Rivers carry water");
    // Two-byte characters from an odd offset, so the limit falls inside one.
    let clause = "\u{e9}".repeat(1_001);
    for _ in 0..4 {
        hud.append(reply(), &clause);
    }
    let line = hud.line().unwrap_or_default().to_owned();
    assert!(line.len() <= LINE_MAX_BYTES, "{} bytes", line.len());
    assert!(line.starts_with("Rivers carry water \u{e9}"), "the start is kept");
    assert!(line.ends_with(CUT), "a cut line says so");
    hud.append(reply(), "And they feed lakes.");
    assert_eq!(hud.line(), Some(line.as_str()), "a full line takes no more");
    hud.show(reply(), "A new line.");
    assert_eq!(hud.line(), Some("A new line."), "a new line starts empty again");
}

#[test]
fn text_is_added_only_to_a_line_of_the_same_owner() {
    let mut hud = Hud::default();
    hud.show(reply(), "Rivers carry water to the sea.");
    hud.append(reply(), "They shape valleys.");
    assert_eq!(hud.line(), Some("Rivers carry water to the sea. They shape valleys."));
    let announcement = Owner::job(JobId::from_parts(2, 2));
    hud.show(announcement, "The build finished.");
    hud.append(reply(), "And they feed lakes.");
    assert_eq!(
        hud.line(),
        Some("And they feed lakes."),
        "a reply's text never follows another message"
    );
}
