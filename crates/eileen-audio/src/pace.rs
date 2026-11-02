//! Paced virtual time for the test sources and sinks (BUILD.md section 4.7, "End to end without a
//! sound card"). [`WavSource`](crate::WavSource), [`CaptureSink`](crate::CaptureSink) and
//! [`RoomMixer`](crate::RoomMixer) each run one task that calls its callbacks once per 10 ms
//! buffer, due when the tokio clock reaches the buffer's place on the device's timeline. In a run
//! that is real time; under paused tokio time it is virtual, so a test moves it only by waiting
//! and no test sleeps for real. A buffer's time on the device's own clock comes from its sample
//! position, never from the clock that paced it, so the times stay exact however late a busy
//! runtime runs the task.
//!
//! The timeline is anchored when the device's first stream starts and keeps running while it is
//! stopped, as a room does: a stream started again takes up at the buffer due next, so what a
//! WAV file holds for the stopped stretch is never captured, as a muted microphone hears nothing.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use eileen_stdx::num;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::stream::{AudioError, StreamFormat};

/// Buffers per second of audio: every test source and sink works in 10 ms buffers, as a WASAPI
/// endpoint's default period is.
const BUFFERS_PER_SECOND: u32 = 100;

/// Nanoseconds per second.
const NANOS: u128 = 1_000_000_000;

/// How fast a test source or sink runs its buffers against the tokio clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Pace {
    /// A buffer per buffer's length: real time in a run, and virtual time under paused tokio
    /// time, where tests run.
    #[default]
    RealTime,
    /// This many buffers per buffer's length, for a run on the real clock that wants its audio
    /// sooner. The buffers' own times still count real audio time.
    Faster(NonZeroU32),
}

impl Pace {
    const fn speed(self) -> u32 {
        match self {
            Self::RealTime => 1,
            Self::Faster(times) => times.get(),
        }
    }
}

/// A device's place in time: where its timeline started on the tokio clock, the next buffer it
/// runs, and the task that runs it while a stream is live.
#[derive(Debug)]
pub(crate) struct Timeline {
    rate_hz: u32,
    channels: u16,
    buffer_frames: u64,
    speed: u32,
    anchor: Option<Instant>,
    next: u64,
    /// The live task's number, so a task that outlived its stream returns at its next wake.
    live: Option<u64>,
    tasks: u64,
    task: Option<JoinHandle<()>>,
}

impl Timeline {
    /// A timeline for streams of `format`, which must hold whole 10 ms buffers.
    pub(crate) fn new(format: StreamFormat, pace: Pace) -> Result<Self, AudioError> {
        let StreamFormat { sample_rate_hz, channels } = format;
        if channels == 0
            || sample_rate_hz == 0
            || !sample_rate_hz.is_multiple_of(BUFFERS_PER_SECOND)
        {
            return Err(AudioError::Unsupported(format));
        }
        Ok(Self {
            rate_hz: sample_rate_hz,
            channels,
            buffer_frames: u64::from(sample_rate_hz / BUFFERS_PER_SECOND),
            speed: pace.speed(),
            anchor: None,
            next: 0,
            live: None,
            tasks: 0,
            task: None,
        })
    }

    /// Frames in one buffer.
    pub(crate) const fn buffer_frames(&self) -> u64 {
        self.buffer_frames
    }

    /// Interleaved samples in one buffer.
    pub(crate) fn buffer_len(&self) -> usize {
        num::saturating_usize_from_u64(self.buffer_frames.saturating_mul(u64::from(self.channels)))
    }

    /// Interleaved samples in `frames` frames.
    pub(crate) fn samples_in(&self, frames: u64) -> usize {
        num::saturating_usize_from_u64(frames.saturating_mul(u64::from(self.channels)))
    }

    /// The time of the frame at `position` on the device's own clock.
    pub(crate) fn time_of(&self, position: u64) -> Duration {
        let nanos = u128::from(position) * NANOS / u128::from(self.rate_hz);
        Duration::from_nanos(num::saturating_u64_from_u128(nanos))
    }

    /// The frames in `duration`, to the nearest frame.
    pub(crate) fn frames_in(&self, duration: Duration) -> u64 {
        let scaled = duration.as_nanos() * u128::from(self.rate_hz);
        num::saturating_u64_from_u128((scaled + NANOS / 2) / NANOS)
    }

    /// Whether a task runs the timeline.
    pub(crate) const fn is_live(&self) -> bool {
        self.live.is_some()
    }

    /// Stops the timeline's task, which runs no buffer after this returns: it runs each buffer
    /// under the device's lock, which the caller holds.
    pub(crate) fn halt(&mut self) {
        self.live = None;
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    /// When the buffer at `position` is due on the tokio clock.
    fn due(&self, anchor: Instant, position: u64) -> Instant {
        let offset = self.time_of(position).checked_div(self.speed).unwrap_or_default();
        // Past the tokio clock's range, which no run reaches, the buffer is due at once.
        anchor.checked_add(offset).unwrap_or(anchor)
    }

    /// Takes up the timeline at `now` for a new task: the next buffer is the first one due at or
    /// after `now` that has not run. Returns the task's number and the frames skipped since the
    /// last buffer that ran.
    fn begin(&mut self, now: Instant) -> (u64, u64) {
        let anchor = *self.anchor.get_or_insert(now);
        let elapsed = now.saturating_duration_since(anchor).as_nanos();
        let frames = elapsed * u128::from(self.speed) * u128::from(self.rate_hz);
        let buffers = frames.div_ceil(NANOS * u128::from(self.buffer_frames));
        let due = num::saturating_u64_from_u128(buffers * u128::from(self.buffer_frames));
        let start = due.max(self.next);
        let skipped = start - self.next;
        self.next = start;
        self.tasks += 1;
        self.live = Some(self.tasks);
        (self.tasks, skipped)
    }

    /// When the next buffer of task `task` is due, or `None` once the task is no longer live.
    fn next_due(&self, task: u64) -> Option<Instant> {
        let anchor = self.anchor?;
        (self.live == Some(task)).then(|| self.due(anchor, self.next))
    }

    /// The next buffer's position for task `task`, moving past it, or `None` once the task is no
    /// longer live.
    fn take_next(&mut self, task: u64) -> Option<u64> {
        (self.live == Some(task)).then(|| {
            let position = self.next;
            self.next = position.saturating_add(self.buffer_frames);
            position
        })
    }
}

/// A device whose task runs buffers on its [`Timeline`].
pub(crate) trait Paced: Send + 'static {
    /// The device's timeline.
    fn timeline(&mut self) -> &mut Timeline;

    /// Runs the buffer that starts at frame `position`, calling the device's callbacks.
    fn step(&mut self, position: u64);
}

/// Locks a device. A callback that panicked poisons nothing worth refusing: the state stays
/// consistent between buffers.
pub(crate) fn lock<T>(device: &Mutex<T>) -> MutexGuard<'_, T> {
    device.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts the task of `device`, which the caller has locked as `locked`, unless one runs.
/// Returns the frames the timeline skipped while no task ran, so the device can account for
/// them.
pub(crate) fn run<D: Paced>(
    name: &'static str,
    device: &Arc<Mutex<D>>,
    locked: &mut D,
) -> Result<u64, AudioError> {
    if locked.timeline().is_live() {
        return Ok(0);
    }
    if tokio::runtime::Handle::try_current().is_err() {
        return Err(AudioError::Setup("a test source or sink needs a tokio runtime"));
    }
    let timeline = locked.timeline();
    let (task, skipped) = timeline.begin(Instant::now());
    timeline.task = Some(eileen_rt::task::spawn(name, drive(Arc::clone(device), task)));
    Ok(skipped)
}

/// Runs each buffer of `device` when it is due, until task `task` is no longer live.
async fn drive<D: Paced>(device: Arc<Mutex<D>>, task: u64) {
    loop {
        let Some(due) = lock(&device).timeline().next_due(task) else { return };
        tokio::time::sleep_until(due).await;
        let mut locked = lock(&device);
        let Some(position) = locked.timeline().take_next(task) else { return };
        locked.step(position);
    }
}
