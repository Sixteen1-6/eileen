//! The view `eileen run --console` prints (BUILD.md sections 3.5 and 6.4): what is heard, what
//! Eileen says and the jobs, one line each, read from the bus.
//!
//! - **Lines.** `Heard`, `Speaking` and every `Job*` event give one line; a lag notice gives
//!   one too, so a console that fell behind says so; every other event gives none. A job is
//!   shown by the name its `JobAccepted` gave it, and by its id when the view never saw that
//!   event.
//! - **Untrusted text.** A transcript, a job's narration and summary, and the names and
//!   reasons beside them can hold anything, a model's output and screen text included, and a
//!   terminal obeys what it is sent. So control characters and the characters that reorder or
//!   break a line are written as escapes, and a text is cut at [`MAX_TEXT`] characters as
//!   shown: an event is always one line and never moves the cursor, clears the screen or sets
//!   a title. Joiners and other invisible characters that scripts and emoji need are left as
//!   they are, and a backslash is not doubled, so the view is for reading, not for parsing.
//! - **Bounds.** The view keeps the names of at most [`MAX_NAMES`] unfinished jobs, the oldest
//!   going first, and forgets a job at its terminal event.
//! - **Where it runs.** [`Console::show`] writes and flushes before it returns, and a
//!   terminal can stop taking output for as long as it likes (a selection in a Windows console
//!   does). So the composition root gives the view its own subscriber and runs
//!   [`Console::follow`] on a thread of its own, never on a runtime worker: a stalled terminal
//!   then holds that thread only, the bus goes on, and the view says how much it missed.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};

use eileen_contracts::bus::{BusEvent, CancelSource, QueueReason};
use eileen_contracts::ids::{Envelope, JobId};
use eileen_contracts::jobs::{JobKind, Route};

use crate::bus::Subscriber;

/// The most characters one text takes on a line, escapes counted as they are shown; the rest
/// is cut and marked `...`.
pub const MAX_TEXT: usize = 200;

/// The most unfinished jobs whose names the view keeps.
pub const MAX_NAMES: usize = 256;

/// Why the view stopped.
#[derive(Debug, thiserror::Error)]
pub enum ConsoleError {
    /// The output refused a line.
    #[error("the console could not be written")]
    Write(#[source] io::Error),
}

/// The console view over an output.
pub struct Console<W> {
    out: W,
    /// Each unfinished job's name, already made safe to print.
    names: BTreeMap<JobId, String>,
}

impl<W> fmt::Debug for Console<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Console").field("jobs", &self.names.len()).finish_non_exhaustive()
    }
}

impl<W: Write> Console<W> {
    /// A view that writes to `out`.
    #[must_use]
    pub const fn new(out: W) -> Self {
        Self { out, names: BTreeMap::new() }
    }

    /// Writes the line `event` gives, if it gives one, and flushes it.
    pub fn show(&mut self, event: &Envelope<BusEvent>) -> Result<(), ConsoleError> {
        let Some(line) = self.line(&event.body) else { return Ok(()) };
        writeln!(self.out, "{line}").and_then(|()| self.out.flush()).map_err(ConsoleError::Write)
    }

    /// Shows every event `events` gives until the bus stops. Each line is a blocking write, so
    /// this runs on a thread of its own (the module doc says why).
    pub async fn follow(&mut self, events: &mut Subscriber) -> Result<(), ConsoleError> {
        while let Some(event) = events.recv().await {
            self.show(&event)?;
        }
        Ok(())
    }

    /// The output, with everything written to it.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.out
    }

    /// The line for one event, without its line end. The match names every event, so a new one
    /// fails to build here until someone decides whether the console shows it.
    fn line(&mut self, event: &BusEvent) -> Option<String> {
        let (label, text) = match event {
            BusEvent::Heard { text } => ("heard", plain(text.get())),
            BusEvent::Speaking { text, .. } => ("says", plain(text)),
            BusEvent::JobAccepted { job, name, kind, route, .. } => {
                let name = self.remember(*job, name);
                ("job", format!("{name} accepted: {} on {}", kind_words(*kind), route_name(*route)))
            }
            BusEvent::JobQueued { job, reason } => {
                ("job", format!("{} queued: waiting for {}", self.name(*job), awaited(*reason)))
            }
            BusEvent::JobStarted { job, warm } => {
                let session = if *warm { " in a warm session" } else { "" };
                ("job", format!("{} started{session}", self.name(*job)))
            }
            BusEvent::JobNarration { job, line } => {
                ("job", format!("{}: {}", self.name(*job), plain(line.get())))
            }
            BusEvent::JobProgressed { job, line, pct } => {
                let name = self.name(*job);
                let line = plain(line.get());
                let text = match pct {
                    Some(pct) => format!("{name} {pct}%: {line}"),
                    None => format!("{name}: {line}"),
                };
                ("job", text)
            }
            BusEvent::JobBlocked { job, reason } => {
                ("job", format!("{} blocked: {}", self.name(*job), plain(reason)))
            }
            BusEvent::JobCompleted { job, summary, .. } => {
                ("job", format!("{} completed: {}", self.forget(*job), plain(summary.get())))
            }
            BusEvent::JobFailed { job, error } => {
                ("job", format!("{} failed: {}", self.forget(*job), plain(error)))
            }
            BusEvent::JobCancelled { job, by } => {
                ("job", format!("{} cancelled {}", self.forget(*job), canceller(*by)))
            }
            BusEvent::JobInterrupted { job } => {
                ("job", format!("{} interrupted", self.forget(*job)))
            }
            BusEvent::Lagged { missed } => {
                ("lagged", format!("events the console missed: {missed}"))
            }
            BusEvent::TurnStarted { .. }
            | BusEvent::TurnRouted { .. }
            | BusEvent::SpeakingStopped { .. }
            | BusEvent::MuteChanged { .. }
            | BusEvent::ConversationWindow { .. }
            | BusEvent::BrainReady { .. }
            | BusEvent::ApprovalRequested { .. }
            | BusEvent::ApprovalResolved { .. }
            | BusEvent::ApprovalExpired { .. }
            | BusEvent::DesktopIntent { .. }
            | BusEvent::InputLock { .. }
            | BusEvent::ForegroundLease { .. }
            | BusEvent::UserTookOver { .. }
            | BusEvent::DesktopJobsPaused { .. }
            | BusEvent::PlanUsage(_)
            | BusEvent::Learn(_)
            | BusEvent::UserError(_)
            | BusEvent::SettingChanged { .. }
            | BusEvent::FirstRun(_)
            | BusEvent::RecordingChanged { .. }
            | BusEvent::IncidentCaptured { .. }
            | BusEvent::SidecarRestarted { .. } => return None,
        };
        Some(format!("{label:<7}{text}"))
    }

    /// Keeps an accepted job's name, dropping the oldest job's when the view holds too many,
    /// and gives the name as the lines show it.
    fn remember(&mut self, job: JobId, name: &str) -> String {
        let name = plain(name);
        if self.names.len() >= MAX_NAMES && !self.names.contains_key(&job) {
            // Job ids sort by the time they were minted, so the first is the oldest.
            let _oldest = self.names.pop_first();
        }
        self.names.insert(job, name.clone());
        name
    }

    /// A job as the lines show it: its name, or its id when the view never saw it accepted.
    fn name(&self, job: JobId) -> String {
        self.names.get(&job).cloned().unwrap_or_else(|| job.to_string())
    }

    /// [`Self::name`] for a job's last line, after which its name is no longer kept.
    fn forget(&mut self, job: JobId) -> String {
        self.names.remove(&job).unwrap_or_else(|| job.to_string())
    }
}

/// `text` made safe to print on one line: control characters and the characters that reorder
/// or break a line as escapes, cut before it would take more than [`MAX_TEXT`] characters.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_TEXT));
    let mut shown = 0_usize;
    for c in text.chars() {
        let start = out.len();
        let width = if c.is_control() {
            out.extend(c.escape_default());
            out.len() - start
        } else if reorders(c) {
            out.extend(c.escape_unicode());
            out.len() - start
        } else {
            out.push(c);
            1
        };
        // An escape is ASCII, so the bytes it added are the characters it shows.
        shown = shown.saturating_add(width);
        if shown > MAX_TEXT {
            out.truncate(start);
            out.push_str("...");
            break;
        }
    }
    out
}

/// Whether `c` is no control character and still changes where text lands: the bidirectional
/// marks, embeddings, overrides and isolates and the deprecated shaping controls, which can
/// show a line in another order than it was written, and the line and paragraph separators.
const fn reorders(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2066}'..='\u{206F}'
    )
}

const fn kind_words(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Coding => "coding",
        JobKind::ComputerUse => "computer use",
        JobKind::General => "a general task",
        JobKind::Research => "research",
        JobKind::DeepAnswer => "a deep answer",
        JobKind::Librarian => "the librarian",
        JobKind::Replay => "a replay",
        JobKind::ContinueSession => "a continued session",
        JobKind::AppTask => "an app task",
        JobKind::Act => "an action",
        JobKind::MlProject => "an ML project",
    }
}

const fn route_name(route: Route) -> &'static str {
    match route {
        Route::Claude => "Claude",
        Route::Codex => "Codex",
        Route::Grok => "Grok",
        Route::DeepSeek => "DeepSeek",
        Route::Local => "a local model",
    }
}

const fn awaited(reason: QueueReason) -> &'static str {
    match reason {
        QueueReason::Memory => "memory",
        QueueReason::BuildSlot => "a build slot",
        QueueReason::Plan => "the plan's limits",
    }
}

const fn canceller(by: CancelSource) -> &'static str {
    match by {
        CancelSource::Voice => "by voice",
        CancelSource::Hud => "on the HUD",
        CancelSource::Hotkey => "by hotkey",
        CancelSource::Tray => "from the tray",
        CancelSource::Cli => "from the command line",
        CancelSource::Parent => "with its parent",
        CancelSource::Shutdown => "at shutdown",
    }
}

#[cfg(test)]
mod tests;
