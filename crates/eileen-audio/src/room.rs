//! A room without a sound card (BUILD.md section 4.7): [`RoomMixer`] joins a speaker and a
//! microphone, so what Eileen plays comes back into what she hears, delayed and attenuated as
//! bleed from real speakers would, over a near-end talker and an optional music or speech-video
//! bed. Its loopback is what the speakers play, Eileen's voice and a bed the PC plays together,
//! as the system loopback that section 2.8 makes the echo canceller's far-end reference. Echo
//! cancellation and barge-in are tested against it (WP4 acceptance 1).
//!
//! One task runs all three streams on one paced timeline ([`Pace`]): each 10 ms buffer it fills
//! the playback, mixes in a bed the speakers play, delivers that mix to the loopback and adds it
//! to the echo line, then mixes and delivers the input. So the echo's delay is exact to the
//! sample whatever the delay, the three streams carry the same times, and a test sees the same
//! samples on every run.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::pace::{self, Pace, Paced, Timeline};
use crate::stream::{
    AudioError, AudioSink, AudioSource, AudioStream, CaptureCallback, CaptureInfo, RenderCallback,
    RenderInfo, SetupError, StreamFormat,
};
use crate::wav::WavClip;

/// The longest echo delay a room takes, far past a real room's tens of milliseconds.
const MAX_ECHO_DELAY: Duration = Duration::from_secs(1);

/// A room's setup.
#[derive(Debug, Clone)]
pub struct RoomConfig {
    /// The format of the speaker, the microphone and the loopback. Its rate must be a whole
    /// number of hundreds of hertz, so it splits into 10 ms buffers.
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
    /// Music or a video's speech playing, looped. Its format must be the room's.
    pub bed: Option<Bed>,
}

/// Background audio, looped from the room's start.
#[derive(Debug, Clone)]
pub struct Bed {
    /// The audio.
    pub clip: WavClip,
    /// Its linear gain, finite and not negative.
    pub gain: f32,
    /// Where it plays.
    pub from: BedSource,
}

/// Where a [`Bed`] plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BedSource {
    /// The PC plays it, as a video or music beside Eileen: it is in the loopback with her voice,
    /// and reaches the microphone as their echo does.
    #[default]
    Speakers,
    /// Something else in the room plays it, as a television: the microphone hears it as it is,
    /// and the loopback never carries it.
    Room,
}

/// A room joining a speaker, [`RoomMixer::sink`], to a microphone, [`RoomMixer::source`], with
/// the speaker's loopback, [`RoomMixer::loopback`]. The speaker plays Eileen's playback and a
/// bed from the speakers, and the loopback delivers exactly that; the microphone hears the
/// near-end talker, a bed in the room and the speaker's echo:
///
/// - `speaker[t] = playback[t] + gain * bed[t mod bed's length]`, for a bed from the speakers
/// - `loopback[t] = speaker[t]`
/// - `input[t] = near[t] + gain * bed[t mod bed's length] + echo_gain * speaker[t - echo_delay]`,
///   the bed's term only for a bed in the room
///
/// The room's timeline starts with its first stream and keeps running while every stream is
/// stopped, as a room does: the playback is silent while the speaker's stream is stopped, and
/// the bed plays on.
#[derive(Clone)]
pub struct RoomMixer {
    format: StreamFormat,
    room: Arc<Mutex<Room>>,
}

/// One stream's callback while it runs, and which start it belongs to, so a stopped stream's
/// handle never stops a later one.
struct Slot<C> {
    callback: Option<C>,
    starts: u64,
}

impl<C> Slot<C> {
    const fn new() -> Self {
        Self { callback: None, starts: 0 }
    }

    /// Takes `callback` for a new start, returning the start's number.
    fn fill(&mut self, callback: C) -> u64 {
        self.callback = Some(callback);
        self.starts += 1;
        self.starts
    }

    /// Ends start `start`, unless a later one took its place.
    fn clear(&mut self, start: u64) {
        if self.starts == start {
            self.callback = None;
        }
    }
}

struct Room {
    timeline: Timeline,
    near: Option<WavClip>,
    bed: Option<Bed>,
    echo_gain: f32,
    /// What the speaker played that is still on its way to the microphone, `echo_delay` of it
    /// between buffers. Allocated once with room for one more buffer, so it never grows.
    echo: VecDeque<f32>,
    speaker: Vec<f32>,
    heard: Vec<f32>,
    microphone: Slot<CaptureCallback>,
    loopback: Slot<CaptureCallback>,
    render: Slot<RenderCallback>,
}

impl RoomMixer {
    /// A room set up as `config` says.
    pub fn new(config: RoomConfig) -> Result<Self, AudioError> {
        let RoomConfig { format, pace, echo_delay, echo_gain, near, bed } = config;
        let timeline = Timeline::new(format, pace)?;
        if echo_delay > MAX_ECHO_DELAY {
            return Err(AudioError::Setup(SetupError::EchoTooLong));
        }
        let gains = [Some(echo_gain), bed.as_ref().map(|bed| bed.gain)];
        if gains.into_iter().flatten().any(|gain| !gain.is_finite() || gain < 0.0) {
            return Err(AudioError::Setup(SetupError::BadGain));
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
            speaker: vec![0.0; buffer_len],
            heard: vec![0.0; buffer_len],
            microphone: Slot::new(),
            loopback: Slot::new(),
            render: Slot::new(),
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

    /// The speaker's loopback: what the speaker plays, captured at the time it plays.
    #[must_use]
    pub fn loopback(&self) -> RoomLoopback {
        RoomLoopback { format: self.format, room: Arc::clone(&self.room) }
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
        start(&self.room, Stream::Microphone, |room| &mut room.microphone, on_buffer)
    }
}

/// A room's loopback: what its speaker plays, one capture stream at a time. Each buffer comes
/// before the microphone's buffer of the same time.
pub struct RoomLoopback {
    format: StreamFormat,
    room: Arc<Mutex<Room>>,
}

impl fmt::Debug for RoomLoopback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoomLoopback").field("format", &self.format).finish_non_exhaustive()
    }
}

impl AudioSource for RoomLoopback {
    fn format(&self) -> StreamFormat {
        self.format
    }

    fn start(&mut self, on_buffer: CaptureCallback) -> Result<Box<dyn AudioStream>, AudioError> {
        start(&self.room, Stream::Loopback, |room| &mut room.loopback, on_buffer)
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
        start(&self.room, Stream::Speaker, |room| &mut room.render, fill)
    }
}

/// Starts `stream` of a room with `callback`, in the slot `slot` picks, and the room's task if
/// no other stream runs it.
fn start<C>(
    shared: &Arc<Mutex<Room>>,
    stream: Stream,
    slot: fn(&mut Room) -> &mut Slot<C>,
    callback: C,
) -> Result<Box<dyn AudioStream>, AudioError> {
    let mut room = pace::lock(shared);
    if slot(&mut room).callback.is_some() {
        return Err(AudioError::Setup(SetupError::AlreadyRunning));
    }
    let skipped = pace::run("audio.room", shared, &mut *room)?;
    room.pass(skipped);
    let start = slot(&mut room).fill(callback);
    drop(room);
    Ok(Box::new(RoomStream { room: Arc::clone(shared), stream, start }))
}

impl Room {
    /// Lets the `frames` before the next buffer pass with no task running: Eileen played
    /// nothing, so the echo line moves on by that much of what the speakers played, the bed
    /// from the speakers or silence.
    fn pass(&mut self, frames: u64) {
        let moved = self.timeline.samples_in(frames).min(self.echo.len());
        drop(self.echo.drain(..moved));
        let end = self.timeline.samples_in(self.timeline.next_position());
        let from = end.saturating_sub(moved);
        let bed = self.bed.as_ref().filter(|bed| bed.from == BedSource::Speakers);
        self.echo.extend((from..end).map(|n| bed.map_or(0.0, |bed| bed_sample(bed, n))));
    }
}

/// The bed's sample at interleaved sample `n` of the room's timeline, its gain applied.
fn bed_sample(bed: &Bed, n: usize) -> f32 {
    let samples = bed.clip.samples();
    let looped = n.checked_rem(samples.len()).and_then(|at| samples.get(at));
    bed.gain * looped.copied().unwrap_or(0.0)
}

/// Adds the bed into `out`, which starts at interleaved sample `start` of the room's timeline.
fn add_bed(out: &mut [f32], bed: &Bed, start: usize) {
    for (n, sample) in out.iter_mut().enumerate() {
        *sample += bed_sample(bed, start.saturating_add(n));
    }
}

impl Paced for Room {
    fn timeline(&mut self) -> &mut Timeline {
        &mut self.timeline
    }

    fn step(&mut self, position: u64) {
        let at = self.timeline.time_of(position);
        let start = self.timeline.samples_in(position);
        self.speaker.fill(0.0);
        if let Some(render) = self.render.callback.as_mut() {
            render(&mut self.speaker, RenderInfo { plays_at: at });
        }
        if let Some(bed) = self.bed.as_ref().filter(|bed| bed.from == BedSource::Speakers) {
            add_bed(&mut self.speaker, bed, start);
        }
        if let Some(loopback) = self.loopback.callback.as_mut() {
            loopback(&self.speaker, CaptureInfo { captured_at: at });
        }
        self.echo.extend(self.speaker.iter().copied());

        let near = self.near.as_ref().map_or(&[][..], WavClip::samples);
        let near = near.get(start..).unwrap_or_default().iter().copied();
        for (heard, sample) in self.heard.iter_mut().zip(near.chain(std::iter::repeat(0.0))) {
            *heard = sample;
        }
        if let Some(bed) = self.bed.as_ref().filter(|bed| bed.from == BedSource::Room) {
            add_bed(&mut self.heard, bed, start);
        }
        let arrived = self.heard.len().min(self.echo.len());
        for (heard, echo) in self.heard.iter_mut().zip(self.echo.drain(..arrived)) {
            *heard += self.echo_gain * echo;
        }

        if let Some(microphone) = self.microphone.callback.as_mut() {
            microphone(&self.heard, CaptureInfo { captured_at: at });
        }
    }
}

/// Which stream of a room a handle runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Microphone,
    Loopback,
    Speaker,
}

/// A running stream of a room. The room's task stops when none of its streams runs.
struct RoomStream {
    room: Arc<Mutex<Room>>,
    stream: Stream,
    start: u64,
}

impl AudioStream for RoomStream {
    fn stop(self: Box<Self>) {}
}

impl Drop for RoomStream {
    fn drop(&mut self) {
        let mut room = pace::lock(&self.room);
        match self.stream {
            Stream::Microphone => room.microphone.clear(self.start),
            Stream::Loopback => room.loopback.clear(self.start),
            Stream::Speaker => room.render.clear(self.start),
        }
        let idle = room.microphone.callback.is_none()
            && room.loopback.callback.is_none()
            && room.render.callback.is_none();
        if idle {
            room.timeline.halt();
        }
    }
}

#[cfg(test)]
mod tests;
