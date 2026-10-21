//! The owner's own Claude sessions' toolset (`session`), through the plugin.
//!
//! Provisional: WP2 pins this module in its first pull request (BUILD.md section 3.3).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{NoArgs, ToolInput, input};
use crate::schema::Registry;

const MODULE: &str = "tools/session";

/// `session_handoff`: creates a pending handoff the user confirms by voice or on the HUD
/// (naming the repository and the summary); never starts a job alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionHandoff {
    /// The session to continue.
    pub session_id: String,
    /// Its working folder.
    #[schemars(extend("x-eileen-path" = true))]
    pub cwd: String,
    /// What it did.
    pub summary: String,
    /// What comes next.
    pub next: String,
}

/// `session_say`: at most 200 characters, at most one per 30 s, spoken as "From your Claude
/// session: ...".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionSay {
    /// The line.
    pub text: String,
}

/// `session_mute`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionMute {
    /// Mute or unmute.
    pub muted: bool,
}

/// `session_ask`: at most 200 characters, one per 30 s; shown on the HUD and spoken as "Your
/// Claude session asks: ..." (HUD only in a call). Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionAsk {
    /// The question.
    pub question: String,
}

/// What `session_ask` returns within 120 s: the user's next addressed reply, typed answer or
/// panel text, which starts nothing and approves nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionAskOutput {
    /// The answer, if the user gave one.
    pub answer: Option<String>,
}

/// `session_skills`: only with `claude_plugin.share_skills` on; learned skills at level 2 or
/// above whose every line is Eileen-authored, user-provenance or approved; never a pending
/// draft. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SessionSkills {
    /// What to look for.
    pub query: String,
    /// At most this many.
    pub limit: Option<u8>,
}

pub(super) fn inputs(out: &mut BTreeMap<&'static str, ToolInput>) {
    input::<NoArgs>(out, MODULE, "session_status");
    input::<NoArgs>(out, MODULE, "session_jobs");
    input::<SessionSay>(out, MODULE, "session_say");
    input::<SessionMute>(out, MODULE, "session_mute");
    input::<SessionHandoff>(out, MODULE, "session_handoff");
    input::<SessionAsk>(out, MODULE, "session_ask");
    input::<SessionSkills>(out, MODULE, "session_skills");
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<SessionHandoff>();
    r.add::<SessionSay>();
    r.add::<SessionMute>();
    r.add::<SessionAsk>();
    r.add::<SessionAskOutput>();
    r.add::<SessionSkills>();
}
