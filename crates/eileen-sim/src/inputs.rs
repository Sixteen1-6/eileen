//! The input generator: every input of a run is drawn from an [`arbitrary::Unstructured`]
//! buffer, so the same code serves a seeded run (the seed fills the buffer through
//! [`CoreRng`]), the `sim` fuzz target (the fuzzer's bytes are the buffer) and later a
//! minimiser (a shorter buffer is a smaller run).
//!
//! The generator draws what the user and the PC do: the speech process's turn events (speech
//! start, partial, speech end, end of turn) over one turn at a time, some of them named, some
//! room talk, barge-in windows while Eileen speaks, the user's mute, unmute and toggle, typed
//! and command-line text, the dictation key, playback faults (an utterance drained early, a
//! device change, a stall), what the PC and the room do (other audio starting and stopping,
//! physical input, another app taking the microphone), and the passing of time to the core's
//! next timer. What answers the core (the speech process's clauses, playback reports, the
//! brain, workers, memory readings) comes from the world's parts, not from here. A seeded run
//! hands the turn events to the world's speech part (`World::hear`), whose audio ring keeps
//! them while the speech process is not ready and replays them once it is.

use arbitrary::Unstructured;
use eileen_contracts::core_io::{ActivityEvent, Input, KeyEdge, PlaybackEvent, UserInput};
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
    "Eileen, what time is it?",
    "Eileen, tell me about rivers",
    "Eileen, create a file called hello dot txt",
    "mm-hm",
    "yeah",
    "want coffee?",
    "Eileen, stop",
    "cancel",
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
    /// Time passing to the core's next timer.
    Timer,
    /// What the PC and the room do.
    Activity,
}

/// The chance, in percent, that a voice input drawn while Eileen speaks is a barge-in window,
/// for a generator built without a profile.
const BARGE_IN_PCT: u8 = 33;

/// What the generator keeps between inputs, so turns and utterances stay coherent: an end of
/// turn names the turn its speech start minted, and other audio and the microphone's other
/// user stop only after they started.
#[derive(Debug)]
pub struct Generator {
    turn: Option<TurnId>,
    spoke: bool,
    window: u8,
    barge_in_pct: u8,
    other_audio: bool,
    mic_elsewhere: bool,
}

impl Default for Generator {
    fn default() -> Self {
        Self::new(BARGE_IN_PCT, false)
    }
}

impl Generator {
    /// A generator that draws a barge-in window for `barge_in_pct` percent of the voice inputs
    /// drawn while Eileen speaks, and starts with other audio playing when `other_audio` is
    /// true.
    #[must_use]
    pub const fn new(barge_in_pct: u8, other_audio: bool) -> Self {
        Self {
            turn: None,
            spoke: false,
            window: 0,
            barge_in_pct,
            other_audio,
            mic_elsewhere: false,
        }
    }

    /// The turn the next voice event belongs to, if one is open.
    #[must_use]
    pub const fn open_turn(&self) -> Option<TurnId> {
        self.turn
    }

    /// Draws the next input, or `None` to let virtual time pass to the core's next timer.
    /// `now_ms` stamps any ID it mints; `rng` supplies IDs' random bits; `speaking` lists
    /// utterances the world is playing, which playback events and barge-in windows may name.
    pub fn next(
        &mut self,
        bytes: &mut Unstructured<'_>,
        mix: &InputMix,
        now_ms: u64,
        rng: &mut CoreRng,
        speaking: &[SpeakId],
    ) -> arbitrary::Result<(InputKind, Option<Input>)> {
        let kind = pick_kind(bytes, mix)?;
        let input = match kind {
            InputKind::Voice => Some(self.voice(bytes, now_ms, rng, speaking)?),
            InputKind::User => Some(user(bytes)?),
            InputKind::Playback => Some(playback(bytes, speaking)?),
            InputKind::Timer => None,
            InputKind::Activity => Some(self.activity(bytes)?),
        };
        Ok((kind, input))
    }

    fn voice(
        &mut self,
        bytes: &mut Unstructured<'_>,
        now_ms: u64,
        rng: &mut CoreRng,
        speaking: &[SpeakId],
    ) -> arbitrary::Result<Input> {
        if let Some(speak) = speaking.first().copied()
            && self.barge_in_pct > 0
            && bytes.ratio(self.barge_in_pct.min(100), 100)?
        {
            // Mostly consecutive windows, now and then a gap, as speech over Eileen comes.
            let gap = if bytes.ratio(1_u8, 8)? { 2 } else { 1 };
            self.window = self.window.wrapping_add(gap);
            let prob_pct = bytes.int_in_range(50..=100)?;
            return Ok(Input::Voice(VoiceEvent::BargeInWindow {
                speak,
                window: self.window,
                prob_pct,
            }));
        }
        let Some(turn) = self.turn else {
            let turn = TurnId::from_parts(now_ms, rng.ulid_random());
            self.turn = Some(turn);
            self.spoke = true;
            return Ok(Input::Voice(VoiceEvent::SpeechStarted {
                turn,
                source: 0,
                replayed: false,
            }));
        };
        let event = match bytes.int_in_range(0_u8..=3)? {
            0 if !self.spoke => {
                self.spoke = true;
                VoiceEvent::SpeechStarted { turn, source: 0, replayed: false }
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

    /// Other audio starts or stops, the user's hands touch the keyboard or mouse, or another
    /// app takes or gives back the microphone.
    fn activity(&mut self, bytes: &mut Unstructured<'_>) -> arbitrary::Result<Input> {
        let event = match bytes.int_in_range(0_u8..=3)? {
            0 | 1 => {
                self.other_audio = !self.other_audio;
                ActivityEvent::OtherAudio { playing: self.other_audio }
            }
            2 => ActivityEvent::PhysicalInput,
            _ => {
                self.mic_elsewhere = !self.mic_elsewhere;
                ActivityEvent::MicInUseElsewhere(self.mic_elsewhere)
            }
        };
        Ok(Input::Activity(event))
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
        (InputKind::Activity, mix.activity),
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
    use eileen_contracts::core_io::{ActivityEvent, Input};
    use eileen_contracts::ids::SpeakId;
    use eileen_contracts::speech::VoiceEvent;
    use eileen_stdx::rng::CoreRng;

    use super::{Generator, InputKind};
    use crate::profile::InputMix;

    const VOICE_ONLY: InputMix = InputMix { voice: 1, user: 0, playback: 0, timer: 0, activity: 0 };
    const ACTIVITY_ONLY: InputMix =
        InputMix { voice: 0, user: 0, playback: 0, timer: 0, activity: 1 };

    #[test]
    fn a_turn_starts_before_its_other_events_and_ends_once() {
        let mut rng = CoreRng::from_seed(7);
        let mut buffer = vec![0_u8; 4_096];
        rng.fill_bytes(&mut buffer);
        let mut bytes = Unstructured::new(&buffer);
        let mut generator = Generator::default();
        let mut open = None;
        for step in 0..500 {
            let (kind, draw) =
                generator.next(&mut bytes, &VOICE_ONLY, step, &mut rng, &[]).unwrap();
            assert_eq!(kind, InputKind::Voice, "the mix allows only voice");
            let Some(Input::Voice(event)) = draw else { panic!("a voice input") };
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
        let mix = InputMix { voice: 1, user: 1, playback: 1, timer: 1, activity: 1 };
        for step in 0..10 {
            assert!(generator.next(&mut bytes, &mix, step, &mut rng, &[]).is_ok(), "step {step}");
        }
    }

    #[test]
    fn other_audio_and_the_microphone_stop_only_after_they_started() {
        let mut rng = CoreRng::from_seed(3);
        let mut buffer = vec![0_u8; 4_096];
        rng.fill_bytes(&mut buffer);
        let mut bytes = Unstructured::new(&buffer);
        // A video plays from the start, as the media-playing profile has it.
        let mut generator = Generator::new(33, true);
        let (mut playing, mut taken) = (true, false);
        let mut seen = [false; 3];
        for step in 0..300 {
            let (kind, draw) =
                generator.next(&mut bytes, &ACTIVITY_ONLY, step, &mut rng, &[]).unwrap();
            assert_eq!(kind, InputKind::Activity, "the mix allows only activity");
            let Some(Input::Activity(event)) = &draw else { panic!("the generator drew {draw:?}") };
            match *event {
                ActivityEvent::OtherAudio { playing: now } => {
                    assert_ne!(now, playing, "each report changes the state");
                    playing = now;
                    seen[0] = true;
                }
                ActivityEvent::MicInUseElsewhere(now) => {
                    assert_ne!(now, taken, "each report changes the state");
                    taken = now;
                    seen[1] = true;
                }
                ActivityEvent::PhysicalInput => seen[2] = true,
                other @ (ActivityEvent::SessionLocked(_) | ActivityEvent::Presentation(_)) => {
                    panic!("the generator drew {other:?}")
                }
            }
        }
        assert_eq!(seen, [true; 3], "every kind of activity is drawn");
    }

    #[test]
    fn the_barge_in_chance_follows_the_profile() {
        let speak = SpeakId::from_parts(1, 1);
        let windows = |pct| {
            let mut rng = CoreRng::from_seed(5);
            let mut buffer = vec![0_u8; 8_192];
            rng.fill_bytes(&mut buffer);
            let mut bytes = Unstructured::new(&buffer);
            let mut generator = Generator::new(pct, false);
            (0..400)
                .filter(|step| {
                    let (_, draw) =
                        generator.next(&mut bytes, &VOICE_ONLY, *step, &mut rng, &[speak]).unwrap();
                    matches!(draw, Some(Input::Voice(VoiceEvent::BargeInWindow { .. })))
                })
                .count()
        };
        assert_eq!(windows(0), 0, "never at 0 percent");
        assert_eq!(windows(100), 400, "always at 100 percent");
        let (low, high) = (windows(20), windows(70));
        assert!(low < high, "70 percent draws more windows than 20: {low} and {high}");
    }
}
