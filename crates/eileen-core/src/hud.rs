//! The HUD state the minimal core assembles (BUILD.md sections 3.3.6 and 6.3): the microphone,
//! and the line Eileen shows when she cannot speak, because speech is down, the microphone is
//! muted, or the speech process made no clause of what she was saying. The full core fills the
//! rest of `HudState`. The state goes out only when it changed.

use eileen_contracts::config::HudConfig;
use eileen_contracts::core_io::Effect;
use eileen_contracts::hud::{HudLayout, HudState, Indicators};
use eileen_contracts::ids::TraceCtx;

use crate::speech::Owner;
use crate::{Core, Cx};

/// The HUD state's own version (section 3.3.6).
const STATE_VERSION: u16 = 1;

/// The longest line the HUD shows, in bytes. Model text has no bound, the whole state must fit
/// one line of the HUD pipe (`eileen_contracts::hud::MAX_LINE_BYTES`, 64 KiB), and the state is
/// sent again whenever the line grows, so the line stays far below that.
pub(crate) const LINE_MAX_BYTES: usize = 4 * 1024;

/// What ends a line cut at `LINE_MAX_BYTES`.
const CUT: &str = " ...";

/// What the core keeps for the HUD.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Hud {
    /// What Eileen shows instead of saying.
    line: Option<String>,
    /// What the line belongs to: text is added only to a line of the same owner.
    owner: Owner,
    /// The line was cut at its limit and takes no more.
    full: bool,
    /// The state last sent, so an unchanged one is not sent again.
    sent: Option<HudState>,
}

impl Hud {
    /// Shows `text` as a new line belonging to `owner`; empty text changes nothing.
    pub(crate) fn show(&mut self, owner: Owner, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.line = None;
        self.owner = owner;
        self.full = false;
        self.push(text);
    }

    /// Adds `text` to the line when it belongs to `owner`, and shows it as a new line
    /// otherwise.
    pub(crate) fn append(&mut self, owner: Owner, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self.line.is_none() || self.owner != owner {
            self.show(owner, text);
        } else {
            self.push(text);
        }
    }

    /// Puts `text` in front of the line when it belongs to `owner`, and shows it as a new line
    /// otherwise.
    pub(crate) fn lead(&mut self, owner: Owner, text: &str) {
        match self.line.take() {
            Some(old) if self.owner == owner => {
                self.show(owner, text);
                self.push(&old);
            }
            old => {
                self.line = old;
                self.show(owner, text);
            }
        }
    }

    /// Adds `text` after a space, cutting the line at its limit on a character boundary.
    fn push(&mut self, text: &str) {
        if self.full {
            return;
        }
        let line = self.line.get_or_insert_with(String::new);
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(text);
        if line.len() > LINE_MAX_BYTES {
            let mut end = LINE_MAX_BYTES.saturating_sub(CUT.len());
            while !line.is_char_boundary(end) {
                end = end.saturating_sub(1);
            }
            line.truncate(end);
            let kept = line.trim_end().len();
            line.truncate(kept);
            line.push_str(CUT);
            self.full = true;
        }
    }

    /// Clears the line: Eileen speaks aloud again.
    pub(crate) fn clear(&mut self) {
        self.line = None;
        self.full = false;
    }

    /// The line, for tests.
    #[cfg(test)]
    pub(crate) fn line(&self) -> Option<&str> {
        self.line.as_deref()
    }
}

/// Where and how the HUD is drawn, from settings.
const fn layout(config: HudConfig) -> HudLayout {
    HudLayout {
        corner: config.corner,
        scale_pct: config.scale_pct,
        opacity_pct: config.opacity_pct,
        show_transcript: config.show_transcript,
        high_contrast: false,
    }
}

impl Core {
    /// After every step: sends the HUD state when it changed.
    pub(crate) fn refresh_hud(&mut self, cx: &mut Cx<'_>) {
        let state = HudState {
            v: STATE_VERSION,
            layout: layout(self.config.config.hud),
            mic: self.mic_state(),
            conversation: None,
            heard: None,
            speaking: self.hud.line.clone(),
            jobs: Vec::new(),
            approvals: Vec::new(),
            intent: None,
            input_lock: None,
            lease: None,
            watch: None,
            panel: None,
            suggestions: Vec::new(),
            first_run: None,
            indicators: Indicators::default(),
        };
        if self.hud.sent.as_ref() == Some(&state) {
            return;
        }
        cx.push(TraceCtx::default(), Effect::Hud(Box::new(state.clone())));
        self.hud.sent = Some(state);
    }
}

#[cfg(test)]
mod tests;
