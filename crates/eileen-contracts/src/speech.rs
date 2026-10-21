//! The speech pipe (BUILD.md section 3.3.7) and what flows through it.
//!
//! Named pipe `\\.\pipe\eileen-speech-<session>` (a Unix socket under `$XDG_RUNTIME_DIR` in
//! Linux tests), user-only ACL, created by the core and opened by `eileen-speech` with a token
//! from its environment. Every frame is a [`FrameHeader`] and `len` bytes of body; `len` is
//! checked against the kind's maximum before anything is allocated ([`FrameHeader::decode`]).
//! VAD, ASR and barge-in state are keyed by `source`. PCM goes straight to the audio driver; the
//! core only sees `ClauseReady`, `Played` and `Paused`.
//!
//! Two framing details are WP0 placeholders that WP4 settles with its `speech_frame` parser,
//! since the plan leaves them open: how the JSON [`PcmHeader`] of a PCM frame ends (a newline
//! here) and the cap on command and event bodies ([`FrameKind::max_len`], 256 KiB here). Both
//! are code, not schema, so settling them changes no snapshot.

use std::collections::BTreeMap;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{Personal, SpeakId, TraceCtx, TurnId};
use crate::schema::Registry;

/// The fixed header of every speech-pipe frame: 32 bytes, little-endian on the wire.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct FrameHeader {
    /// The frame kind (see [`FrameKind`]).
    pub kind: u8,
    /// Reserved flags, zero today.
    pub flags: u8,
    /// The audio source: 0 is the PC microphone; later audio connectors (earbuds, a phone) get
    /// their own number.
    pub source: u16,
    /// The body's length in bytes.
    pub len: u32,
    /// The sender's frame counter.
    pub seq: u64,
    /// The turn's ULID bytes, or zeros.
    pub turn: [u8; 16],
}

/// The kinds of speech-pipe frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    /// 1, core to speech: 512 samples of `i16`, 16 kHz mono (one 32 ms window), `len` 1024.
    Audio,
    /// 2, core to speech: JSON [`SpeechCommandFrame`].
    Command,
    /// 3, speech to core: JSON [`SpeechEventFrame`].
    Event,
    /// 4, speech to core: JSON [`PcmHeader`], then `i16` samples; at most 1 MiB.
    Pcm,
    /// 5, a heartbeat every second.
    Ping,
    /// 6, the heartbeat's answer.
    Pong,
}

impl FrameKind {
    /// Every kind, in wire order.
    pub const ALL: [Self; 6] =
        [Self::Audio, Self::Command, Self::Event, Self::Pcm, Self::Ping, Self::Pong];

    /// The kind's byte on the wire.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Audio => 1,
            Self::Command => 2,
            Self::Event => 3,
            Self::Pcm => 4,
            Self::Ping => 5,
            Self::Pong => 6,
        }
    }

    /// The kind for a wire byte.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Audio),
            2 => Some(Self::Command),
            3 => Some(Self::Event),
            4 => Some(Self::Pcm),
            5 => Some(Self::Ping),
            6 => Some(Self::Pong),
            _ => None,
        }
    }

    /// The largest body this kind may carry, in bytes. The 256 KiB for commands and events is a
    /// placeholder WP4 settles (see the module doc).
    #[must_use]
    pub const fn max_len(self) -> u32 {
        match self {
            Self::Audio => AUDIO_FRAME_BYTES,
            Self::Command | Self::Event => 256 * 1024,
            Self::Pcm => 1024 * 1024,
            Self::Ping | Self::Pong => 0,
        }
    }
}

/// The body of an audio frame: 512 samples of 2 bytes.
pub const AUDIO_FRAME_BYTES: u32 = 1024;

/// Why a frame header was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// An unknown kind byte.
    UnknownKind(u8),
    /// A body longer than the kind allows (or an audio frame of the wrong size).
    BadLength {
        /// The kind.
        kind: FrameKind,
        /// The length the header claims.
        len: u32,
    },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKind(code) => write!(f, "unknown speech frame kind {code}"),
            Self::BadLength { kind, len } => {
                write!(f, "a {kind:?} frame may not be {len} bytes (at most {})", kind.max_len())
            }
        }
    }
}

impl std::error::Error for FrameError {}

impl FrameHeader {
    /// The header's size on the wire.
    pub const SIZE: usize = 32;

    /// The header as wire bytes.
    #[must_use]
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut out = [0_u8; Self::SIZE];
        let parts: [&[u8]; 6] = [
            &[self.kind],
            &[self.flags],
            &self.source.to_le_bytes(),
            &self.len.to_le_bytes(),
            &self.seq.to_le_bytes(),
            &self.turn,
        ];
        let mut slots = out.iter_mut();
        for byte in parts.into_iter().flatten() {
            if let Some(slot) = slots.next() {
                *slot = *byte;
            }
        }
        out
    }

    /// Reads a header and checks its kind and length before the caller allocates the body.
    pub fn decode(bytes: &[u8; Self::SIZE]) -> Result<(FrameKind, Self), FrameError> {
        let [kind, flags, s0, s1, l0, l1, l2, l3, q0, q1, q2, q3, q4, q5, q6, q7, turn @ ..] =
            *bytes;
        let header = Self {
            kind,
            flags,
            source: u16::from_le_bytes([s0, s1]),
            len: u32::from_le_bytes([l0, l1, l2, l3]),
            seq: u64::from_le_bytes([q0, q1, q2, q3, q4, q5, q6, q7]),
            turn,
        };
        let frame_kind = FrameKind::from_code(kind).ok_or(FrameError::UnknownKind(kind))?;
        let len_ok = match frame_kind {
            FrameKind::Audio => header.len == AUDIO_FRAME_BYTES,
            FrameKind::Command
            | FrameKind::Event
            | FrameKind::Pcm
            | FrameKind::Ping
            | FrameKind::Pong => header.len <= frame_kind.max_len(),
        };
        if len_ok {
            Ok((frame_kind, header))
        } else {
            Err(FrameError::BadLength { kind: frame_kind, len: header.len })
        }
    }
}

/// The body of a command frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpeechCommandFrame {
    /// Correlation.
    pub ctx: TraceCtx,
    /// The core's span the command belongs to.
    pub span: u64,
    /// The command.
    pub cmd: SpeechCmd,
}

/// The body of an event frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpeechEventFrame {
    /// Correlation.
    pub ctx: TraceCtx,
    /// The speech process's span the event belongs to.
    pub span: u64,
    /// The event.
    pub event: VoiceEvent,
}

/// The JSON part of a PCM frame; the samples follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PcmHeader {
    /// The utterance.
    pub speak: SpeakId,
    /// Its clause.
    pub clause: u16,
    /// The samples' rate, Hz.
    pub sample_rate: u32,
}

/// The languages a component handles, as BCP 47 tags, reported by the component itself: a
/// speech stage from its model's entry in `models.toml`, a brain backend or worker driver for
/// its resolved model (the Claude backend and driver report `Any`). Eileen's code holds no list
/// of supported languages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LanguageSupport {
    /// Any language.
    Any,
    /// These languages.
    Listed(Vec<String>),
}

/// From the speech process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceEvent {
    /// Speech began; the speech process minted the turn.
    SpeechStarted {
        /// The turn.
        turn: TurnId,
        /// The audio source.
        source: u16,
    },
    /// A partial transcript; it reaches only the HUD state while it is current.
    Partial {
        /// The turn.
        turn: TurnId,
        /// The text so far.
        text: Personal<String>,
        /// Whether the recognizer will not change it.
        stable: bool,
    },
    /// Speech stopped (the turn may continue).
    SpeechEnded {
        /// The turn.
        turn: TurnId,
    },
    /// The turn ended, with its final transcript.
    EndOfTurn {
        /// The turn.
        turn: TurnId,
        /// The final transcript.
        text: Personal<String>,
        /// The end-of-turn model's confidence, percent.
        smart_turn_pct: u8,
        /// Silence before the end, ms.
        silence_ms: u32,
        /// The recognizer's confidence, percent.
        asr_confidence_pct: u8,
        /// Why the turn ended.
        reason: TurnEndReason,
        /// The audio source.
        source: u16,
        /// The active speech level: the median RMS of the turn's VAD-speech frames,
        /// echo-cancelled, before Eileen's input gain, in dBFS.
        level_db: i16,
    },
    /// After the first-run clip is measured; `None` after "forget my voice".
    Calibration(Option<VoiceCalibration>),
    /// A barge-in window while Eileen speaks.
    BargeInWindow {
        /// The utterance being spoken.
        speak: SpeakId,
        /// The window's number since playback began.
        window: u8,
        /// The probability the user is speaking to interrupt, percent.
        prob_pct: u8,
    },
    /// A clause is synthesized and its PCM sent.
    ClauseReady {
        /// The utterance.
        speak: SpeakId,
        /// The clause.
        clause: u16,
        /// Its characters.
        chars: u32,
        /// Its samples.
        samples: u32,
    },
    /// Every clause of the utterance is synthesized.
    SpeakDone {
        /// The utterance.
        speak: SpeakId,
    },
    /// An embedding the core asked for.
    Embedding {
        /// The request.
        request: u64,
        /// The vector.
        vector: Vec<f32>,
    },
    /// A check the core asked for: name recognisability, speaker match, clone timing.
    CheckResult {
        /// The request.
        request: u64,
        /// The result.
        result: SpeechCheck,
    },
    /// The speaker check's score for a turn, only if the check is adopted and enrolled.
    SpeakerMatch {
        /// The turn.
        turn: TurnId,
        /// The score, percent.
        score_pct: u8,
    },
    /// Real-time factor per engine, per mille.
    Health {
        /// Engine name to real-time factor.
        rtf_by_engine: BTreeMap<String, u16>,
    },
}

/// Why a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurnEndReason {
    /// The end-of-turn model said the user finished.
    SmartTurn,
    /// The silence limit passed.
    Silence,
    /// The turn reached its maximum length.
    MaxLength,
    /// The dictation key was released.
    KeyReleased,
    /// The microphone was muted.
    Muted,
}

/// The listening half of the first run's calibration (section 2.8), as stored in
/// `calibration.toml`: the addressee score's level reference and the name matcher's extra
/// aliases reach the core only through this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceCalibration {
    /// The user's active speech level, dBFS.
    pub level_ref_db: i16,
    /// The room's noise floor, dBFS.
    pub noise_floor_db: i16,
    /// How the recognizer hears the assistant's name in this voice.
    pub name_aliases: Vec<Personal<String>>,
}

/// A check the core asks the speech process to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpeechCheckRequest {
    /// Can the recognizer pick this name out of speech?
    NameRecognisable {
        /// The name.
        name: String,
    },
    /// Does the speaker of a turn match the enrolled voice?
    SpeakerMatch {
        /// The turn.
        turn: TurnId,
    },
    /// How fast does a cloned voice start speaking?
    CloneTiming {
        /// The voice.
        voice: String,
    },
    /// Does a voice render faster than real time?
    VoiceRealtime {
        /// The voice.
        voice: String,
        /// The engine.
        engine: Option<String>,
    },
}

/// A check's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpeechCheck {
    /// Whether the name was recognised in synthesized test sentences.
    NameRecognisable {
        /// Whether it passed.
        passed: bool,
        /// How many test sentences recognised it.
        hits: u8,
        /// Of how many.
        tries: u8,
    },
    /// The speaker check's score.
    SpeakerMatch {
        /// The score, percent.
        score_pct: u8,
    },
    /// Time to the first audio of a cloned voice.
    CloneTiming {
        /// Time to first audio, ms.
        ttfa_ms: u32,
    },
    /// A voice's real-time factor.
    VoiceRealtime {
        /// Per mille.
        rtf_per_mille: u16,
    },
    /// The check could not run.
    Unavailable {
        /// Why.
        reason: String,
    },
}

/// To the speech process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpeechCmd {
    /// Speak an utterance, clause by clause.
    Speak {
        /// The utterance.
        speak: SpeakId,
        /// The voice.
        voice: String,
        /// The clauses, in order.
        clauses: Vec<String>,
        /// The whole line, for engines that read ahead.
        whole_line: String,
    },
    /// Stop synthesizing an utterance.
    Cancel {
        /// The utterance.
        speak: SpeakId,
    },
    /// Continue an utterance after a pause.
    Resume {
        /// The utterance.
        speak: SpeakId,
    },
    /// Change how the speech process listens.
    SetMode(ListenMode),
    /// Words the recognizer should favour (the assistant's name, app names).
    SetHotwords {
        /// The words.
        words: Vec<String>,
    },
    /// Embed an utterance for the semantic matcher.
    Embed {
        /// The request.
        request: u64,
        /// The utterance.
        text: Personal<String>,
    },
    /// Run a check.
    Check {
        /// The request.
        request: u64,
        /// The check.
        check: SpeechCheckRequest,
    },
    /// Load a model.
    LoadModel {
        /// The model's name in `models.toml`.
        model: String,
    },
    /// Unload a model.
    UnloadModel {
        /// The model's name in `models.toml`.
        model: String,
    },
    /// Start an enrollment.
    Enroll {
        /// What to enroll.
        kind: EnrollKind,
    },
    /// Clone a voice from the recording just made.
    CloneFromRecording {
        /// The new voice's name.
        voice: String,
    },
    /// Delete a voice and its files.
    DeleteVoice {
        /// The voice.
        name: String,
    },
}

/// How the speech process listens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListenMode {
    /// Turns for the conversation.
    Conversation,
    /// Dictation into a field: no addressee gate, the key ends the turn.
    Dictation,
    /// An enrollment or the first-run clip.
    Enrollment,
}

/// What an enrollment records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnrollKind {
    /// The speaker check's voice print.
    SpeakerCheck,
    /// A voice to clone.
    Clone,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<FrameHeader>();
    r.add::<FrameKind>();
    r.add::<SpeechCommandFrame>();
    r.add::<SpeechEventFrame>();
    r.add::<PcmHeader>();
    r.add::<LanguageSupport>();
    r.add::<VoiceEvent>();
    r.add::<TurnEndReason>();
    r.add::<VoiceCalibration>();
    r.add::<SpeechCheckRequest>();
    r.add::<SpeechCheck>();
    r.add::<SpeechCmd>();
    r.add::<ListenMode>();
    r.add::<EnrollKind>();
}

#[cfg(test)]
mod tests {
    use super::{AUDIO_FRAME_BYTES, FrameError, FrameHeader, FrameKind};

    fn header(kind: u8, len: u32) -> FrameHeader {
        FrameHeader { kind, flags: 0, source: 2, len, seq: 0x0102_0304_0506_0708, turn: [9; 16] }
    }

    #[test]
    fn the_header_is_32_bytes_in_memory_and_on_the_wire() {
        assert_eq!(size_of::<FrameHeader>(), FrameHeader::SIZE, "repr(C) size");
        let bytes = header(2, 10).encode();
        assert_eq!(bytes[..8], [2, 0, 2, 0, 10, 0, 0, 0], "kind, flags, source, len");
        assert_eq!(bytes[8..16], [8, 7, 6, 5, 4, 3, 2, 1], "seq, little-endian");
        assert_eq!(bytes[16..], [9; 16], "turn");
    }

    #[test]
    fn decode_round_trips_every_kind() {
        for kind in FrameKind::ALL {
            let len = if kind == FrameKind::Audio { AUDIO_FRAME_BYTES } else { 0 };
            let sent = header(kind.code(), len);
            assert_eq!(FrameHeader::decode(&sent.encode()), Ok((kind, sent)), "{kind:?}");
        }
    }

    #[test]
    fn decode_checks_the_length_before_any_allocation() {
        let too_long = header(FrameKind::Pcm.code(), 1024 * 1024 + 1);
        assert_eq!(
            FrameHeader::decode(&too_long.encode()),
            Err(FrameError::BadLength { kind: FrameKind::Pcm, len: 1024 * 1024 + 1 }),
            "pcm over 1 MiB"
        );
        let short_audio = header(FrameKind::Audio.code(), 1022);
        assert!(FrameHeader::decode(&short_audio.encode()).is_err(), "audio is exactly 1024");
        let ping_body = header(FrameKind::Ping.code(), 1);
        assert!(FrameHeader::decode(&ping_body.encode()).is_err(), "ping has no body");
        assert_eq!(
            FrameHeader::decode(&header(0, 0).encode()),
            Err(FrameError::UnknownKind(0)),
            "kind 0"
        );
    }
}
