//! The HUD state the minimal core assembles (BUILD.md sections 3.3.6 and 6.3): the microphone,
//! and the line Eileen shows when she cannot speak, because speech is down, the microphone is
//! muted, or the speech process made no clause of what she was saying. The full core fills the
//! rest of `HudState`. The state goes out only when it changed.

use eileen_contracts::config::HudConfig;
use eileen_contracts::core_io::Effect;
use eileen_contracts::hud::{HudLayout, HudState, Indicators};
use eileen_contracts::ids::TraceCtx;

use crate::{Core, Cx};

/// The HUD state's own version (section 3.3.6).
const STATE_VERSION: u16 = 1;

/// What the core keeps for the HUD.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Hud {
    /// What Eileen shows instead of saying.
    line: Option<String>,
    /// The state last sent, so an unchanged one is not sent again.
    sent: Option<HudState>,
}

impl Hud {
    /// Shows `text` as a new line; empty text changes nothing.
    pub(crate) fn show(&mut self, text: &str) {
        let text = text.trim();
        if !text.is_empty() {
            self.line = Some(text.to_owned());
        }
    }

    /// Adds `text` to the line.
    pub(crate) fn append(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if let Some(line) = self.line.as_mut() {
            line.push(' ');
            line.push_str(text);
        } else {
            self.line = Some(text.to_owned());
        }
    }

    /// Clears the line: Eileen speaks aloud again.
    pub(crate) fn clear(&mut self) {
        self.line = None;
    }
}

/// Where and how the HUD is drawn, from settings.
const fn layout(config: &HudConfig) -> HudLayout {
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
            layout: layout(&self.config.config.hud),
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
