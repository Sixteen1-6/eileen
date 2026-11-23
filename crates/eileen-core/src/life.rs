//! The speech process's life (BUILD.md sections 6.3, 6.4 and 6.6; decisions D81 and D83). The
//! core starts `eileen-speech` on its first step and gives it 120 s to send `Ready`. From that
//! `Ready` on it expects a `Heartbeat` (the audio component pings every second) and counts the
//! process hung after 5 s with none. A process that hangs, by either measure, gets
//! `HangDumpThenKill`; one that exits needs no dump. Either way `Restart` follows one backoff
//! step, 1 s doubling to 60 s and never more (the audio ring of section 6.6 holds 180 s, which
//! covers the 120 s load and the longest step), and the steps start again from 1 s once the
//! process has stayed up for a minute after its `Ready`, so a load that hangs and is killed at
//! 120 s never resets them.
//!
//! An `Exited` carries no generation, so the core never sends `Restart` while the process it had
//! killed may still report its exit: after `HangDumpThenKill` the restart waits for that
//! `Exited` as well as the backoff step, and for no more than `EXIT_WAIT_NS` from the kill. An
//! `Exited` while the process is down and its exit was not awaited changes nothing.
//!
//! While speech is down nothing is heard or synthesized: replies go to the HUD, as does the part
//! of an utterance the speech process had not synthesized, and a turn left open is dropped: it
//! is marked handled, so its own late `EndOfTurn` is never routed, since its audio is replayed
//! under a new turn once the process is back.
//!
//! Not here: the core orders no reload that stops recognition yet, so the suspension of the 5 s
//! count from such a reload to its first `Heartbeat` arrives with the machine profile's core
//! part, which orders the first one (section 6.3 as #146 amends it).

use eileen_contracts::bus::BusEvent;
use eileen_contracts::connectors::{SidecarCmd, SidecarEvent, SidecarId};
use eileen_contracts::core_io::Effect;
use eileen_contracts::hud::MicState;
use eileen_contracts::ids::{MonoTime, TraceCtx};

use crate::{Core, Cx, TimerKind};

/// How long a `Start` or `Restart` may go without `Ready`, ns: 120 s, six times section 2.8's
/// slowest model load.
pub(crate) const READY_NS: u64 = 120_000_000_000;

/// How long the process may go without a `Heartbeat` once it is ready, ns: five of the audio
/// component's one-second pings.
pub(crate) const HEARTBEAT_NS: u64 = 5_000_000_000;

/// The first backoff step before a `Restart`, ns.
pub(crate) const BACKOFF_FIRST_NS: u64 = 1_000_000_000;

/// The longest backoff step, ns.
pub(crate) const BACKOFF_MAX_NS: u64 = 60_000_000_000;

/// How long the process must stay up after its `Ready` for the backoff to start again from its
/// first step, ns.
pub(crate) const STABLE_NS: u64 = 60_000_000_000;

/// How long after `HangDumpThenKill` the core waits for the killed process's `Exited` before it
/// restarts anyway, ns (rule 7): the guard writes a hang dump of a process holding its models
/// before the kill, which can take longer than the first backoff step.
pub(crate) const EXIT_WAIT_NS: u64 = 10_000_000_000;

/// Why the process went down, as `SidecarRestarted` says it.
const HUNG: &str = "missed heartbeat";
const NOT_READY: &str = "not ready within 120 s";
const EXITED: &str = "exited";

/// Where the speech process stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Life {
    /// The core has not taken its first step.
    #[default]
    Off,
    /// A `Start`, or a `Restart` when `restarted`, went out at `since`; no `Ready` yet.
    Loading { since: MonoTime, restarted: bool },
    /// Ready since `since`; the last `Heartbeat` (or the `Ready`) came at `beat`.
    Up { since: MonoTime, beat: MonoTime },
    /// Down for `why` since `since`; the speech timer brings the `Restart`. `exit` says what
    /// the restart still waits for after a kill.
    Down { why: &'static str, since: MonoTime, exit: Exit },
}

/// After `HangDumpThenKill`, the killed process's `Exited` and the backoff step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    /// Nothing: the process exited of itself, or the kill's `Exited` came.
    Seen,
    /// The kill's `Exited` has not come, and the backoff step has not passed.
    Awaited,
    /// The backoff step passed: the `Restart` goes out with the `Exited`.
    AwaitedPastStep,
}

/// The speech process as the core sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SpeechLife {
    life: Life,
    /// Times it went down since it last stayed up for a minute, for the backoff.
    failures: u32,
}

impl SpeechLife {
    /// Whether the core has started the process.
    pub(crate) const fn started(&self) -> bool {
        !matches!(self.life, Life::Off)
    }

    /// Whether the process is ready, so Eileen can hear and speak.
    pub(crate) const fn up(&self) -> bool {
        matches!(self.life, Life::Up { .. })
    }
}

/// The backoff step before the restart that follows the `failures`-th failure in a row:
/// 1 s, doubling, to 60 s and never more.
pub(crate) const fn backoff_ns(failures: u32) -> u64 {
    // 2^6 s is past the cap already, so larger shifts are never needed.
    let doublings = if failures > 7 { 6 } else { failures.saturating_sub(1) };
    let step = BACKOFF_FIRST_NS.saturating_mul(1_u64 << doublings);
    if step > BACKOFF_MAX_NS { BACKOFF_MAX_NS } else { step }
}

/// `ns` after `at`.
const fn after(at: MonoTime, ns: u64) -> MonoTime {
    MonoTime(at.0.saturating_add(ns))
}

impl Core {
    /// The core's first step: the speech process starts, with 120 s to be ready.
    pub(crate) fn boot(&mut self, cx: &mut Cx<'_>) {
        self.life.life = Life::Loading { since: cx.now, restarted: false };
        cx.push(TraceCtx::default(), Effect::Sidecar(SidecarId::Speech, SidecarCmd::Start));
        self.timers.set(cx, TimerKind::Speech, after(cx.now, READY_NS));
    }

    /// From a sidecar's client. The minimal core runs only the speech process.
    pub(crate) fn sidecar(&mut self, cx: &mut Cx<'_>, which: SidecarId, event: &SidecarEvent) {
        if which != SidecarId::Speech {
            return;
        }
        match event {
            SidecarEvent::Ready { .. } => self.speech_ready(cx),
            SidecarEvent::Heartbeat => self.speech_beat(cx.now),
            SidecarEvent::Exited { .. } => match self.life.life {
                Life::Loading { .. } | Life::Up { .. } => self.speech_down(cx, EXITED, false),
                // The end of the process the core killed: the restart may go out now.
                Life::Down { why, since, exit: Exit::Awaited } => {
                    self.life.life = Life::Down { why, since, exit: Exit::Seen };
                }
                Life::Down { why, exit: Exit::AwaitedPastStep, .. } => self.speech_restart(cx, why),
                Life::Off | Life::Down { exit: Exit::Seen, .. } => {}
            },
            SidecarEvent::Event(_) => {}
        }
    }

    /// `Ready`: the process listens, and the 5 s count starts. A `Ready` the core is not
    /// waiting for is ignored.
    fn speech_ready(&mut self, cx: &mut Cx<'_>) {
        if !matches!(self.life.life, Life::Loading { .. }) {
            return;
        }
        self.life.life = Life::Up { since: cx.now, beat: cx.now };
        self.timers.set(cx, TimerKind::Speech, after(cx.now, HEARTBEAT_NS));
    }

    /// A `Heartbeat`: the process is alive. None counts before its `Ready`.
    const fn speech_beat(&mut self, now: MonoTime) {
        if let Life::Up { since, .. } = self.life.life {
            self.life.life = Life::Up { since, beat: now };
        }
    }

    /// The speech timer fired: the load deadline, the heartbeat check, or a restart that is
    /// due. The heartbeat check is set once, at `Ready`, and moved on from the latest
    /// `Heartbeat` each time it fires early, so a heartbeat sets no timer of its own.
    pub(crate) fn speech_timer(&mut self, cx: &mut Cx<'_>) {
        match self.life.life {
            Life::Off => {}
            Life::Loading { since, .. } => {
                if cx.now.0.saturating_sub(since.0) >= READY_NS {
                    self.speech_down(cx, NOT_READY, true);
                } else {
                    self.timers.set(cx, TimerKind::Speech, after(since, READY_NS));
                }
            }
            Life::Up { beat, .. } => {
                if cx.now.0.saturating_sub(beat.0) >= HEARTBEAT_NS {
                    self.speech_down(cx, HUNG, true);
                } else {
                    self.timers.set(cx, TimerKind::Speech, after(beat, HEARTBEAT_NS));
                }
            }
            Life::Down { why, since, exit } => {
                let waited = cx.now.0.saturating_sub(since.0) >= EXIT_WAIT_NS;
                if exit == Exit::Seen || waited {
                    self.speech_restart(cx, why);
                } else {
                    // The step passed before the killed process's exit was reported.
                    self.life.life = Life::Down { why, since, exit: Exit::AwaitedPastStep };
                    self.timers.set(cx, TimerKind::Speech, after(since, EXIT_WAIT_NS));
                }
            }
        }
    }

    /// `Restart`, with `SidecarRestarted` on the bus and 120 s to be ready.
    fn speech_restart(&mut self, cx: &mut Cx<'_>, why: &'static str) {
        self.life.life = Life::Loading { since: cx.now, restarted: true };
        cx.push(TraceCtx::default(), Effect::Sidecar(SidecarId::Speech, SidecarCmd::Restart));
        let restarted =
            BusEvent::SidecarRestarted { which: SidecarId::Speech, reason: why.to_owned() };
        cx.emit(TraceCtx::default(), restarted);
        self.timers.set(cx, TimerKind::Speech, after(cx.now, READY_NS));
    }

    /// The process hung (`hung`: it is hang-dumped and killed) or exited. `Restart` waits one
    /// backoff step, and after a kill the killed process's `Exited` too; the open turn is
    /// dropped, and what Eileen had not synthesized goes to the HUD.
    fn speech_down(&mut self, cx: &mut Cx<'_>, why: &'static str, hung: bool) {
        if let Life::Up { since, beat } = self.life.life {
            // A hung process was last known alive at its last heartbeat, an exited one now.
            let alive_until = if hung { beat } else { cx.now };
            if alive_until.0.saturating_sub(since.0) >= STABLE_NS {
                self.life.failures = 0;
            }
        }
        if hung {
            let kill = Effect::Sidecar(SidecarId::Speech, SidecarCmd::HangDumpThenKill);
            cx.push(TraceCtx::default(), kill);
        }
        self.life.failures = self.life.failures.saturating_add(1);
        let exit = if hung { Exit::Awaited } else { Exit::Seen };
        self.life.life = Life::Down { why, since: cx.now, exit };
        self.timers.set(cx, TimerKind::Speech, after(cx.now, backoff_ns(self.life.failures)));
        if let Some(open) = self.turns.end_of_turn() {
            // Its audio is recognised afresh under a new turn: its own late events are dropped.
            self.handled.insert(open);
        }
        self.partial = None;
        if let Some(speak) = self.turns.speaking() {
            // The utterance can take no more clauses, so the rest of a reply goes to the HUD.
            self.brain_reply_on_hud(speak);
        }
        let mut out = Vec::new();
        self.turns.speech_lost(&mut out);
        self.apply(cx, out);
    }

    /// Whether Eileen can speak aloud: not muted, and the speech process is ready.
    pub(crate) const fn can_speak(&self) -> bool {
        !self.turns.muted() && self.life.up()
    }

    /// The microphone as the HUD shows it.
    pub(crate) const fn mic_state(&self) -> MicState {
        if self.turns.muted() {
            return MicState::Muted;
        }
        match self.life.life {
            Life::Up { .. } => MicState::Listening,
            Life::Off | Life::Loading { restarted: false, .. } => MicState::LoadingSpeech,
            Life::Loading { restarted: true, .. } | Life::Down { .. } => MicState::SpeechRestarting,
        }
    }
}

#[cfg(test)]
mod tests;
