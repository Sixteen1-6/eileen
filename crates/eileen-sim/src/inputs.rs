//! The input generator: every input of a run is drawn from an [`arbitrary::Unstructured`]
//! buffer, so the same code serves a seeded run (the seed fills the buffer through
//! [`CoreRng`]), the `sim` fuzz target (the fuzzer's bytes are the buffer) and later a
//! minimiser (a shorter buffer is a smaller run).
//!
//! The skeleton draws a representative subset of [`Input`]: the speech process's turn events
//! (speech start, partial, speech end, end of turn) over one turn at a time, the user's mute,
//! unmute and toggle, typed and command-line text, the dictation key, playback accounting
//! (an utterance drained, a device change, a stall) and timers. WP1 adds the rest with the
//! world's parts that answer the core (the brain, workers, sidecars, memory).

use arbitrary::Unstructured;
use eileen_contracts::core_io::{Input, KeyEdge, PlaybackEvent, TimerId, UserInput};
use eileen_contracts::ids::{Personal, SpeakId, TurnId};
use eileen_contracts::speech::{TurnEndReason, VoiceEvent};
use eileen_stdx::rng::CoreRng;

use crate::profile::InputMix;

/// Short utterances the generator speaks and types; they are synthetic, never anyone's words.
const PHRASES: &[&str] = &[
    "what time is it",
    "open the editor",
    "stop",
    "go on",
    "move that window left",
    "mute",
    "never mind",
    "play the next one",
];

/// The kinds of input the generator draws, for the step log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// A speech process turn event.
    Voice,
    /// A user input (mute, text, the dictation key).
    User,
    /// Playback accounting.
    Playback,
    /// A timer firing.
    Timer,
}

/// What the generator keeps between inputs, so turns and utterances stay coherent: an end of
/// turn names the turn its speech start minted.
#[derive(Debug, Default)]
pub struct Generator {
    turn: Option<TurnId>,
    spoke: bool,
    next_timer: u64,
}

impl Generator {
    /// The turn the next voice event belongs to, if one is open.
    #[must_use]
    pub const fn open_turn(&self) -> Option<TurnId> {
        self.turn
    }

    /// Draws the next input. `now_ms` stamps any ID it mints; `rng` supplies IDs' random bits;
    /// `speaking` lists utterances the world is playing, which playback events may name.
    pub fn next(
        &mut self,
        bytes: &mut Unstructured<'_>,
        mix: &InputMix,
        now_ms: u64,
        rng: &mut CoreRng,
        speaking: &[SpeakId],
    ) -> arbitrary::Result<(InputKind, Input)> {
        let kind = pick_kind(bytes, mix)?;
        let input = match kind {
            InputKind::Voice => self.voice(bytes, now_ms, rng)?,
            InputKind::User => user(bytes)?,
            InputKind::Playback => playback(bytes, speaking)?,
            InputKind::Timer => {
                self.next_timer = self.next_timer.wrapping_add(1);
                Input::Timer(TimerId(self.next_timer))
            }
        };
        Ok((kind, input))
    }

    fn voice(
        &mut self,
        bytes: &mut Unstructured<'_>,
        now_ms: u64,
        rng: &mut CoreRng,
    ) -> arbitrary::Result<Input> {
        let Some(turn) = self.turn else {
            let turn = TurnId::from_parts(now_ms, rng.ulid_random());
            self.turn = Some(turn);
            self.spoke = true;
            return Ok(Input::Voice(VoiceEvent::SpeechStarted { turn, source: 0 }));
        };
        let event = match bytes.int_in_range(0_u8..=3)? {
            0 if !self.spoke => {
                self.spoke = true;
                VoiceEvent::SpeechStarted { turn, source: 0 }
            }
            0 | 1 => VoiceEvent::Partial { turn, text: phrase(bytes)?, stable: bytes.arbitrary()? },
            2 if self.spoke => {
                self.spoke = false;
                VoiceEvent::SpeechEnded { turn }
            }
            _ => {
                self.turn = None;
                self.spoke = false;
                VoiceEvent::EndOfTurn {
                    turn,
                    text: phrase(bytes)?,
                    smart_turn_pct: bytes.int_in_range(0..=100)?,
                    silence_ms: bytes.int_in_range(0..=2_000)?,
                    asr_confidence_pct: bytes.int_in_range(0..=100)?,
                    reason: *bytes.choose(&[
                        TurnEndReason::SmartTurn,
                        TurnEndReason::Silence,
                        TurnEndReason::MaxLength,
                        TurnEndReason::Muted,
                    ])?,
                    source: 0,
                    level_db: bytes.int_in_range(-60..=-10)?,
                }
            }
        };
        Ok(Input::Voice(event))
    }
}

fn pick_kind(bytes: &mut Unstructured<'_>, mix: &InputMix) -> arbitrary::Result<InputKind> {
    let total = mix.total().max(1);
    let mut draw = bytes.int_in_range(0..=total - 1)?;
    for (kind, weight) in [
        (InputKind::Voice, mix.voice),
        (InputKind::User, mix.user),
        (InputKind::Playback, mix.playback),
        (InputKind::Timer, mix.timer),
    ] {
        if draw < weight {
            return Ok(kind);
        }
        draw -= weight;
    }
    Ok(InputKind::Timer)
}

fn phrase(bytes: &mut Unstructured<'_>) -> arbitrary::Result<Personal<String>> {
    Ok(Personal::new((*bytes.choose(PHRASES)?).to_owned()))
}

fn user(bytes: &mut Unstructured<'_>) -> arbitrary::Result<Input> {
    let input = match bytes.int_in_range(0_u8..=5)? {
        0 => UserInput::SetMute(true),
        1 => UserInput::SetMute(false),
        2 => UserInput::ToggleMute,
        3 => UserInput::Typed { text: phrase(bytes)? },
        4 => UserInput::Cli { text: phrase(bytes)? },
        _ => UserInput::DictationKey(if bytes.arbitrary()? { KeyEdge::Down } else { KeyEdge::Up }),
    };
    Ok(Input::User(input))
}

fn playback(bytes: &mut Unstructured<'_>, speaking: &[SpeakId]) -> arbitrary::Result<Input> {
    let event = match (bytes.int_in_range(0_u8..=3)?, speaking.is_empty()) {
        (0 | 1, false) => PlaybackEvent::Drained { speak: *bytes.choose(speaking)? },
        (2, _) => PlaybackEvent::Stalled,
        _ => PlaybackEvent::DeviceChanged,
    };
    Ok(Input::Playback(event))
}

#[cfg(test)]
mod tests {
    use arbitrary::Unstructured;
    use eileen_contracts::core_io::Input;
    use eileen_contracts::speech::VoiceEvent;
    use eileen_stdx::rng::CoreRng;

    use super::{Generator, InputKind};
    use crate::profile::InputMix;

    const VOICE_ONLY: InputMix = InputMix { voice: 1, user: 0, playback: 0, timer: 0 };

    #[test]
    fn a_turn_starts_before_its_other_events_and_ends_once() {
        let mut rng = CoreRng::from_seed(7);
        let mut buffer = vec![0_u8; 4_096];
        rng.fill_bytes(&mut buffer);
        let mut bytes = Unstructured::new(&buffer);
        let mut generator = Generator::default();
        let mut open = None;
        for step in 0..500 {
            let (kind, input) =
                generator.next(&mut bytes, &VOICE_ONLY, step, &mut rng, &[]).unwrap();
            assert_eq!(kind, InputKind::Voice, "the mix allows only voice");
            let Input::Voice(event) = input else { panic!("a voice input") };
            match event {
                VoiceEvent::SpeechStarted { turn, .. } => {
                    assert!(open.is_none_or(|o| o == turn), "one turn at a time");
                    open = Some(turn);
                }
                VoiceEvent::Partial { turn, .. } | VoiceEvent::SpeechEnded { turn } => {
                    assert_eq!(Some(turn), open, "within the open turn");
                }
                VoiceEvent::EndOfTurn { turn, .. } => {
                    assert_eq!(Some(turn), open, "ends the open turn");
                    open = None;
                }
                other @ (VoiceEvent::Calibration(_)
                | VoiceEvent::BargeInWindow { .. }
                | VoiceEvent::ClauseReady { .. }
                | VoiceEvent::SpeakDone { .. }
                | VoiceEvent::Embedding { .. }
                | VoiceEvent::CheckResult { .. }
                | VoiceEvent::SpeakerMatch { .. }
                | VoiceEvent::Health { .. }) => panic!("the generator does not draw {other:?}"),
            }
        }
    }

    #[test]
    fn an_exhausted_buffer_still_yields_inputs() {
        let mut rng = CoreRng::from_seed(1);
        let mut bytes = Unstructured::new(&[]);
        let mut generator = Generator::default();
        let mix = InputMix { voice: 1, user: 1, playback: 1, timer: 1 };
        for step in 0..10 {
            assert!(generator.next(&mut bytes, &mix, step, &mut rng, &[]).is_ok(), "step {step}");
        }
    }
}
