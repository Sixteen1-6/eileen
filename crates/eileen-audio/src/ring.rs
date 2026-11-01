//! The speech ring (BUILD.md section 6.6, decision D81): every 512-sample 16 kHz frame the
//! microphone gives, from the capture callback to the speech pipe, kept until the speech process
//! has confirmed it.
//!
//! Two halves. The capture side, [`CaptureWriter`], numbers each frame and hands it over an SPSC
//! ring (`rtrb`) from the audio thread, allocating, freeing, blocking and logging nothing. The
//! reader side, [`SpeechRing`], moves the frames into a replay store that holds every frame the
//! speech process has not confirmed, and gives the component the next frame to write with its
//! flags. The store is bounded (180 s, [`STORE_FRAMES`]) and allocated once.
//!
//! The rules, from D81 and section 6.6:
//!
//! - A frame's `seq` counts the frames captured since the core started, from 1. It never restarts
//!   with a connection or a new capture stream, and a frame the handoff lost still used its
//!   number, so a gap in the numbers is frames lost.
//! - Frames up to the `seq` of a `Pong` with no turn open, or of the frame that carried an
//!   `EndOfTurn`, are done and leave the store. A confirmation never reaches past the newest frame
//!   written on the connection, since the speech process can only have read what it was sent.
//! - On a new connection the replay starts at the oldest frame not confirmed.
//! - On overrun the store drops its oldest frames. The first frame written after any that were
//!   lost unwritten carries the break flag, which follows from the numbers: a frame carries it
//!   when its `seq` is not one more than the frame written before it on the connection (or, first
//!   on a connection, than the newest confirmed frame).
//! - A mute (`SetCapture(false)`) stops the stream, then [`SpeechRing::capture_stopped`] empties
//!   the store and drops the handoff, so no frame captured before a mute is written after it. The
//!   first frame captured after it is marked in the store, so it carries the break flag on every
//!   connection that writes it, a replay included, though no number was skipped.
//! - A frame written more than 250 ms after capture carries the replayed flag. Its age is measured
//!   on the source's clock, so the ring reads no clock: the capture side's now is the newest frame
//!   pulled from the handoff plus 32 ms for each number taken since, so frames the handoff lost
//!   still count as time passing. Frames of an earlier stream, whose clock the newest stream's may
//!   not share, count as replayed.
//!
//! Only one capture stream runs at a time, and the component calls [`SpeechRing::open_capture`]
//! and [`SpeechRing::capture_stopped`] only once the earlier stream has stopped, so its callback,
//! and with it its writer, is never called again. The two atomics both sides share, the next
//! `seq` and the count of frames the handoff lost, come from loom under `--cfg loom`, so the loom
//! tests explore every order of them; rtrb's own atomics are outside loom's view and rely on
//! rtrb's tests, and a test on real threads drives the handoff through many wraps.

use std::collections::VecDeque;
use std::time::Duration;

use eileen_contracts::speech::{AUDIO_FRAME_SAMPLES, AudioFlags, Frame, FrameBody, VoiceEvent};
#[cfg(loom)]
use loom::sync::Arc;
#[cfg(loom)]
use loom::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(loom))]
use std::sync::Arc;
#[cfg(not(loom))]
use std::sync::atomic::{AtomicU64, Ordering};

/// The replay store's cap: 180 s of 32 ms frames, about 5.8 MB, which covers the core's 120 s load
/// deadline and a 60 s restart backoff step (BUILD.md section 6.6).
pub(crate) const STORE_FRAMES: usize = 5_625;

/// The handoff's frames: about 2 s, room for the reader task to fall behind for a while.
pub(crate) const HANDOFF_FRAMES: usize = 64;

/// A frame written more than this long after capture carries the replayed flag (D81).
pub(crate) const REPLAYED_AFTER: Duration = Duration::from_millis(250);

/// The audio one frame holds: 512 samples at 16 kHz. Each `seq` is one frame, so each number
/// taken is this much capture time passed.
pub(crate) const FRAME_TIME: Duration = Duration::from_millis(32);

/// What was true while a frame was captured, which it carries to the speech process.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CaptureFlags {
    /// Eileen's own playback was sounding.
    pub(crate) playback: bool,
    /// Other audio was playing.
    pub(crate) other_audio: bool,
}

impl CaptureFlags {
    /// The wire flags these set.
    const fn audio_flags(self) -> AudioFlags {
        let mut flags = AudioFlags::NONE;
        if self.playback {
            flags = flags.union(AudioFlags::PLAYBACK);
        }
        if self.other_audio {
            flags = flags.union(AudioFlags::OTHER_AUDIO);
        }
        flags
    }
}

/// A frame as the capture side numbered it.
#[derive(Debug, Clone, Copy)]
struct RingFrame {
    seq: u64,
    /// The capture stream it came from, whose clock `captured_at` is on.
    stream: u64,
    captured_at: Duration,
    flags: CaptureFlags,
    /// The first frame pulled after capture stopped, so frames are missing before it. The reader
    /// sets it; the writer always leaves it false.
    break_before: bool,
    samples: [i16; AUDIO_FRAME_SAMPLES],
}

/// The newest frame pulled from the handoff, which a frame's age is measured against.
#[derive(Debug, Clone, Copy)]
struct Newest {
    seq: u64,
    stream: u64,
    captured_at: Duration,
}

/// What both sides share.
#[derive(Debug)]
struct Shared {
    /// The next frame's `seq`. One writer at a time takes numbers from it, and a writer for a
    /// later stream goes on from where the last one stopped.
    next_seq: AtomicU64,
    /// Frames the capture side lost to a full handoff since the reader last took the count.
    lost: AtomicU64,
}

/// The capture side: the audio callback's end of the handoff, one per capture stream.
pub(crate) struct CaptureWriter {
    handoff: rtrb::Producer<RingFrame>,
    shared: Arc<Shared>,
    stream: u64,
}

impl CaptureWriter {
    /// Numbers one captured frame and hands it to the reader. Returns `false` when the handoff
    /// was full: the frame is lost, but its number is used, so the reader sees the gap. Called on
    /// the audio thread; it allocates, frees, blocks and logs nothing.
    pub(crate) fn push(
        &mut self,
        samples: &[i16; AUDIO_FRAME_SAMPLES],
        captured_at: Duration,
        flags: CaptureFlags,
    ) -> bool {
        // Relaxed: only one writer runs at a time, and a read-modify-write always sees the
        // newest value, so no two frames share a number. The frame itself reaches the reader
        // through rtrb's release and acquire.
        let seq = self.shared.next_seq.fetch_add(1, Ordering::Relaxed);
        let frame = RingFrame {
            seq,
            stream: self.stream,
            captured_at,
            flags,
            break_before: false,
            samples: *samples,
        };
        if self.handoff.push(frame).is_ok() {
            true
        } else {
            self.shared.lost.fetch_add(1, Ordering::Relaxed);
            false
        }
    }
}

/// A frame to write next, borrowed from the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Outgoing<'a> {
    /// Its number among the frames captured since the core started.
    pub(crate) seq: u64,
    /// When it was captured, on the source's clock.
    pub(crate) captured_at: Duration,
    /// Its flags: the capture flags, and the replayed and break flags of this write.
    pub(crate) flags: AudioFlags,
    /// Its samples.
    pub(crate) samples: &'a [i16; AUDIO_FRAME_SAMPLES],
}

/// The reader side: the handoff's consumer and the replay store.
pub(crate) struct SpeechRing {
    /// The current capture stream's handoff; `None` while capture is stopped.
    handoff: Option<rtrb::Consumer<RingFrame>>,
    handoff_frames: usize,
    shared: Arc<Shared>,
    /// Every frame not confirmed, oldest first, in `seq` order; at most `store_frames`.
    store: VecDeque<RingFrame>,
    store_frames: usize,
    /// The newest capture stream's number, counted from 1.
    stream: u64,
    /// The newest frame pulled since capture last stopped.
    newest: Option<Newest>,
    /// The newest frame confirmed; the store holds only later frames.
    confirmed: u64,
    /// The newest frame written on the current connection, or `confirmed` before the first.
    written: u64,
    /// Capture stopped since the last frame pulled, so the next frame pulled breaks.
    break_next: bool,
    /// Frames the store dropped on overrun since the count was last taken.
    evicted: u64,
}

impl SpeechRing {
    /// A ring whose store holds at most `store_frames` frames ([`STORE_FRAMES`] in the product),
    /// each capture stream's handoff `handoff_frames` ([`HANDOFF_FRAMES`]); both at least one.
    /// The store is allocated here, once.
    pub(crate) fn new(store_frames: usize, handoff_frames: usize) -> Self {
        let store_frames = store_frames.max(1);
        Self {
            handoff: None,
            handoff_frames: handoff_frames.max(1),
            shared: Arc::new(Shared { next_seq: AtomicU64::new(1), lost: AtomicU64::new(0) }),
            store: VecDeque::with_capacity(store_frames),
            store_frames,
            stream: 0,
            newest: None,
            confirmed: 0,
            written: 0,
            break_next: false,
            evicted: 0,
        }
    }

    /// A writer for a new capture stream, which the component moves into its capture callback.
    /// Any earlier stream must have stopped, so its writer is never called again: a frame it
    /// pushed after this would reach no reader and no count. Frames still in that stream's
    /// handoff move into the store first and keep their numbers; once the new stream's first
    /// frame is pulled, those not yet written carry the replayed flag, since their capture times
    /// are on a clock the new stream's may not share.
    pub(crate) fn open_capture(&mut self) -> CaptureWriter {
        self.pull();
        self.stream = self.stream.wrapping_add(1);
        let (producer, consumer) = rtrb::RingBuffer::new(self.handoff_frames);
        self.handoff = Some(consumer);
        CaptureWriter { handoff: producer, shared: Arc::clone(&self.shared), stream: self.stream }
    }

    /// Capture has stopped (a mute): its stream will not call the writer again. Drops the
    /// handoff and every frame in the store, written or not, so nothing captured before the mute
    /// is written or replayed after it; forgets the newest capture time, since the next stream
    /// may run on another source's clock; and marks the next frame pulled with the break flag.
    pub(crate) fn capture_stopped(&mut self) {
        self.handoff = None;
        self.store.clear();
        self.newest = None;
        self.break_next = true;
    }

    /// Moves every frame waiting in the handoff into the store, dropping the store's oldest frames
    /// when it is full. The component calls it while no connection is up, so the handoff never
    /// fills; [`SpeechRing::next_frame`] calls it too.
    pub(crate) fn pull(&mut self) {
        let Some(handoff) = self.handoff.as_mut() else {
            return;
        };
        while let Ok(mut frame) = handoff.pop() {
            frame.break_before = std::mem::take(&mut self.break_next);
            if self.store.len() >= self.store_frames {
                self.store.pop_front();
                self.evicted = self.evicted.saturating_add(1);
            }
            self.newest = Some(Newest {
                seq: frame.seq,
                stream: frame.stream,
                captured_at: frame.captured_at,
            });
            self.store.push_back(frame);
        }
    }

    /// A new connection is up: the next frame written is the oldest one not confirmed.
    pub(crate) fn connected(&mut self) {
        self.pull();
        self.written = self.confirmed;
    }

    /// The next frame to write on the current connection, with its flags, or `None` when every
    /// frame in the store is written. The frame counts as written once returned; a write that
    /// fails ends the connection, and the next [`SpeechRing::connected`] replays it with the same
    /// break flag when capture had stopped before it.
    pub(crate) fn next_frame(&mut self) -> Option<Outgoing<'_>> {
        self.pull();
        // Relaxed: a count read stale by a frame or two only leaves out frames captured after
        // the pull above, which make this frame no older than the next write will see it.
        let taken = self.shared.next_seq.load(Ordering::Relaxed).saturating_sub(1);
        let written = self.written;
        let at = self.store.partition_point(|frame| frame.seq <= written);
        let frame = self.store.get(at)?;
        let mut flags = frame.flags.audio_flags();
        if frame.break_before || frame.seq != written.saturating_add(1) {
            flags = flags | AudioFlags::BREAK;
        }
        if replayed(frame, self.newest, taken) {
            flags = flags | AudioFlags::REPLAYED;
        }
        self.written = frame.seq;
        Some(Outgoing {
            seq: frame.seq,
            captured_at: frame.captured_at,
            flags,
            samples: &frame.samples,
        })
    }

    /// Frames up to `seq` are done and leave the store, though never past the newest frame written
    /// on the connection.
    pub(crate) fn confirm_through(&mut self, seq: u64) {
        let through = seq.min(self.written);
        if through <= self.confirmed {
            return;
        }
        self.confirmed = through;
        while self.store.front().is_some_and(|frame| frame.seq <= through) {
            self.store.pop_front();
        }
    }

    /// Reads what a frame from the speech process confirms: a `Pong` with no turn open, or an
    /// `EndOfTurn` event, confirms through the `seq` it carries; any other frame confirms nothing.
    pub(crate) fn on_speech_frame(&mut self, frame: &Frame) {
        let done = match &frame.body {
            FrameBody::Pong => frame.turn == [0; 16],
            FrameBody::Event(event) => matches!(event.event, VoiceEvent::EndOfTurn { .. }),
            FrameBody::Audio(_)
            | FrameBody::Command(_)
            | FrameBody::Pcm { .. }
            | FrameBody::Ping
            | FrameBody::Hello(_) => false,
        };
        if done {
            self.confirm_through(frame.seq);
        }
    }

    /// Frames lost since the count was last taken: to a full handoff, or dropped from a full
    /// store.
    pub(crate) fn take_dropped(&mut self) -> u64 {
        let lost = self.shared.lost.swap(0, Ordering::Relaxed);
        lost.saturating_add(std::mem::take(&mut self.evicted))
    }
}

/// Whether `frame`, written now, is more than [`REPLAYED_AFTER`] old. The capture side's now is
/// the newest frame pulled plus [`FRAME_TIME`] for each number taken since (`taken` is the newest
/// number taken), so a frame the handoff lost still counts as time captured. A frame of an
/// earlier stream than the newest counts as replayed, since the two clocks may differ.
fn replayed(frame: &RingFrame, newest: Option<Newest>, taken: u64) -> bool {
    let Some(newest) = newest else {
        // Never: every frame in the store was pulled after capture last stopped.
        return true;
    };
    if frame.stream != newest.stream {
        return true;
    }
    let since = u32::try_from(taken.saturating_sub(newest.seq)).unwrap_or(u32::MAX);
    let now = newest.captured_at.saturating_add(FRAME_TIME.saturating_mul(since));
    now.saturating_sub(frame.captured_at) > REPLAYED_AFTER
}

#[cfg(test)]
mod concurrency;

#[cfg(test)]
mod tests;
