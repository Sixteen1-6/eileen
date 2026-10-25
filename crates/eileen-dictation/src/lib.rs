//! Eileen's tier-0 dictation cleanup (BUILD.md section 2.8), always on and pure: vocabulary
//! replacement, scratch-that, fillers per language, stutters, typed-slot self-corrections ("at
//! 3, no, 4 pm") and spoken punctuation. The core calls it through the `DictationCleaner` port.
//! A stub until WP4 builds it.
//!
//! # Owns
//!
//! - [`Cleaner`], the implementation of `eileen_contracts::ports::DictationCleaner`.
//! - The 79 conformance cases of `SpeakoFlow` (MIT, attributed) and Eileen's typed-slot
//!   positives and negatives.
//! - The `dictation_cleanup` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - Recognition (`eileen-speech`), the dictation key and flow (`eileen-core`), the vocabulary
//!   store (`eileen-learn`), or inserting the text (`eileen-desktop`).
//!
//! # Invariants
//!
//! - Pure: no IO, no clock, no native library (check 24).
//! - Text that is not a correction is left as it was ("I actually enjoyed the movie"; WP4
//!   acceptance 8 tests Eileen's negatives).
//! - The stub returns the text unchanged with nothing counted (unit test
//!   `tests::the_stub_cleaner_returns_the_text_unchanged`).
//!
//! # Boundaries
//!
//! - Called by `eileen-core` through the port, and by `eileen-sim`.
//! - May depend on `eileen-contracts` and `eileen-stdx`.
//! - Must not depend on `eileen-core` (it calls the port), `eileen-speech`, `eileen-trace` (the
//!   crate logs nothing) or rmcp, nor know vendor CLI flags (section 3.1); nor on a native
//!   library or anything that can hang (check 24), or a `-win` or `-linux` crate (check 15).
//! - Wire types: none of its own; `Cleaned` is a contract type.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-dictation` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-dictation`).
//! - Layers: the conformance cases as table tests, property tests, and the `dictation_cleanup`
//!   fuzz target.
//! - A bug here starts as a sentence the cleaner changed that the user meant as said.
//!
//! # Traps
//!
//! - The text is the user's words: it stays `Personal` and is never logged as plain text.
//!
//! # Logs and tracing
//!
//! - None: the crate is pure.
//!
//! # Do not change without a decision record
//!
//! - The port's signature (`eileen-contracts::ports`, section 3.3.15).

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod fuzz_entry;

use eileen_contracts::desktop::FieldCategory;
use eileen_contracts::ids::Personal;
use eileen_contracts::learn::VocabularyEntry;
use eileen_contracts::ports::{Cleaned, DictationCleaner};

/// The tier-0 cleaner. The stub returns the text unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cleaner;

impl DictationCleaner for Cleaner {
    fn clean(
        &self,
        raw: &str,
        _category: FieldCategory,
        _vocabulary: &[VocabularyEntry],
    ) -> Cleaned {
        Cleaned { text: Personal::new(raw.to_owned()), fillers_removed: 0, corrections: 0 }
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::desktop::FieldCategory;
    use eileen_contracts::ports::DictationCleaner;

    use super::Cleaner;

    #[test]
    fn the_stub_cleaner_returns_the_text_unchanged() {
        let cleaned = Cleaner.clean("um, send it at 3, no, 4 pm", FieldCategory::Chat, &[]);
        assert_eq!(cleaned.text.get(), "um, send it at 3, no, 4 pm", "unchanged");
        assert_eq!((cleaned.fillers_removed, cleaned.corrections), (0, 0), "nothing counted");
    }
}
