//! Whole frames of the speech pipe as values, written to bytes and read back on top of
//! [`FrameHeader`]. The framing is in the parent module's doc.

use serde::Serialize;

use super::{
    AUDIO_FRAME_SAMPLES, AudioFlags, FrameError, FrameHeader, FrameKind, PCM_HEADER_MAX, PcmHeader,
    SpeechCommandFrame, SpeechEventFrame, SpeechHelloFrame,
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

    /// Appends the body's bytes to `out`.
    fn write(&self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        let kind = self.kind();
        match self {
            Self::Audio(samples) => write_samples(out, samples.as_slice()),
            Self::Command(frame) => write_json(out, frame, kind)?,
            Self::Event(frame) => write_json(out, frame, kind)?,
            Self::Pcm { header, samples } => {
                let start = out.len();
                write_json(out, header, kind)?;
                out.push(b'\n');
                if out.len() - start > PCM_HEADER_MAX {
                    return Err(FrameError::MissingPcmTerminator);
                }
                write_samples(out, samples);
            }
            Self::Ping | Self::Pong => {}
            Self::Hello(hello) => write_json(out, hello, kind)?,
        }
        Ok(())
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

fn write_samples(out: &mut Vec<u8>, samples: &[i16]) {
    out.reserve(samples.len() * 2);
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
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
        let start = out.len();
        out.extend_from_slice(&[0; FrameHeader::SIZE]);
        let written = self.body.write(out).and_then(|()| {
            let body = out.len() - start - FrameHeader::SIZE;
            let len = u32::try_from(body).unwrap_or(u32::MAX);
            let header = self.header(len);
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
/// It holds one frame at most: the header's 32 bytes, then a body buffer reserved only after
/// [`FrameHeader::decode`] has allowed the kind, flags and length, so a corrupt length never
/// allocates. After an error the stream is out of step and every later call returns the same
/// error: close the connection.
#[derive(Debug, Default)]
pub struct FrameReader {
    /// The header bytes read so far.
    header: [u8; FrameHeader::SIZE],
    /// How many of them.
    have: usize,
    /// The accepted header and the body read so far, once the header is whole.
    body: Option<(FrameHeader, usize, Vec<u8>)>,
    /// The error that stopped the stream.
    failed: Option<FrameError>,
}

impl FrameReader {
    /// A reader at the start of a stream.
    #[must_use]
    pub const fn new() -> Self {
        Self { header: [0; FrameHeader::SIZE], have: 0, body: None, failed: None }
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
            let (_, header) = FrameHeader::decode(&self.header)?;
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
    use super::{Frame, FrameBody, FrameReader};
    use crate::ids::TraceCtx;
    use crate::speech::{
        AUDIO_FRAME_BYTES, AUDIO_FRAME_SAMPLES, AudioFlags, FrameError, FrameHeader, FrameKind,
        HELLO_BODY_MAX, JSON_BODY_MAX, PCM_BODY_MAX, PCM_HEADER_MAX, PcmHeader, SpeechCmd,
        SpeechCommandFrame, SpeechEventFrame, SpeechHelloFrame, VoiceEvent,
    };

    fn speak() -> crate::ids::SpeakId {
        "speak_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap()
    }

    fn turn() -> crate::ids::TurnId {
        "turn_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap()
    }

    /// One frame of every kind, with the header fields D81 gives each direction.
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
            Frame::new(FrameBody::Hello(SpeechHelloFrame { token: "a1b2".repeat(16) })),
        ];
        let kinds: Vec<_> = frames.iter().map(|frame| frame.body.kind()).collect();
        assert_eq!(kinds, FrameKind::ALL, "one frame of every kind, in wire order");
        frames
    }

    /// Reads every frame from `stream`, giving the reader `chunk` bytes at a time.
    fn read_all(stream: &[u8], chunk: usize) -> Result<Vec<Frame>, FrameError> {
        let mut reader = FrameReader::new();
        let mut frames = Vec::new();
        for mut piece in stream.chunks(chunk) {
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

    #[test]
    fn every_kind_round_trips_whole_and_a_byte_at_a_time() {
        let frames = one_of_each();
        let mut stream = Vec::new();
        for frame in &frames {
            frame.encode_into(&mut stream).unwrap();
        }
        for chunk in [stream.len(), 1, 7, FrameHeader::SIZE] {
            assert_eq!(read_all(&stream, chunk).unwrap(), frames, "chunks of {chunk}");
        }
        let audio = frames[0].encode().unwrap();
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
    fn an_over_length_len_is_refused_before_any_body_byte_is_read() {
        let over = |kind: FrameKind, len: u32| FrameHeader {
            kind: kind.code(),
            flags: 0,
            source: 0,
            len,
            seq: 0,
            turn: [0; 16],
        };
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
            let mut reader = FrameReader::new();
            let header = over(kind, len).encode();
            let wanted = Some(FrameError::BadLength { kind, len });
            assert_eq!(reader.feed(&header).err(), wanted, "{kind:?} {len}: refused at the header");
            assert_eq!(reader.feed(&[0; 64]).err(), wanted, "{kind:?} {len}: and every call after");
            assert_eq!(FrameHeader::decode(&header).err(), wanted, "{kind:?} {len}: the header");
        }
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        for code in [0, 8, 0xFF] {
            let header =
                FrameHeader { kind: code, flags: 0, source: 0, len: 0, seq: 0, turn: [0; 16] };
            let wanted = Some(FrameError::UnknownKind(code));
            assert_eq!(FrameReader::new().feed(&header.encode()).err(), wanted, "kind {code}");
            assert_eq!(Frame::decode(&header, &[]).err(), wanted, "kind {code}, whole");
        }
    }

    #[test]
    fn flags_are_refused_on_every_kind_but_audio_and_outside_the_four_bits() {
        let ping = Frame { flags: AudioFlags::REPLAYED, ..Frame::new(FrameBody::Ping) };
        let wanted = Some(FrameError::BadFlags { kind: FrameKind::Ping, flags: 1 });
        assert_eq!(ping.encode().err(), wanted, "written");
        let header = FrameHeader {
            kind: FrameKind::Ping.code(),
            flags: 1,
            source: 0,
            len: 0,
            seq: 0,
            turn: [0; 16],
        };
        assert_eq!(FrameReader::new().feed(&header.encode()).err(), wanted, "read");
        let audio = FrameHeader { kind: 1, flags: 0x10, len: AUDIO_FRAME_BYTES, ..header };
        assert_eq!(
            FrameReader::new().feed(&audio.encode()).err(),
            Some(FrameError::BadFlags { kind: FrameKind::Audio, flags: 0x10 }),
            "audio, bit 4"
        );
    }

    /// The parts, one after another.
    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    /// A PCM frame with `body` as its body, whatever it holds.
    fn pcm_with_body(body: &[u8]) -> Vec<u8> {
        let header = FrameHeader {
            kind: FrameKind::Pcm.code(),
            flags: 0,
            source: 0,
            len: u32::try_from(body.len()).unwrap(),
            seq: 0,
            turn: [0; 16],
        };
        cat(&[&header.encode(), body])
    }

    #[test]
    fn a_pcm_frame_whose_header_terminator_is_missing_is_refused() {
        let json =
            br#"{"speak":"speak_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F","clause":0,"sample_rate":24000}"#;
        let good = cat(&[json, b"\n", &[1, 0, 2, 0]]);
        let frames = read_all(&pcm_with_body(&good), 5).unwrap();
        assert_eq!(
            frames[0].body,
            FrameBody::Pcm {
                header: PcmHeader { speak: speak(), clause: 0, sample_rate: 24_000 },
                samples: vec![1, 2]
            },
            "the newline ends the header"
        );
        let missing = cat(&[json, &[1, 0, 2, 0]]);
        assert_eq!(
            read_all(&pcm_with_body(&missing), 64),
            Err(FrameError::MissingPcmTerminator),
            "no newline"
        );
        let late = cat(&[json, &[b' '; PCM_HEADER_MAX], b"\n"]);
        assert_eq!(
            read_all(&pcm_with_body(&late), 64),
            Err(FrameError::MissingPcmTerminator),
            "a newline past the header's maximum"
        );
        let odd = cat(&[json, b"\n", &[1, 0, 2]]);
        assert_eq!(read_all(&pcm_with_body(&odd), 64), Err(FrameError::OddSamples), "odd");
        let not_json = cat(&[b"{not json", b"\n"]);
        assert_eq!(
            read_all(&pcm_with_body(&not_json), 64),
            Err(FrameError::BadJson { kind: FrameKind::Pcm }),
            "a header that is not JSON"
        );
    }

    #[test]
    fn a_hello_over_256_bytes_is_refused() {
        let hello = |token: String| Frame::new(FrameBody::Hello(SpeechHelloFrame { token }));
        // `{"token":""}` is 12 bytes, so a 244-byte token is the longest that fits.
        let longest = hello("t".repeat(244)).encode().unwrap();
        assert_eq!(longest.len(), FrameHeader::SIZE + 256, "exactly 256 bytes of body");
        assert_eq!(
            hello("t".repeat(245)).encode(),
            Err(FrameError::BadLength { kind: FrameKind::Hello, len: 257 }),
            "written"
        );
        let mut over = longest;
        over[4..8].copy_from_slice(&257_u32.to_le_bytes());
        over.push(b' ');
        assert_eq!(
            read_all(&over, 1),
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
        let hello = SpeechHelloFrame { token: "s3cret-token".to_owned() };
        let shown = format!("{:?}", Frame::new(FrameBody::Hello(hello)));
        assert!(!shown.contains("s3cret"), "{shown}");
    }

    #[test]
    fn a_body_shorter_than_its_header_says_is_refused_whole() {
        let header = FrameHeader {
            kind: FrameKind::Hello.code(),
            flags: 0,
            source: 0,
            len: 10,
            seq: 0,
            turn: [0; 16],
        };
        assert_eq!(
            Frame::decode(&header, b"{}"),
            Err(FrameError::BadLength { kind: FrameKind::Hello, len: 10 }),
            "two bytes where ten were promised"
        );
    }
}
