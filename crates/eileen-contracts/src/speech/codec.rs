//! Whole frames of the speech pipe as values, written to bytes and read back on top of
//! [`FrameHeader`]. The framing is in the parent module's doc.

use std::fmt;

use serde::Serialize;

use super::{
    AUDIO_FRAME_BYTES, AUDIO_FRAME_SAMPLES, AudioFlags, FrameError, FrameHeader, FrameKind,
    FrameSender, PCM_HEADER_MAX, PcmHeader, SpeechCommandFrame, SpeechEventFrame, SpeechHelloFrame,
};

/// The body of a frame, one variant per [`FrameKind`].
#[derive(Debug, Clone, PartialEq)]
pub enum FrameBody {
    /// One 32 ms window of microphone audio, 16 kHz mono.
    Audio(Box<[i16; AUDIO_FRAME_SAMPLES]>),
    /// A command to the speech process.
    Command(SpeechCommandFrame),
    /// An event from the speech process.
    Event(SpeechEventFrame),
    /// Synthesized speech for playback.
    Pcm {
        /// What the samples are.
        header: PcmHeader,
        /// The samples, at `header.sample_rate`.
        samples: Vec<i16>,
    },
    /// A heartbeat.
    Ping,
    /// A heartbeat's answer.
    Pong,
    /// The speech process's token, the first frame on every connection.
    Hello(SpeechHelloFrame),
}

impl FrameBody {
    /// The kind of frame that carries this body.
    #[must_use]
    pub const fn kind(&self) -> FrameKind {
        match self {
            Self::Audio(_) => FrameKind::Audio,
            Self::Command(_) => FrameKind::Command,
            Self::Event(_) => FrameKind::Event,
            Self::Pcm { .. } => FrameKind::Pcm,
            Self::Ping => FrameKind::Ping,
            Self::Pong => FrameKind::Pong,
            Self::Hello(_) => FrameKind::Hello,
        }
    }

    /// Reads the body of a frame of `kind` whose length its header check allowed.
    fn read(kind: FrameKind, body: &[u8]) -> Result<Self, FrameError> {
        let bad_json = |_| FrameError::BadJson { kind };
        Ok(match kind {
            FrameKind::Audio => {
                let mut samples = Box::new([0_i16; AUDIO_FRAME_SAMPLES]);
                let (pairs, rest) = body.as_chunks::<2>();
                if pairs.len() != AUDIO_FRAME_SAMPLES || !rest.is_empty() {
                    return Err(FrameError::OddSamples);
                }
                for (slot, pair) in samples.iter_mut().zip(pairs) {
                    *slot = i16::from_le_bytes(*pair);
                }
                Self::Audio(samples)
            }
            FrameKind::Command => Self::Command(serde_json::from_slice(body).map_err(bad_json)?),
            FrameKind::Event => Self::Event(serde_json::from_slice(body).map_err(bad_json)?),
            FrameKind::Pcm => {
                let end = body
                    .iter()
                    .take(PCM_HEADER_MAX)
                    .position(|byte| *byte == b'\n')
                    .ok_or(FrameError::MissingPcmTerminator)?;
                let (json, rest) = body.split_at(end);
                let header = serde_json::from_slice(json).map_err(bad_json)?;
                let samples = read_samples(rest.get(1..).unwrap_or_default())?;
                Self::Pcm { header, samples }
            }
            FrameKind::Ping => Self::Ping,
            FrameKind::Pong => Self::Pong,
            FrameKind::Hello => Self::Hello(serde_json::from_slice(body).map_err(bad_json)?),
        })
    }
}

fn write_json<T: Serialize>(
    out: &mut Vec<u8>,
    value: &T,
    kind: FrameKind,
) -> Result<(), FrameError> {
    serde_json::to_writer(out, value).map_err(|_| FrameError::BadJson { kind })
}

fn write_pcm(out: &mut Vec<u8>, header: &PcmHeader, samples: &[i16]) -> Result<(), FrameError> {
    let start = out.len();
    write_json(out, header, FrameKind::Pcm)?;
    out.push(b'\n');
    if out.len() - start > PCM_HEADER_MAX {
        return Err(FrameError::MissingPcmTerminator);
    }
    write_samples(out, samples);
    Ok(())
}

fn write_samples(out: &mut Vec<u8>, samples: &[i16]) {
    out.reserve(samples.len() * 2);
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
}

/// Writes an audio frame: the header, checked first, then the samples from the borrowed window.
/// The one path for audio, from [`Frame::encode_into`] and [`Frame::encode_audio_into`].
fn write_audio(
    header: &FrameHeader,
    samples: &[i16; AUDIO_FRAME_SAMPLES],
    out: &mut Vec<u8>,
) -> Result<(), FrameError> {
    header.check()?;
    out.reserve(FrameHeader::SIZE + AUDIO_FRAME_SAMPLES * 2);
    out.extend_from_slice(&header.encode());
    write_samples(out, samples);
    Ok(())
}

fn read_samples(bytes: &[u8]) -> Result<Vec<i16>, FrameError> {
    let (pairs, rest) = bytes.as_chunks::<2>();
    if rest.is_empty() {
        Ok(pairs.iter().map(|pair| i16::from_le_bytes(*pair)).collect())
    } else {
        Err(FrameError::OddSamples)
    }
}

/// A whole frame: the header fields its body does not decide, and the body. The header's kind
/// and length come from the body when the frame is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// The flags; only an audio frame may carry any ([`FrameHeader::flags`]).
    pub flags: AudioFlags,
    /// The audio source ([`FrameHeader::source`]).
    pub source: u16,
    /// The audio frame number this frame carries ([`FrameHeader::seq`]).
    pub seq: u64,
    /// The open turn's ULID bytes, or zeros ([`FrameHeader::turn`]).
    pub turn: [u8; 16],
    /// The body.
    pub body: FrameBody,
}

impl Frame {
    /// A frame with no flags, source 0, seq 0 and no turn.
    #[must_use]
    pub const fn new(body: FrameBody) -> Self {
        Self { flags: AudioFlags::NONE, source: 0, seq: 0, turn: [0; 16], body }
    }

    /// The frame's header, with the kind and the given body length.
    const fn header(&self, len: u32) -> FrameHeader {
        FrameHeader {
            kind: self.body.kind().code(),
            flags: self.flags.bits(),
            source: self.source,
            len,
            seq: self.seq,
            turn: self.turn,
        }
    }

    /// Appends the frame's bytes to `out`: the header, then the body. Refuses what
    /// [`FrameHeader::decode`] would refuse on the other end (flags on a kind other than audio,
    /// a body longer than its kind allows), and leaves `out` as it was when it does.
    pub fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        let kind = self.body.kind();
        match &self.body {
            FrameBody::Audio(samples) => write_audio(&self.header(AUDIO_FRAME_BYTES), samples, out),
            FrameBody::Command(frame) => self.write_sized(out, |out| write_json(out, frame, kind)),
            FrameBody::Event(frame) => self.write_sized(out, |out| write_json(out, frame, kind)),
            FrameBody::Pcm { header, samples } => {
                self.write_sized(out, |out| write_pcm(out, header, samples))
            }
            FrameBody::Ping | FrameBody::Pong => self.write_sized(out, |_| Ok(())),
            FrameBody::Hello(hello) => self.write_sized(out, |out| write_json(out, hello, kind)),
        }
    }

    /// Writes an audio frame, with no turn, from a borrowed window of samples: the bytes
    /// [`Frame::encode_into`] writes for the same frame, without boxing a copy of the window
    /// first, so the audio component writes each frame straight from its ring.
    pub fn encode_audio_into(
        flags: AudioFlags,
        source: u16,
        seq: u64,
        samples: &[i16; AUDIO_FRAME_SAMPLES],
        out: &mut Vec<u8>,
    ) -> Result<(), FrameError> {
        let header = FrameHeader {
            kind: FrameKind::Audio.code(),
            flags: flags.bits(),
            source,
            len: AUDIO_FRAME_BYTES,
            seq,
            turn: [0; 16],
        };
        write_audio(&header, samples, out)
    }

    /// Writes a body whose length is known only once it is written: room for the header, the
    /// body, then the checked header in that room; or, on an error, `out` as it was.
    fn write_sized(
        &self,
        out: &mut Vec<u8>,
        body: impl FnOnce(&mut Vec<u8>) -> Result<(), FrameError>,
    ) -> Result<(), FrameError> {
        let start = out.len();
        out.extend_from_slice(&[0; FrameHeader::SIZE]);
        let written = body(out).and_then(|()| {
            let len = out.len() - start - FrameHeader::SIZE;
            let header = self.header(u32::try_from(len).unwrap_or(u32::MAX));
            header.check()?;
            Ok(header)
        });
        match written {
            Ok(header) => {
                if let Some(slot) = out.get_mut(start..start + FrameHeader::SIZE) {
                    slot.copy_from_slice(&header.encode());
                }
                Ok(())
            }
            Err(error) => {
                out.truncate(start);
                Err(error)
            }
        }
    }

    /// The frame's bytes (see [`Frame::encode_into`]).
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        Ok(out)
    }

    /// Reads a frame from its header and its whole body. The header is checked again, and the
    /// body must be as long as the header says.
    pub fn decode(header: &FrameHeader, body: &[u8]) -> Result<Self, FrameError> {
        let kind = header.check()?;
        if usize::try_from(header.len).ok() != Some(body.len()) {
            return Err(FrameError::BadLength { kind, len: header.len });
        }
        let flags = AudioFlags::from_bits(header.flags)
            .ok_or(FrameError::BadFlags { kind, flags: header.flags })?;
        Ok(Self {
            flags,
            source: header.source,
            seq: header.seq,
            turn: header.turn,
            body: FrameBody::read(kind, body)?,
        })
    }
}

/// Reads frames from a byte stream as it arrives: give it what the connection read, and it
/// returns each frame once its last byte is in.
///
/// A reader belongs to one end of the pipe and reads what the other end sent, so it refuses a
/// kind that end may not send ([`FrameKind::sender`]). The core's readers also hold the speech
/// process to one `Hello`: [`FrameReader::for_core`] refuses any frame before the first `Hello`,
/// and both refuse a `Hello` after it. The transport reads the `Hello` with
/// [`FrameReader::for_core`]; the audio component, which takes the connection after that, reads
/// with [`FrameReader::for_core_after_hello`]; the speech process reads with
/// [`FrameReader::for_speech`].
///
/// It holds one frame at most: the header's 32 bytes, then a body buffer reserved only after
/// [`FrameHeader::decode`] has allowed the kind, flags and length and the reader has allowed the
/// kind from its peer, so a corrupt length never allocates. After an error the stream is out of
/// step and every later call returns the same error: close the connection. `Debug` shows lengths
/// only, never the bytes of a body, which may be a `Hello`'s token.
pub struct FrameReader {
    /// The end whose frames this reader takes.
    peer: FrameSender,
    /// Whether the next frame must be the speech process's `Hello`.
    awaiting_hello: bool,
    /// The header bytes read so far.
    header: [u8; FrameHeader::SIZE],
    /// How many of them.
    have: usize,
    /// The accepted header and the body read so far, once the header is whole.
    body: Option<(FrameHeader, usize, Vec<u8>)>,
    /// The error that stopped the stream.
    failed: Option<FrameError>,
}

impl fmt::Debug for FrameReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = self
            .body
            .as_ref()
            .map(|(header, len, body)| (FrameKind::from_code(header.kind), *len, body.len()));
        f.debug_struct("FrameReader")
            .field("peer", &self.peer)
            .field("awaiting_hello", &self.awaiting_hello)
            .field("header_bytes", &self.have)
            .field("body_kind_len_read", &body)
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

impl FrameReader {
    const fn reading(peer: FrameSender, awaiting_hello: bool) -> Self {
        Self {
            peer,
            awaiting_hello,
            header: [0; FrameHeader::SIZE],
            have: 0,
            body: None,
            failed: None,
        }
    }

    /// The core's reader at the start of a connection: the speech process's frames, the first of
    /// them its `Hello`.
    #[must_use]
    pub const fn for_core() -> Self {
        Self::reading(FrameSender::Speech, true)
    }

    /// The core's reader for a connection whose `Hello` was already read: the speech process's
    /// frames, never another `Hello`.
    #[must_use]
    pub const fn for_core_after_hello() -> Self {
        Self::reading(FrameSender::Speech, false)
    }

    /// The speech process's reader: the core's frames.
    #[must_use]
    pub const fn for_speech() -> Self {
        Self::reading(FrameSender::Core, false)
    }

    /// Allows a kind from this reader's peer at this place in the stream.
    fn admit(&mut self, kind: FrameKind) -> Result<(), FrameError> {
        let sender = kind.sender();
        if sender != FrameSender::Either && sender != self.peer {
            return Err(FrameError::WrongDirection { kind });
        }
        match (self.awaiting_hello, kind == FrameKind::Hello) {
            (true, true) => {
                self.awaiting_hello = false;
                Ok(())
            }
            (true, false) => Err(FrameError::BeforeHello { kind }),
            (false, true) => Err(FrameError::SecondHello),
            (false, false) => Ok(()),
        }
    }

    /// How many more bytes the current frame needs: the rest of its header, or of its body.
    #[must_use]
    pub const fn needed(&self) -> usize {
        match &self.body {
            Some((_, len, body)) => *len - body.len(),
            None => FrameHeader::SIZE - self.have,
        }
    }

    /// Takes bytes from the front of `input`, never past the end of the current frame, and
    /// returns how many it took and the frame they completed, if any. Call it again with the
    /// rest of `input` until all of it is taken.
    pub fn feed(&mut self, input: &[u8]) -> Result<(usize, Option<Frame>), FrameError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        let result = self.step(input);
        if let Err(error) = &result {
            self.failed = Some(*error);
            self.body = None;
        }
        result
    }

    fn step(&mut self, input: &[u8]) -> Result<(usize, Option<Frame>), FrameError> {
        let mut taken = 0;
        if self.body.is_none() {
            taken = self.needed().min(input.len());
            let into = self.header.get_mut(self.have..self.have + taken);
            if let (Some(into), Some(from)) = (into, input.get(..taken)) {
                into.copy_from_slice(from);
            }
            self.have += taken;
            if self.have < FrameHeader::SIZE {
                return Ok((taken, None));
            }
            self.have = 0;
            let (kind, header) = FrameHeader::decode(&self.header)?;
            self.admit(kind)?;
            let len = usize::try_from(header.len).unwrap_or(0);
            self.body = Some((header, len, Vec::with_capacity(len)));
        }
        let rest = input.get(taken..).unwrap_or_default();
        let more = self.needed().min(rest.len());
        if let Some((_, _, body)) = self.body.as_mut() {
            body.extend_from_slice(rest.get(..more).unwrap_or_default());
        }
        taken += more;
        if self.needed() > 0 {
            return Ok((taken, None));
        }
        match self.body.take() {
            Some((header, _, body)) => Ok((taken, Some(Frame::decode(&header, &body)?))),
            None => Ok((taken, None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Frame, FrameBody, FrameReader};
    use crate::harness::SessionToken;
    use crate::ids::{SpeakId, TraceCtx, TurnId};
    use crate::speech::{
        AUDIO_FRAME_BYTES, AUDIO_FRAME_SAMPLES, AudioFlags, FrameError, FrameHeader, FrameKind,
        FrameSender, HELLO_BODY_MAX, JSON_BODY_MAX, PCM_BODY_MAX, PCM_HEADER_MAX, PcmHeader,
        SpeechCmd, SpeechCommandFrame, SpeechEventFrame, SpeechHelloFrame, VoiceEvent,
    };

    fn speak() -> SpeakId {
        "speak_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap()
    }

    fn turn() -> TurnId {
        "turn_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap()
    }

    fn hello(token: &str) -> Frame {
        let token = SessionToken::new(token.to_owned());
        Frame::new(FrameBody::Hello(SpeechHelloFrame { token }))
    }

    /// A header of `kind` with an allowed length and nothing else.
    fn bare(kind: FrameKind) -> FrameHeader {
        let len = if kind == FrameKind::Audio { AUDIO_FRAME_BYTES } else { 0 };
        FrameHeader { kind: kind.code(), flags: 0, source: 0, len, seq: 0, turn: [0; 16] }
    }

    /// One frame of every kind, in wire order, with the header fields D81 gives each direction.
    fn one_of_each() -> Vec<Frame> {
        let mut audio = Box::new([0_i16; AUDIO_FRAME_SAMPLES]);
        for (n, sample) in audio.iter_mut().enumerate() {
            *sample = i16::try_from(n).unwrap() * 61 - 15_000;
        }
        let frames = vec![
            Frame {
                flags: AudioFlags::REPLAYED | AudioFlags::OTHER_AUDIO,
                source: 0,
                seq: 41,
                turn: [0; 16],
                body: FrameBody::Audio(audio),
            },
            Frame::new(FrameBody::Command(SpeechCommandFrame {
                ctx: TraceCtx::default(),
                span: 7,
                cmd: SpeechCmd::Cancel { speak: speak() },
            })),
            Frame {
                seq: 41,
                ..Frame::new(FrameBody::Event(SpeechEventFrame {
                    ctx: TraceCtx { turn: Some(turn()), job: None, call: None },
                    span: 9,
                    event: VoiceEvent::SpeechStarted { turn: turn(), source: 0, replayed: true },
                }))
            },
            Frame {
                seq: 41,
                ..Frame::new(FrameBody::Pcm {
                    header: PcmHeader { speak: speak(), clause: 2, sample_rate: 24_000 },
                    samples: vec![i16::MIN, -1, 0, 1, i16::MAX],
                })
            },
            Frame { seq: 41, ..Frame::new(FrameBody::Ping) },
            Frame { seq: 41, turn: turn().ulid().to_bytes(), ..Frame::new(FrameBody::Pong) },
            hello(&"a1b2".repeat(16)),
        ];
        let kinds: Vec<_> = frames.iter().map(|frame| frame.body.kind()).collect();
        assert_eq!(kinds, FrameKind::ALL, "one frame of every kind, in wire order");
        frames
    }

    /// What each end sends: the core's frames, and the speech process's with its `Hello` first.
    fn by_sender(frames: Vec<Frame>) -> (Vec<Frame>, Vec<Frame>) {
        let (hellos, rest): (Vec<_>, Vec<_>) =
            frames.into_iter().partition(|frame| frame.body.kind() == FrameKind::Hello);
        let (from_core, others): (Vec<_>, Vec<_>) =
            rest.into_iter().partition(|frame| frame.body.kind().sender() == FrameSender::Core);
        let mut from_speech = hellos;
        from_speech.extend(others);
        (from_core, from_speech)
    }

    fn stream(frames: &[Frame]) -> Vec<u8> {
        let mut out = Vec::new();
        for frame in frames {
            frame.encode_into(&mut out).unwrap();
        }
        out
    }

    /// Feeds `pieces` to `reader` one after another and collects the frames.
    fn read_pieces<'a>(
        mut reader: FrameReader,
        pieces: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<Vec<Frame>, FrameError> {
        let mut frames = Vec::new();
        for mut piece in pieces {
            while !piece.is_empty() {
                let (taken, frame) = reader.feed(piece)?;
                assert!(taken > 0, "the reader takes at least one byte of a non-empty input");
                frames.extend(frame);
                piece = &piece[taken..];
            }
        }
        assert_eq!(reader.needed(), FrameHeader::SIZE, "the stream ends between frames");
        Ok(frames)
    }

    /// Reads every frame from `stream`, giving `reader` `chunk` bytes at a time.
    fn read_all(
        reader: FrameReader,
        stream: &[u8],
        chunk: usize,
    ) -> Result<Vec<Frame>, FrameError> {
        read_pieces(reader, stream.chunks(chunk))
    }

    #[test]
    fn every_kind_round_trips_whole_and_a_byte_at_a_time() {
        let frames = one_of_each();
        let audio = frames[0].encode().unwrap();
        let (from_core, from_speech) = by_sender(frames);
        let (core_bytes, speech_bytes) = (stream(&from_core), stream(&from_speech));
        for chunk in [core_bytes.len(), 1, 7, FrameHeader::SIZE] {
            let read = read_all(FrameReader::for_speech(), &core_bytes, chunk).unwrap();
            assert_eq!(read, from_core, "the core's frames in chunks of {chunk}");
        }
        for chunk in [speech_bytes.len(), 1, 7, FrameHeader::SIZE] {
            let read = read_all(FrameReader::for_core(), &speech_bytes, chunk).unwrap();
            assert_eq!(read, from_speech, "the speech process's frames in chunks of {chunk}");
        }
        assert_eq!(audio.len(), FrameHeader::SIZE + 1024, "an audio frame is 1056 bytes");
        assert_eq!(audio[1], 0b1001, "replayed is bit 0 and other audio bit 3");
        assert_eq!(audio[8..16], 41_u64.to_le_bytes(), "seq");
    }

    #[test]
    fn the_flag_bits_are_fixed() {
        let bits = [
            AudioFlags::REPLAYED,
            AudioFlags::BREAK,
            AudioFlags::PLAYBACK,
            AudioFlags::OTHER_AUDIO,
        ]
        .map(AudioFlags::bits);
        assert_eq!(bits, [1, 2, 4, 8], "D81's flags in order, one bit each");
        let all = AudioFlags::from_bits(0b1111).unwrap();
        assert!(all.contains(AudioFlags::BREAK | AudioFlags::PLAYBACK), "contains");
        assert_eq!(AudioFlags::from_bits(0x10), None, "bit 4 is no flag");
        assert!(AudioFlags::default().is_empty(), "the default is no flag");
    }

    #[test]
    fn an_audio_frame_from_a_borrowed_window_is_the_frame_encode_writes() {
        let mut window = [0_i16; AUDIO_FRAME_SAMPLES];
        for (n, sample) in window.iter_mut().enumerate() {
            *sample = i16::try_from(n).unwrap() - 256;
        }
        let flags = AudioFlags::BREAK | AudioFlags::PLAYBACK;
        let frame = Frame {
            flags,
            source: 3,
            seq: 9_000,
            turn: [0; 16],
            body: FrameBody::Audio(Box::new(window)),
        };
        let mut out = vec![7];
        Frame::encode_audio_into(flags, 3, 9_000, &window, &mut out).unwrap();
        assert_eq!(out[0], 7, "appended after what was there");
        assert_eq!(out[1..], frame.encode().unwrap(), "the same bytes as encode");
        let read = read_all(FrameReader::for_speech(), &out[1..], 100).unwrap();
        assert_eq!(read, [frame], "and it reads back");
    }

    #[test]
    fn an_over_length_len_is_refused_before_any_body_byte_is_read() {
        let over = |kind: FrameKind, len: u32| FrameHeader { len, ..bare(kind) };
        let cases = [
            (FrameKind::Audio, AUDIO_FRAME_BYTES + 1),
            (FrameKind::Audio, AUDIO_FRAME_BYTES - 2),
            (FrameKind::Command, JSON_BODY_MAX + 1),
            (FrameKind::Event, JSON_BODY_MAX + 1),
            (FrameKind::Pcm, PCM_BODY_MAX + 1),
            (FrameKind::Ping, 1),
            (FrameKind::Pong, 1),
            (FrameKind::Hello, HELLO_BODY_MAX + 1),
            (FrameKind::Pcm, u32::MAX),
        ];
        for (kind, len) in cases {
            let header = over(kind, len).encode();
            let wanted = Some(FrameError::BadLength { kind, len });
            for mut reader in [FrameReader::for_core(), FrameReader::for_speech()] {
                assert_eq!(reader.feed(&header).err(), wanted, "{kind:?} {len}: refused");
                assert_eq!(reader.feed(&[0; 64]).err(), wanted, "{kind:?} {len}: every call after");
            }
            assert_eq!(FrameHeader::decode(&header).err(), wanted, "{kind:?} {len}: the header");
        }
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        for code in [0, 8, 0xFF] {
            let header =
                FrameHeader { kind: code, flags: 0, source: 0, len: 0, seq: 0, turn: [0; 16] };
            let wanted = Some(FrameError::UnknownKind(code));
            assert_eq!(FrameReader::for_speech().feed(&header.encode()).err(), wanted, "{code}");
            assert_eq!(Frame::decode(&header, &[]).err(), wanted, "kind {code}, whole");
        }
    }

    #[test]
    fn flags_are_refused_on_every_kind_but_audio_and_outside_the_four_bits() {
        let ping = Frame { flags: AudioFlags::REPLAYED, ..Frame::new(FrameBody::Ping) };
        let wanted = Some(FrameError::BadFlags { kind: FrameKind::Ping, flags: 1 });
        assert_eq!(ping.encode().err(), wanted, "written");
        let header = FrameHeader { flags: 1, ..bare(FrameKind::Ping) };
        assert_eq!(FrameReader::for_speech().feed(&header.encode()).err(), wanted, "read");
        let audio = FrameHeader { flags: 0x10, ..bare(FrameKind::Audio) };
        assert_eq!(
            FrameReader::for_speech().feed(&audio.encode()).err(),
            Some(FrameError::BadFlags { kind: FrameKind::Audio, flags: 0x10 }),
            "audio, bit 4"
        );
    }

    #[test]
    fn a_kind_from_the_wrong_end_is_refused_at_its_header() {
        for kind in FrameKind::ALL {
            let (mut reader, wrong) = match kind.sender() {
                FrameSender::Core => (FrameReader::for_core_after_hello(), "the core's"),
                FrameSender::Speech => (FrameReader::for_speech(), "the speech process's"),
                FrameSender::Either => continue,
            };
            let header = bare(kind).encode();
            let wanted = Some(FrameError::WrongDirection { kind });
            assert_eq!(reader.feed(&header).err(), wanted, "{kind:?} is only {wrong}");
            assert_eq!(reader.needed(), FrameHeader::SIZE, "{kind:?}: no body reserved");
            assert_eq!(reader.feed(&[0; 8]).err(), wanted, "{kind:?}: and every call after");
        }
        let senders = FrameKind::ALL.map(FrameKind::sender);
        assert_eq!(
            senders,
            [
                FrameSender::Core,
                FrameSender::Core,
                FrameSender::Speech,
                FrameSender::Speech,
                FrameSender::Either,
                FrameSender::Either,
                FrameSender::Speech,
            ],
            "audio and commands from the core; events, PCM and the Hello from the speech process"
        );
    }

    #[test]
    fn the_core_reads_a_hello_first_and_only_once() {
        let ping = bare(FrameKind::Ping).encode();
        let hello_bytes = hello("token").encode().unwrap();
        assert_eq!(
            FrameReader::for_core().feed(&ping).err(),
            Some(FrameError::BeforeHello { kind: FrameKind::Ping }),
            "a ping before the Hello"
        );
        let twice = cat(&[&hello_bytes, &ping, &hello_bytes]);
        assert_eq!(
            read_all(FrameReader::for_core(), &twice, 9),
            Err(FrameError::SecondHello),
            "a Hello after the first"
        );
        assert_eq!(
            FrameReader::for_core_after_hello().feed(&hello_bytes).err(),
            Some(FrameError::SecondHello),
            "a Hello after the transport read the first"
        );
        let read = read_all(FrameReader::for_core(), &cat(&[&hello_bytes, &ping]), 4);
        assert_eq!(read.map(|frames| frames.len()), Ok(2), "a Hello, then a ping");
    }

    /// The parts, one after another.
    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    /// A PCM frame with `body` as its body, whatever it holds.
    fn pcm_with_body(body: &[u8]) -> Vec<u8> {
        let len = u32::try_from(body.len()).unwrap();
        cat(&[&FrameHeader { len, ..bare(FrameKind::Pcm) }.encode(), body])
    }

    #[test]
    fn a_pcm_frame_whose_header_terminator_is_missing_is_refused() {
        let read = |body: &[u8], chunk| {
            read_all(FrameReader::for_core_after_hello(), &pcm_with_body(body), chunk)
        };
        let json =
            br#"{"speak":"speak_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F","clause":0,"sample_rate":24000}"#;
        let frames = read(&cat(&[json, b"\n", &[1, 0, 2, 0]]), 5).unwrap();
        assert_eq!(
            frames[0].body,
            FrameBody::Pcm {
                header: PcmHeader { speak: speak(), clause: 0, sample_rate: 24_000 },
                samples: vec![1, 2]
            },
            "the newline ends the header"
        );
        let missing = cat(&[json, &[1, 0, 2, 0]]);
        assert_eq!(read(&missing, 64), Err(FrameError::MissingPcmTerminator), "no newline");
        let late = cat(&[json, &[b' '; PCM_HEADER_MAX], b"\n"]);
        assert_eq!(
            read(&late, 64),
            Err(FrameError::MissingPcmTerminator),
            "a newline past the header's maximum"
        );
        let odd = cat(&[json, b"\n", &[1, 0, 2]]);
        assert_eq!(read(&odd, 64), Err(FrameError::OddSamples), "odd");
        assert_eq!(
            read(b"{not json\n".as_slice(), 64),
            Err(FrameError::BadJson { kind: FrameKind::Pcm }),
            "a header that is not JSON"
        );
    }

    #[test]
    fn a_hello_over_256_bytes_is_refused() {
        // `{"token":""}` is 12 bytes, so a 244-byte token is the longest that fits.
        let longest = hello(&"t".repeat(244)).encode().unwrap();
        assert_eq!(longest.len(), FrameHeader::SIZE + 256, "exactly 256 bytes of body");
        assert_eq!(
            hello(&"t".repeat(245)).encode(),
            Err(FrameError::BadLength { kind: FrameKind::Hello, len: 257 }),
            "written"
        );
        let mut over = longest;
        over[4..8].copy_from_slice(&257_u32.to_le_bytes());
        over.push(b' ');
        assert_eq!(
            read_all(FrameReader::for_core(), &over, 1),
            Err(FrameError::BadLength { kind: FrameKind::Hello, len: 257 }),
            "read"
        );
    }

    #[test]
    fn a_failed_write_leaves_the_buffer_as_it_was() {
        let mut out = vec![9, 9];
        let ping = Frame { flags: AudioFlags::BREAK, ..Frame::new(FrameBody::Ping) };
        assert!(ping.encode_into(&mut out).is_err(), "flags on a ping");
        assert_eq!(out, [9, 9], "untouched");
    }

    #[test]
    fn a_hello_never_shows_its_token() {
        let frame = hello("s3cret-token");
        let shown = format!("{frame:?}");
        assert!(!shown.contains("s3cret"), "the frame: {shown}");
        let bytes = frame.encode().unwrap();
        let mut reader = FrameReader::for_core();
        let (taken, done) = reader.feed(&bytes[..bytes.len() - 1]).unwrap();
        assert_eq!((taken, done), (bytes.len() - 1, None), "all but the last byte");
        let shown = format!("{reader:?}");
        let token_bytes = format!("{:?}", &b"s3cret"[..]);
        let token_bytes = token_bytes.trim_matches(['[', ']']);
        assert!(!shown.contains(token_bytes), "the reader: {shown}");
        assert!(shown.contains("Hello"), "the reader names the kind it is reading: {shown}");
        let (_, done) = reader.feed(&bytes[bytes.len() - 1..]).unwrap();
        let secret = match done {
            Some(Frame { body: FrameBody::Hello(read), .. }) => read.token.expose().to_owned(),
            other => format!("{other:?}"),
        };
        assert_eq!(secret, "s3cret-token", "the last byte completes the Hello, token and all");
    }

    #[test]
    fn a_body_shorter_than_its_header_says_is_refused_whole() {
        let header = FrameHeader { len: 10, ..bare(FrameKind::Hello) };
        assert_eq!(
            Frame::decode(&header, b"{}"),
            Err(FrameError::BadLength { kind: FrameKind::Hello, len: 10 }),
            "two bytes where ten were promised"
        );
    }

    /// Short text of any characters; even fully escaped, 40 of them fit a `Hello`.
    fn text() -> impl Strategy<Value = String> {
        prop::collection::vec(any::<char>(), 0..40).prop_map(String::from_iter)
    }

    /// The header fields of a frame that carries no flags.
    fn plain(seq: u64, turn: [u8; 16], body: FrameBody) -> Frame {
        Frame { seq, turn, ..Frame::new(body) }
    }

    fn heartbeat() -> impl Strategy<Value = Frame> {
        (any::<bool>(), any::<u64>(), any::<[u8; 16]>()).prop_map(|(ping, seq, turn)| {
            plain(seq, turn, if ping { FrameBody::Ping } else { FrameBody::Pong })
        })
    }

    /// A frame the core may send, with any field its kind allows.
    fn core_frame() -> impl Strategy<Value = Frame> {
        let audio = (
            0_u8..16,
            any::<u16>(),
            any::<u64>(),
            any::<[u8; 16]>(),
            prop::collection::vec(any::<i16>(), AUDIO_FRAME_SAMPLES),
        )
            .prop_map(|(flags, source, seq, turn, samples)| Frame {
                flags: AudioFlags::from_bits(flags).unwrap(),
                source,
                seq,
                turn,
                body: FrameBody::Audio(samples.into_boxed_slice().try_into().unwrap()),
            });
        let command = (any::<u64>(), prop::collection::vec(text(), 0..4), any::<bool>()).prop_map(
            |(span, words, cancel)| {
                let cmd = if cancel {
                    SpeechCmd::Cancel { speak: speak() }
                } else {
                    SpeechCmd::SetHotwords { words }
                };
                let body = SpeechCommandFrame { ctx: TraceCtx::default(), span, cmd };
                Frame::new(FrameBody::Command(body))
            },
        );
        prop_oneof![audio, command, heartbeat()]
    }

    /// A frame the speech process may send after its `Hello`, with any field its kind allows.
    fn speech_frame() -> impl Strategy<Value = Frame> {
        let event = (any::<u64>(), any::<u64>(), any::<u16>(), any::<(bool, bool)>(), any::<u32>())
            .prop_map(|(seq, span, source, (started, replayed), chars)| {
                let event = if started {
                    VoiceEvent::SpeechStarted { turn: turn(), source, replayed }
                } else {
                    VoiceEvent::ClauseReady {
                        speak: speak(),
                        clause: source,
                        chars,
                        samples: chars,
                    }
                };
                let body = SpeechEventFrame { ctx: TraceCtx::default(), span, event };
                plain(seq, [0; 16], FrameBody::Event(body))
            });
        let pcm = (
            any::<u64>(),
            any::<u16>(),
            any::<u32>(),
            prop::collection::vec(any::<i16>(), 0..2048),
        )
            .prop_map(|(seq, clause, sample_rate, samples)| {
                let header = PcmHeader { speak: speak(), clause, sample_rate };
                plain(seq, [0; 16], FrameBody::Pcm { header, samples })
            });
        prop_oneof![event, pcm, heartbeat()]
    }

    /// The speech process's frames: its `Hello`, then the rest.
    fn speech_frames() -> impl Strategy<Value = Vec<Frame>> {
        (text(), prop::collection::vec(speech_frame(), 0..6)).prop_map(|(token, rest)| {
            let mut frames = vec![hello(&token)];
            frames.extend(rest);
            frames
        })
    }

    /// `bytes` cut at the given places, in order.
    fn cut<'a>(bytes: &'a [u8], cuts: &[prop::sample::Index]) -> Vec<&'a [u8]> {
        let mut at: Vec<usize> = cuts.iter().map(|cut| cut.index(bytes.len() + 1)).collect();
        at.sort_unstable();
        let mut pieces = Vec::new();
        let mut from = 0;
        for to in at.into_iter().chain([bytes.len()]) {
            pieces.push(&bytes[from..to]);
            from = to;
        }
        pieces
    }

    /// Bytes that start with a header of any kind, flags and a small length, so a body is read.
    fn noise() -> impl Strategy<Value = Vec<u8>> {
        (0_u8..9, 0_u8..17, 0_u32..1100, prop::collection::vec(any::<u8>(), 0..2400)).prop_map(
            |(kind, flags, len, rest)| {
                let header = FrameHeader { kind, flags, source: 0, len, seq: 0, turn: [0; 16] };
                cat(&[&header.encode(), &rest])
            },
        )
    }

    proptest! {
        #[test]
        fn any_frames_read_back_however_the_stream_is_cut(
            from_core in prop::collection::vec(core_frame(), 0..6),
            from_speech in speech_frames(),
            cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..12),
        ) {
            let core_bytes = stream(&from_core);
            let read = read_pieces(FrameReader::for_speech(), cut(&core_bytes, &cuts));
            prop_assert_eq!(read, Ok(from_core), "the core's frames");
            let speech_bytes = stream(&from_speech);
            let read = read_pieces(FrameReader::for_core(), cut(&speech_bytes, &cuts));
            prop_assert_eq!(read, Ok(from_speech), "the speech process's frames");
        }

        #[test]
        fn any_bytes_in_any_pieces_keep_the_reader_bounded(
            bytes in prop_oneof![noise(), prop::collection::vec(any::<u8>(), 0..2400)],
            cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..12),
            end in 0_usize..3,
        ) {
            let mut reader = [
                FrameReader::for_core(),
                FrameReader::for_core_after_hello(),
                FrameReader::for_speech(),
            ]
            .into_iter()
            .nth(end)
            .unwrap();
            let most = FrameHeader::SIZE.max(usize::try_from(PCM_BODY_MAX).unwrap());
            let mut failed = None;
            for mut piece in cut(&bytes, &cuts) {
                while !piece.is_empty() {
                    match reader.feed(piece) {
                        Ok((taken, _)) => {
                            prop_assert!(failed.is_none(), "no frame after an error");
                            prop_assert!(taken > 0 && taken <= piece.len(), "took {}", taken);
                            piece = &piece[taken..];
                        }
                        Err(error) => {
                            prop_assert!(failed.is_none_or(|first| first == error), "{}", error);
                            failed = Some(error);
                            piece = &piece[1..];
                        }
                    }
                    prop_assert!(reader.needed() <= most, "needs {}", reader.needed());
                }
            }
        }
    }
}
