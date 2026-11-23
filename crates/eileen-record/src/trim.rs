//! `eileen record trim` (BUILD.md sections 3.3.16 and 3.11): cuts a recording's input lane down
//! to the turns and jobs a scenario is made from. Scenario mode starts a fresh core from the
//! scenario's configuration and fixtures, never an old snapshot, so trim keeps, besides every
//! input the recorder attributed to a chosen turn or job, the setup inputs a fresh core needs
//! to reach the same starting state: the latest one of each kind before the first kept input
//! ([`setup_kind`]). It keeps the setup inputs no other turn or job claims between the first
//! and the last kept input, which change that state while the turn and its jobs run. And it
//! keeps each kept hook input's answer, wherever the answer sits, because scenario mode
//! compares the live core's answer with it and strict mode compares it like a bus event.

use std::collections::BTreeSet;

use eileen_contracts::config::SettingsEvent;
use eileen_contracts::core_io::{ActivityEvent, Input, UserInput};
use eileen_contracts::ids::{JobId, TraceCtx, TurnId};
use eileen_contracts::record::RecordLine;
use eileen_contracts::speech::VoiceEvent;

/// The turns and jobs a scenario is cut to, as a recording's or an incident's manifest lists
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// The chosen turns.
    pub turns: BTreeSet<TurnId>,
    /// The chosen jobs, usually those the turns started.
    pub jobs: BTreeSet<JobId>,
}

impl Selection {
    /// Whether an input with this correlation belongs to a chosen turn or job.
    #[must_use]
    pub fn holds(&self, ctx: &TraceCtx) -> bool {
        ctx.turn.is_some_and(|turn| self.turns.contains(&turn))
            || ctx.job.is_some_and(|job| self.jobs.contains(&job))
    }

    /// Whether an input with this correlation belongs to a turn or job that was not chosen.
    #[must_use]
    pub fn foreign(&self, ctx: &TraceCtx) -> bool {
        (ctx.turn.is_some() || ctx.job.is_some()) && !self.holds(ctx)
    }
}

/// The standing state a setup input sets. Trim keeps the latest setup input of each kind, and
/// for [`SetupKind::Mute`] the latest `SetMute` with every `ToggleMute` after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SetupKind {
    /// `SetMute` and `ToggleMute`.
    Mute,
    /// The memory reading.
    Memory,
    /// Other audio playing.
    OtherAudio,
    /// Another program using the microphone.
    MicInUseElsewhere,
    /// The session locked.
    SessionLocked,
    /// A presentation running.
    Presentation,
    /// A changed configuration file.
    Settings,
    /// The voice calibration.
    Calibration,
}

/// The standing state `input` sets that the next turn is judged by and a fresh core started
/// from the scenario's configuration and fixtures would not have, or `None` for a request, a
/// reaction to an effect, or state scenario mode synthesizes from the fixtures and the live
/// core's effects (`Store` `Loaded`, sidecars' `Ready`, timers and playback). Every variant is
/// named, so a new input does not compile here until someone decides whether it is setup.
#[must_use]
pub const fn setup_kind(input: &Input) -> Option<SetupKind> {
    match input {
        Input::User(user) => match user {
            UserInput::SetMute(_) | UserInput::ToggleMute => Some(SetupKind::Mute),
            UserInput::Approve(_)
            | UserInput::Deny(_)
            | UserInput::CancelJob(_)
            | UserInput::PauseDesktopJobs
            | UserInput::StartAnyway(_)
            | UserInput::Typed { .. }
            | UserInput::Cli { .. }
            | UserInput::Hud { .. }
            | UserInput::DictationKey(_)
            | UserInput::SaveIncident
            | UserInput::SaveScenario => None,
        },
        Input::Activity(activity) => match activity {
            ActivityEvent::OtherAudio { .. } => Some(SetupKind::OtherAudio),
            ActivityEvent::MicInUseElsewhere(_) => Some(SetupKind::MicInUseElsewhere),
            ActivityEvent::SessionLocked(_) => Some(SetupKind::SessionLocked),
            ActivityEvent::Presentation(_) => Some(SetupKind::Presentation),
            ActivityEvent::PhysicalInput => None,
        },
        Input::Settings(settings) => match settings {
            SettingsEvent::FileChanged { .. } => Some(SetupKind::Settings),
            SettingsEvent::ChecksDone { .. } => None,
        },
        Input::Voice(voice) => match voice {
            VoiceEvent::Calibration(_) => Some(SetupKind::Calibration),
            VoiceEvent::SpeechStarted { .. }
            | VoiceEvent::Partial { .. }
            | VoiceEvent::SpeechEnded { .. }
            | VoiceEvent::EndOfTurn { .. }
            | VoiceEvent::BargeInWindow { .. }
            | VoiceEvent::ClauseReady { .. }
            | VoiceEvent::SpeakDone { .. }
            | VoiceEvent::Embedding { .. }
            | VoiceEvent::CheckResult { .. }
            | VoiceEvent::SpeakerMatch { .. }
            | VoiceEvent::Health { .. } => None,
        },
        Input::Memory(_) => Some(SetupKind::Memory),
        Input::Playback(_)
        | Input::Brain(_)
        | Input::Worker(..)
        | Input::Hook(..)
        | Input::ToolCall(_)
        | Input::ToolResult(..)
        | Input::Sidecar(..)
        | Input::Learn(_)
        | Input::Store(_)
        | Input::Timer(_) => None,
    }
}

/// Whether `input` is a setup input ([`setup_kind`]).
#[must_use]
pub const fn is_setup(input: &Input) -> bool {
    setup_kind(input).is_some()
}

/// The lines of `lines` a scenario of `selection` needs, in their order: every input of a
/// chosen turn or job; before the first of them, the latest setup input of each kind (for mute,
/// the latest `SetMute` and every `ToggleMute` after it); between the first and the last, every
/// setup input no other turn or job claims; and the answer to each kept hook input. Nothing
/// when no input belongs to the selection.
#[must_use]
pub fn trim(lines: &[RecordLine], selection: &Selection) -> Vec<RecordLine> {
    let chosen = |line: &RecordLine| match line {
        RecordLine::Input(record) => selection.holds(&record.ctx),
        RecordLine::HookAnswer(_) => false,
    };
    let (Some(first), Some(last)) = (lines.iter().position(chosen), lines.iter().rposition(chosen))
    else {
        return Vec::new();
    };
    let mut setup = BTreeSet::new();
    let mut settled = BTreeSet::new();
    for (at, line) in lines.iter().enumerate().take(first).rev() {
        let RecordLine::Input(record) = line else { continue };
        let Some(kind) = setup_kind(&record.input) else { continue };
        if settled.contains(&kind) {
            continue;
        }
        setup.insert(at);
        if !matches!(record.input, Input::User(UserInput::ToggleMute)) {
            settled.insert(kind);
        }
    }
    let hooks: BTreeSet<u64> = lines
        .iter()
        .filter_map(|line| match line {
            RecordLine::Input(record)
                if matches!(record.input, Input::Hook(..)) && selection.holds(&record.ctx) =>
            {
                Some(record.seq)
            }
            RecordLine::Input(_) | RecordLine::HookAnswer(_) => None,
        })
        .collect();
    lines
        .iter()
        .enumerate()
        .filter(|(at, line)| match line {
            RecordLine::Input(record) => {
                selection.holds(&record.ctx)
                    || setup.contains(at)
                    || (first < *at
                        && *at < last
                        && is_setup(&record.input)
                        && !selection.foreign(&record.ctx))
            }
            RecordLine::HookAnswer(answer) => hooks.contains(&answer.input),
        })
        .map(|(_, line)| line.clone())
        .collect()
}
