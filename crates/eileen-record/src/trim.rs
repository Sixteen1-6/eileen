//! `eileen record trim` (BUILD.md section 3.3.16): cuts a recording down to the inputs a
//! scenario needs. Scenario mode starts a fresh core from the scenario's configuration and
//! fixtures, never an old snapshot, so trim keeps, besides every line of the chosen window, the
//! setup inputs before it: the inputs that leave the core in a standing state the fixtures do
//! not give it ([`is_setup`]). And it keeps each kept hook input's answer, wherever the answer
//! sits, because scenario mode compares the live core's answer with it and strict mode compares
//! it like a bus event.

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

use eileen_contracts::config::SettingsEvent;
use eileen_contracts::core_io::{ActivityEvent, Input, UserInput};
use eileen_contracts::record::RecordLine;
use eileen_contracts::speech::VoiceEvent;

/// Whether an input sets standing state the next turn is judged by, which a fresh core started
/// from the scenario's configuration and fixtures would not have: mute, the memory reading, the
/// user's and the PC's activity (other audio, the microphone taken, the session locked, a
/// presentation), a changed configuration file and the voice calibration. Everything else is a
/// request, a reaction to an effect, or state scenario mode synthesizes from the fixtures and
/// the live core's effects (`Store` `Loaded`, sidecars' `Ready`, timers and playback).
#[must_use]
pub const fn is_setup(input: &Input) -> bool {
    matches!(
        input,
        Input::User(UserInput::SetMute(_) | UserInput::ToggleMute)
            | Input::Memory(_)
            | Input::Activity(
                ActivityEvent::OtherAudio { .. }
                    | ActivityEvent::MicInUseElsewhere(_)
                    | ActivityEvent::SessionLocked(_)
                    | ActivityEvent::Presentation(_)
            )
            | Input::Settings(SettingsEvent::FileChanged { .. })
            | Input::Voice(VoiceEvent::Calibration(_))
    )
}

/// The lines of `lines` a scenario of the inputs numbered `window` needs, in their order: every
/// input in the window, every setup input before it, and the answer to each kept hook input.
/// Inputs after the window, answers to hooks that were not kept, and every other input before
/// the window are dropped.
#[must_use]
pub fn trim(lines: &[RecordLine], window: &RangeInclusive<u64>) -> Vec<RecordLine> {
    let kept_input = |line: &RecordLine| match line {
        RecordLine::Input(record) => {
            window.contains(&record.seq)
                || (record.seq < *window.start() && is_setup(&record.input))
        }
        RecordLine::HookAnswer(_) => false,
    };
    let hooks: BTreeSet<u64> = lines
        .iter()
        .filter(|line| kept_input(line))
        .filter_map(|line| match line {
            RecordLine::Input(record) if matches!(record.input, Input::Hook(..)) => {
                Some(record.seq)
            }
            RecordLine::Input(_) | RecordLine::HookAnswer(_) => None,
        })
        .collect();
    lines
        .iter()
        .filter(|line| match line {
            RecordLine::Input(_) => kept_input(line),
            RecordLine::HookAnswer(answer) => hooks.contains(&answer.input),
        })
        .cloned()
        .collect()
}
