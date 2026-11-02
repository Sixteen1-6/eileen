//! A room without a sound card (BUILD.md section 4.7): [`RoomMixer`] joins a speaker and a
//! microphone, so what Eileen plays comes back into what she hears, delayed and attenuated as
//! bleed from real speakers would, over a near-end talker and an optional music or speech-video
//! bed. Echo cancellation and barge-in are tested against it (WP4 acceptance 1).
//!
//! One task runs both halves on one paced timeline ([`Pace`]): each 10 ms buffer it fills the
//! playback, adds it to the echo line, then mixes and delivers the input. So the echo's delay is
//! exact to the sample whatever the delay, and a test sees the same samples on every run.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::pace::{self, Pace, Paced, Timeline};
use crate::stream::{
    AudioError, AudioSink, AudioSource, AudioStream, CaptureCallback, CaptureInfo, RenderCallback,
    RenderInfo, StreamFormat,
};
use crate::wav::WavClip;

/// The longest echo delay a room takes, far past a real room's tens of milliseconds.
const MAX_ECHO_DELAY: Duration = Duration::from_secs(1);

/// A room's setup.
#[derive(Debug, Clone)]
pub struct RoomConfig {
    /// The format of both the speaker and the microphone. Its rate must be a whole number of
    /// hundreds of hertz, so it splits into 10 ms buffers.
    pub format: StreamFormat,
    /// How fast the room runs.
    pub pace: Pace,
    /// From a sample leaving the speaker to its echo reaching the microphone, at most 1 s.
    pub echo_delay: Duration,
    /// The echo's linear gain (0.1 is 20 dB down), finite and not negative.
    pub echo_gain: f32,
    /// Someone talking in the room, from the room's start, then silence. Its format must be the
    /// room's.
    pub near: Option<WavClip>,
    /// Music or a video's speech playing in the room, looped. Its format must be the room's.
    pub bed: Option<Bed>,
}

/// Background audio in a room, looped.
#[derive(Debug, Clone)]
pub struct Bed {
    /// The audio.
    pub clip: WavClip,
    /// Its linear gain, finite and not negative.
    pub gain: f32,
}

/// A room joining a speaker, [`RoomMixer::sink`], to a microphone, [`RoomMixer::source`]. The
/// microphone hears the near-end talker, the bed and the echo of the speaker:
///
/// `input[t] = near[t] + bed.gain * bed[t mod bed's length] + echo_gain * playback[t - echo_delay]`
///
/// The room's timeline starts with its first stream and keeps running while both halves are
/// stopped, as a room does; the speaker plays silence while its half is stopped.
#[derive(Clone)]
pub struct RoomMixer {
    format: StreamFormat,
    room: Arc<Mutex<Room>>,
}

struct Room {
    timeline: Timeline,
    near: Option<WavClip>,
    bed: Option<Bed>,
    echo_gain: f32,
    /// The played samples still on their way to the microphone, `echo_delay` of them between
    /// buffers. Allocated once with room for one more buffer, so it never grows.
    echo: VecDeque<f32>,
    played: Vec<f32>,
    heard: Vec<f32>,
    capture: Option<CaptureCallback>,
    capture_starts: u64,
    render: Option<RenderCallback>,
    render_starts: u64,
}

impl RoomMixer {
    /// A room set up as `config` says.
    pub fn new(config: RoomConfig) -> Result<Self, AudioError> {
        let RoomConfig { format, pace, echo_delay, echo_gain, near, bed } = config;
        let timeline = Timeline::new(format, pace)?;
        if echo_delay > MAX_ECHO_DELAY {
            return Err(AudioError::Setup("a room's echo delay is at most 1 s"));
        }
        let gains = [Some(echo_gain), bed.as_ref().map(|bed| bed.gain)];
        if gains.into_iter().flatten().any(|gain| !gain.is_finite() || gain < 0.0) {
            return Err(AudioError::Setup("a room's gains are finite and not negative"));
        }
        let mut clips = near.iter().chain(bed.as_ref().map(|bed| &bed.clip));
        if let Some(clip) = clips.find(|clip| clip.format() != format) {
            return Err(AudioError::Unsupported(clip.format()));
        }
        let buffer_len = timeline.buffer_len();
        let delay_len = timeline.samples_in(timeline.frames_in(echo_delay));
        let mut echo = VecDeque::with_capacity(delay_len.saturating_add(buffer_len));
        echo.resize(delay_len, 0.0);
        let room = Room {
            timeline,
            near,
            bed,
            echo_gain,
            echo,
            played: vec![0.0; buffer_len],
            heard: vec![0.0; buffer_len],
            capture: None,
            capture_starts: 0,
            render: None,
            render_starts: 0,
        };
        Ok(Self { format, room: Arc::new(Mutex::new(room)) })
    }

    /// The room's microphone.
    #[must_use]
    pub fn source(&self) -> RoomSource {
        RoomSource { format: self.format, room: Arc::clone(&self.room) }
    }

    /// The room's speaker.
    #[must_use]
    pub fn sink(&self) -> RoomSink {
        RoomSink { format: self.format, room: Arc::clone(&self.room) }
    }
}

impl fmt::Debug for RoomMixer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoomMixer").field("format", &self.format).finish_non_exhaustive()
    }
}

/// A room's microphone: what the room hears, one capture stream at a time.
pub struct RoomSource {
    format: StreamFormat,
    room: Arc<Mutex<Room>>,
}

impl fmt::Debug for RoomSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoomSource").field("format", &self.format).finish_non_exhaustive()
    }
}

impl AudioSource for RoomSource {
    fn format(&self) -> StreamFormat {
        self.format
    }

    fn start(&mut self, on_buffer: CaptureCallback) -> Result<Box<dyn AudioStream>, AudioError> {
        let mut room = pace::lock(&self.room);
        if room.capture.is_some() {
            return Err(AudioError::Setup("the room's microphone is already capturing"));
        }
        let skipped = pace::run("audio.room", &self.room, &mut *room)?;
        room.pass(skipped);
        room.capture = Some(on_buffer);
        room.capture_starts += 1;
        let start = room.capture_starts;
        drop(room);
        Ok(Box::new(RoomStream { room: Arc::clone(&self.room), half: Half::Microphone, start }))
    }
}

/// A room's speaker: what Eileen plays into the room, one render stream at a time.
pub struct RoomSink {
    format: StreamFormat,
    room: Arc<Mutex<Room>>,
}

impl fmt::Debug for RoomSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoomSink").field("format", &self.format).finish_non_exhaustive()
    }
}

impl AudioSink for RoomSink {
    fn format(&self) -> StreamFormat {
        self.format
    }

    fn start(&mut self, fill: RenderCallback) -> Result<Box<dyn AudioStream>, AudioError> {
        let mut room = pace::lock(&self.room);
        if room.render.is_some() {
            return Err(AudioError::Setup("the room's speaker is already playing"));
        }
        let skipped = pace::run("audio.room", &self.room, &mut *room)?;
        room.pass(skipped);
        room.render = Some(fill);
        room.render_starts += 1;
        let start = room.render_starts;
        drop(room);
        Ok(Box::new(RoomStream { room: Arc::clone(&self.room), half: Half::Speaker, start }))
    }
}

impl Room {
    /// Lets `frames` of the room's time pass with no task running: the speaker played nothing,
    /// so the echo line moves on by that much silence.
    fn pass(&mut self, frames: u64) {
        let moved = self.timeline.samples_in(frames).min(self.echo.len());
        drop(self.echo.drain(..moved));
        self.echo.extend(std::iter::repeat_n(0.0, moved));
    }
}

impl Paced for Room {
    fn timeline(&mut self) -> &mut Timeline {
        &mut self.timeline
    }

    fn step(&mut self, position: u64) {
        let at = self.timeline.time_of(position);
        self.played.fill(0.0);
        if let Some(render) = self.render.as_mut() {
            render(&mut self.played, RenderInfo { plays_at: at });
        }
        self.echo.extend(self.played.iter().copied());

        let start = self.timeline.samples_in(position);
        let near = self.near.as_ref().map_or(&[][..], WavClip::samples);
        let near = near.get(start..).unwrap_or_default().iter().copied();
        for (heard, sample) in self.heard.iter_mut().zip(near.chain(std::iter::repeat(0.0))) {
            *heard = sample;
        }
        if let Some(bed) = self.bed.as_ref() {
            let samples = bed.clip.samples();
            let looped = samples.get(start % samples.len().max(1)..).unwrap_or_default();
            for (heard, sample) in
                self.heard.iter_mut().zip(looped.iter().chain(samples.iter().cycle()))
            {
                *heard += bed.gain * sample;
            }
        }
        let arrived = self.heard.len().min(self.echo.len());
        for (heard, echo) in self.heard.iter_mut().zip(self.echo.drain(..arrived)) {
            *heard += self.echo_gain * echo;
        }

        if let Some(capture) = self.capture.as_mut() {
            capture(&self.heard, CaptureInfo { captured_at: at });
        }
    }
}

/// Which half of a room a stream runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Half {
    Microphone,
    Speaker,
}

/// A running stream of one half of a room. The room's task stops when neither half runs.
struct RoomStream {
    room: Arc<Mutex<Room>>,
    half: Half,
    start: u64,
}

impl AudioStream for RoomStream {
    fn stop(self: Box<Self>) {}
}

impl Drop for RoomStream {
    fn drop(&mut self) {
        let mut room = pace::lock(&self.room);
        match self.half {
            Half::Microphone if room.capture_starts == self.start => room.capture = None,
            Half::Speaker if room.render_starts == self.start => room.render = None,
            Half::Microphone | Half::Speaker => {}
        }
        if room.capture.is_none() && room.render.is_none() {
            room.timeline.halt();
        }
    }
}

#[cfg(test)]
mod tests;
