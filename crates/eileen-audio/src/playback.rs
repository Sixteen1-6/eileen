//! Eileen's playback (BUILD.md sections 2.8, 3.3.7 and 6.6): the PCM the speech process sends,
//! queued per utterance and clause and played through the render callback, with duck, pause,
//! resume and stop, and the samples-played accounting the core maps back to the words heard.
//!
//! Two halves. The [`Player`] is the component's: PCM frames go into it straight from the speech
//! connection, never through `Core::step` (section 3.3.7), are converted from 24 kHz synthesis to
//! the device's rate by the resampler of `crate::resample`, and wait in a bounded queue, which
//! refuses and logs a frame that does not fit. The [`Renderer`] (in `render`) is the render
//! callback's: it hands the queued samples to the device and counts them, allocating, freeing,
//! blocking and logging nothing (the crate's invariant), and reports what it played as
//! [`PlaybackEvent`]s over a second bounded queue.
//!
//! The rules:
//!
//! - Utterances play in the order their audio arrived, each whole: audio of an utterance that
//!   comes after a later one's began is refused, so a stopped or overtaken utterance never plays
//!   again. A paused utterance with audio left holds the ones behind it until it resumes or
//!   stops, so Eileen never talks over herself.
//! - `Duck { db }` and `Unduck` take effect in the next output callback (simulator invariant 2):
//!   the gain moves over its first millisecond of frames (on into the next buffers when the
//!   callback's is shorter), so the step does not click, then holds. A duck only lowers; a
//!   positive `db` counts as 0.
//! - `Pause { speak }` stops the utterance at a sample by the next callback and keeps every
//!   queued clause; `Resume` continues from that sample, and the resampler's history is kept
//!   with it, so the audio after the pause is exactly what would have followed. A pause and a
//!   resume that both land between two callbacks cut nothing and report nothing. `Stop` drops the
//!   utterance's queued audio and refuses any that comes later. A command for an utterance the
//!   renderer has passed changes nothing: it has played out.
//! - Each utterance starts the interpolator from silence, and the last 16 samples of the one
//!   before it (under 1 ms) stay in the old interpolator's history unplayed; speech ends in
//!   silence, so nothing heard is lost, and `samples_played` never counts them.
//! - `samples_played` counts the clause's own samples, at the 24 kHz of its `PcmHeader`, as
//!   `ClauseReady.samples` does, so the core maps it straight to characters: the device's samples
//!   since the clause began, times 24 kHz over the device's rate. `Played` comes at most every
//!   50 ms of audio handed to the device, `Paused` once per pause and `Stopped` once. An utterance
//!   paused before its audio reaches the renderer is held at its first sample (`Paused` with 0
//!   played), and one stopped before any audio arrived is reported by the player itself
//!   (`Stopped` with 0 played).
//! - `Drained` means every sample queued so far for the utterance has played, as the core reads
//!   it: the queue ran dry while it played, or a later utterance's audio began. It comes once per
//!   dry spell, so more than once when synthesis falls behind, and the utterance keeps taking
//!   audio after it. A reply is one utterance whose clauses arrive in batches as its text
//!   streams, and only the core knows when the reply is closed, so nothing here ends an
//!   utterance's intake but a `Stop` or a later utterance. The player drops a `Drained` that the
//!   newest utterance's later audio has made stale (samples were queued after the renderer ran
//!   dry), since the core, reading it after that audio's `ClauseReady`, would end a closed reply
//!   with its last clause unplayed; the next dry spell is reported instead.
//!
//! The halves share three `rtrb` SPSC queues (samples, markers and events) and a few atomics: the
//! target gain, the newest utterance the renderer has reached, and one hold (playing, paused or
//! stopped) per live utterance. Each utterance gets an ordinal in arrival order, and its hold
//! lives in slot `ordinal % UTTERANCES`. The player gives a new ordinal only within
//! [`UTTERANCES`] of the one the renderer last reached (`ordinal - head < UTTERANCES`), and
//! writes a hold only for an ordinal not older than that one, so no write reaches a slot another
//! live utterance holds. Before the renderer has reached any, seven may wait. Holds are stored
//! and loaded sequentially consistent: a new utterance's hold is stored before any of its audio
//! is queued, so no sample of it plays under an older utterance's hold, and a stop is stored
//! after the audio it drops, so a renderer that sees the stop also sees every marker queued
//! before it.

mod render;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};

use eileen_contracts::core_io::{AudioCmd, PlaybackEvent};
use eileen_contracts::ids::SpeakId;
use eileen_contracts::speech::PcmHeader;
use eileen_stdx::num;
use eileen_trace::names;
use rtrb::{Consumer, Producer, RingBuffer};

pub(crate) use self::render::Renderer;
use crate::resample::{Conversion, Resampler};
use crate::stream::StreamFormat;

/// The rate the speech process synthesizes at, the only one a PCM frame may carry.
pub(crate) const SYNTH_RATE: u32 = 24_000;

/// The playback queue's default length: 30 s of device samples, about 5.8 MB at 48 kHz, which
/// holds the largest PCM frame (1 MiB, about 22 s at 24 kHz) with room for the next clause.
const QUEUE_SECONDS: usize = 30;

/// Markers the queue holds: one per PCM frame.
const MARKERS: usize = 1_024;

/// Events the renderer can report before the component drains them; more are counted as lost.
const EVENTS: usize = 256;

/// Utterances live at once (queued, playing or held), and so the hold slots.
const UTTERANCES: u64 = 8;

/// The hold slots: [`UTTERANCES`] of them.
const SLOTS: usize = 8;

/// Utterances the player remembers as ended, so their late audio is refused.
const ENDED: usize = 64;

/// Utterances the player remembers before their audio arrives (paused early).
const WAITING: usize = 16;

/// Why the player did not take a frame or could not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PlayError {
    /// No conversion goes from 24 kHz synthesis to this device rate, or a frame came at a rate
    /// other than 24 kHz.
    #[error("no playback at {sample_rate} Hz")]
    Rate {
        /// The rate refused.
        sample_rate: u32,
    },
    /// The sample or marker queue cannot hold the frame; the frame is dropped and the drop
    /// logged. Room comes back as the renderer plays.
    #[error("the playback queue has no room for a frame of {samples} samples")]
    Full {
        /// Device samples the frame needed.
        samples: usize,
    },
    /// The frame would start a new utterance while the most playback keeps are live; it is
    /// dropped and the drop logged. Room comes back when the renderer moves on to a later
    /// utterance, not as samples play.
    #[error("playback already holds {live} live utterances, the most it keeps")]
    Utterances {
        /// Utterances live when the frame came.
        live: u64,
    },
    /// The utterance was stopped, or a later one's audio began after it.
    #[error("the utterance takes no more audio")]
    Ended,
}

/// Where an utterance stands, as its hold slot carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Hold {
    Playing = 0,
    Paused = 1,
    Stopped = 2,
}

impl Hold {
    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Paused,
            2 => Self::Stopped,
            _ => Self::Playing,
        }
    }
}

/// An entry in the marker queue: `frames` device frames of one clause, after the samples it
/// covers.
#[derive(Debug, Clone, Copy)]
struct Marker {
    ordinal: u64,
    speak: SpeakId,
    clause: u16,
    frames: usize,
}

/// A bounded queue or list of the player's whose overflow it logs as `channel.full`.
#[derive(Debug, Clone, Copy)]
enum Queue {
    /// The device samples.
    Samples,
    /// The markers, one per PCM frame.
    Markers,
    /// The live utterances and their hold slots.
    Utterances,
    /// Utterances paused before their audio.
    Waiting,
    /// Events the player reports itself.
    Reported,
}

impl Queue {
    const ALL: usize = 5;

    const fn channel(self) -> &'static str {
        match self {
            Self::Samples => "audio.playback",
            Self::Markers => "audio.playback.markers",
            Self::Utterances => "audio.playback.utterances",
            Self::Waiting => "audio.playback.waiting",
            Self::Reported => "audio.playback.reported",
        }
    }
}

/// What both halves read.
struct Shared {
    /// The target gain's `f32` bits.
    gain: AtomicU32,
    /// The ordinal of the newest utterance the renderer has reached; 0 before the first.
    head: AtomicU64,
    /// One hold per live utterance, at `ordinal % UTTERANCES`.
    holds: [AtomicU8; SLOTS],
    /// Events the renderer could not queue.
    lost: AtomicU64,
}

impl Shared {
    fn hold(&self, ordinal: u64) -> Hold {
        let slot = self.holds.get(slot_of(ordinal));
        Hold::from_u8(slot.map_or(0, |slot| slot.load(Ordering::SeqCst)))
    }
}

fn slot_of(ordinal: u64) -> usize {
    num::saturating_usize_from_u64(ordinal % UTTERANCES)
}

/// `ordinal` if its utterance is still live: not older than `head`, the one the renderer last
/// reached. An older one has played out, and its slot may already be a later utterance's.
fn live(ordinal: Option<u64>, head: u64) -> Option<u64> {
    ordinal.filter(|&ordinal| ordinal >= head)
}

/// The frames the gain moves over: 1 ms at the device's rate.
pub(crate) fn ramp_frames(rate: u32) -> usize {
    usize::try_from(rate / 1_000).unwrap_or(1).max(1)
}

/// A synthesized sample at the device's full scale of 1.0.
fn to_device(sample: i16) -> f32 {
    f32::from(sample) / 32_768.0
}

/// What the renderer hands a device at `device_rate`, on each channel, for `samples` of one
/// utterance played without a break: a fresh interpolator over the whole of them in one call,
/// which gives the same samples bit for bit as any cut into clauses, frames and buffers. `None`
/// when playback has no conversion to that rate.
#[cfg(any(test, feature = "fake"))]
pub(crate) fn rendered(device_rate: u32, samples: &[i16]) -> Option<Vec<f32>> {
    let mut resampler = Resampler::new(Conversion::between(SYNTH_RATE, device_rate)?);
    let input: Vec<f32> = samples.iter().copied().map(to_device).collect();
    let mut output = vec![0.0; resampler.output_len(input.len())];
    let written = resampler.process(&input, &mut output).ok()?;
    output.truncate(written);
    Some(output)
}

/// The default queue for a device of `format`: [`QUEUE_SECONDS`] of its frames.
pub(crate) fn default_queue(format: StreamFormat) -> usize {
    usize::try_from(format.sample_rate_hz).unwrap_or(usize::MAX).saturating_mul(QUEUE_SECONDS)
}

/// A player and its renderer for a device of `format`, with a queue of `queue` device frames.
pub(crate) fn playback(
    format: StreamFormat,
    queue: usize,
) -> Result<(Player, Renderer), PlayError> {
    playback_with(format, queue, EVENTS)
}

/// [`playback`] with an event queue of `events`.
pub(crate) fn playback_with(
    format: StreamFormat,
    queue: usize,
    events: usize,
) -> Result<(Player, Renderer), PlayError> {
    let rate = format.sample_rate_hz;
    let conversion =
        Conversion::between(SYNTH_RATE, rate).ok_or(PlayError::Rate { sample_rate: rate })?;
    let shared = Arc::new(Shared {
        gain: AtomicU32::new(1.0_f32.to_bits()),
        head: AtomicU64::new(0),
        holds: Default::default(),
        lost: AtomicU64::new(0),
    });
    let (sample_in, sample_out) = RingBuffer::new(queue);
    let (marker_in, marker_out) = RingBuffer::new(MARKERS);
    let (event_in, event_out) = RingBuffer::new(events);
    let player = Player {
        samples: sample_in,
        markers: marker_in,
        events: event_out,
        shared: Arc::clone(&shared),
        conversion,
        resampler: Resampler::new(conversion),
        input: Vec::new(),
        output: Vec::new(),
        capacity: queue,
        utterances: BTreeMap::new(),
        ended: VecDeque::new(),
        newest: None,
        next_ordinal: 1,
        reported: VecDeque::with_capacity(EVENTS),
        drops: [0; Queue::ALL],
        lost: 0,
    };
    Ok((player, Renderer::new(sample_out, marker_out, event_in, shared, format)))
}

/// What the player knows of one utterance.
#[derive(Debug, Clone, Copy)]
struct Utterance {
    /// Its ordinal, once its first audio was queued.
    ordinal: Option<u64>,
    hold: Hold,
}

/// The component's half: takes PCM and the playback commands, and gives the events.
pub(crate) struct Player {
    samples: Producer<f32>,
    markers: Producer<Marker>,
    events: Consumer<PlaybackEvent>,
    shared: Arc<Shared>,
    conversion: Conversion,
    resampler: Resampler,
    /// A frame's samples as `f32`, reused.
    input: Vec<f32>,
    /// A frame's samples at the device's rate, reused.
    output: Vec<f32>,
    capacity: usize,
    /// Utterances live or paused before their audio.
    utterances: BTreeMap<SpeakId, Utterance>,
    /// Utterances that take no more audio, newest last.
    ended: VecDeque<SpeakId>,
    /// The utterance whose audio was queued last, which may take more.
    newest: Option<SpeakId>,
    next_ordinal: u64,
    /// Events the player reports itself (an utterance stopped before any audio), at most
    /// [`EVENTS`].
    reported: VecDeque<PlaybackEvent>,
    /// Drops counted per [`Queue`], for the log.
    drops: [u64; Queue::ALL],
    /// Events the renderer lost, as last logged.
    lost: u64,
}

impl Player {
    /// Queues a PCM frame's samples for its utterance and clause.
    pub(crate) fn push(&mut self, header: PcmHeader, samples: &[i16]) -> Result<(), PlayError> {
        if header.sample_rate != SYNTH_RATE {
            return Err(PlayError::Rate { sample_rate: header.sample_rate });
        }
        let speak = header.speak;
        let ordinal = match self.utterances.get(&speak).and_then(|u| u.ordinal) {
            Some(ordinal) if self.newest == Some(speak) => ordinal,
            Some(_) => return Err(PlayError::Ended),
            None if self.ended.contains(&speak) => return Err(PlayError::Ended),
            None => self.next_ordinal,
        };
        // A new utterance starts its resampler from silence.
        let fresh = (Some(speak) != self.newest).then(|| Resampler::new(self.conversion));
        let needed = fresh.as_ref().unwrap_or(&self.resampler).output_len(samples.len());
        let head = self.shared.head.load(Ordering::SeqCst);
        if fresh.is_some() && ordinal.saturating_sub(head) >= UTTERANCES {
            self.note_full(Queue::Utterances);
            // Before the renderer reaches any, ordinals start at 1.
            return Err(PlayError::Utterances { live: ordinal.saturating_sub(head.max(1)) });
        }
        if self.samples.slots() < needed {
            self.note_full(Queue::Samples);
            return Err(PlayError::Full { samples: needed });
        }
        if self.markers.is_full() {
            self.note_full(Queue::Markers);
            return Err(PlayError::Full { samples: needed });
        }
        if let Some(fresh) = fresh {
            self.begin(speak, ordinal, fresh);
        }
        self.input.clear();
        self.input.extend(samples.iter().copied().map(to_device));
        self.output.resize(needed, 0.0);
        let written = self.resampler.process(&self.input, &mut self.output).unwrap_or(0);
        let converted = self.output.get(..written).unwrap_or_default();
        if self.samples.push_entire_slice(converted).is_err()
            || self
                .markers
                .push(Marker { ordinal, speak, clause: header.clause, frames: written })
                .is_err()
        {
            // Both had room above, and only this half writes them.
            return Err(PlayError::Full { samples: needed });
        }
        Ok(())
    }

    /// Starts a new utterance at `ordinal` with `resampler`: the one before it takes no more
    /// audio, and its hold slot takes its hold before any of its audio is queued.
    fn begin(&mut self, speak: SpeakId, ordinal: u64, resampler: Resampler) {
        if let Some(previous) = self.newest.replace(speak) {
            self.end(previous);
        }
        let hold = self.utterances.get(&speak).map_or(Hold::Playing, |u| u.hold);
        self.utterances.insert(speak, Utterance { ordinal: Some(ordinal), hold });
        self.set_hold(ordinal, hold);
        self.next_ordinal = ordinal + 1;
        self.resampler = resampler;
    }

    fn set_hold(&self, ordinal: u64, hold: Hold) {
        if let Some(slot) = self.shared.holds.get(slot_of(ordinal)) {
            slot.store(hold as u8, Ordering::SeqCst);
        }
    }

    /// Marks `speak` as taking no more audio.
    fn end(&mut self, speak: SpeakId) {
        if self.newest == Some(speak) {
            self.newest = None;
        }
        if !self.ended.contains(&speak) {
            if self.ended.len() == ENDED {
                self.ended.pop_front();
            }
            self.ended.push_back(speak);
        }
    }

    /// Logs a drop for want of room in `queue`, with that queue's own count.
    fn note_full(&mut self, queue: Queue) {
        let capacity = match queue {
            Queue::Samples => self.capacity,
            Queue::Markers => MARKERS,
            Queue::Utterances => SLOTS,
            Queue::Waiting => WAITING,
            Queue::Reported => EVENTS,
        };
        let Some(times) = self.drops.get_mut(queue as usize) else { return };
        *times += 1;
        let times = *times;
        tracing::warn!(name: names::CHANNEL_FULL, channel = queue.channel(), capacity, times);
    }

    /// Whether audio is queued that the renderer has not yet handed to the device.
    pub(crate) fn has_audio(&self) -> bool {
        self.samples.slots() < self.capacity
    }

    /// Serves a playback command and returns whether it was one.
    pub(crate) fn apply(&mut self, cmd: &AudioCmd) -> bool {
        match *cmd {
            AudioCmd::Duck { db } => self.duck(db),
            AudioCmd::Unduck => self.duck(0),
            AudioCmd::Pause { speak } => self.pause(speak),
            AudioCmd::Resume { speak } => self.resume(speak),
            AudioCmd::Stop { speak } => self.stop(speak),
            AudioCmd::SetCapture(_)
            | AudioCmd::PlayPhrase { .. }
            | AudioCmd::Earcon(_)
            | AudioCmd::Record { .. } => return false,
        }
        true
    }

    /// Lowers playback by `db` decibels from the next callback; 0 is full gain.
    pub(crate) fn duck(&self, db: i8) {
        let gain = 10_f32.powf(f32::from(db.min(0)) / 20.0);
        self.shared.gain.store(gain.to_bits(), Ordering::Release);
    }

    /// Holds `speak` at its next sample, or at its first if it has not begun.
    pub(crate) fn pause(&mut self, speak: SpeakId) {
        let head = self.head();
        if let Some(utterance) = self.utterances.get_mut(&speak) {
            if utterance.hold == Hold::Playing {
                utterance.hold = Hold::Paused;
                let ordinal = utterance.ordinal;
                if let Some(ordinal) = live(ordinal, head) {
                    self.set_hold(ordinal, Hold::Paused);
                }
            }
            return;
        }
        if self.ended.contains(&speak) {
            return;
        }
        if self.utterances.values().filter(|u| u.ordinal.is_none()).count() >= WAITING {
            self.note_full(Queue::Waiting);
            return;
        }
        self.utterances.insert(speak, Utterance { ordinal: None, hold: Hold::Paused });
    }

    /// The ordinal of the newest utterance the renderer has reached.
    fn head(&self) -> u64 {
        self.shared.head.load(Ordering::SeqCst)
    }

    /// Continues `speak` from the sample it paused at.
    pub(crate) fn resume(&mut self, speak: SpeakId) {
        let Some(utterance) = self.utterances.get_mut(&speak) else { return };
        if utterance.hold != Hold::Paused {
            return;
        }
        utterance.hold = Hold::Playing;
        let ordinal = utterance.ordinal;
        match ordinal {
            Some(ordinal) => {
                if let Some(ordinal) = live(Some(ordinal), self.head()) {
                    self.set_hold(ordinal, Hold::Playing);
                }
            }
            // Paused before its audio: nothing left to remember.
            None => {
                self.utterances.remove(&speak);
            }
        }
    }

    /// Drops `speak`'s queued audio and refuses any more.
    pub(crate) fn stop(&mut self, speak: SpeakId) {
        let head = self.head();
        let known = self.utterances.get_mut(&speak);
        match known.map(|u| (u.ordinal, std::mem::replace(&mut u.hold, Hold::Stopped))) {
            Some((Some(ordinal), _)) => {
                // An utterance the renderer has passed played out; its slot may be another's.
                if let Some(ordinal) = live(Some(ordinal), head) {
                    self.set_hold(ordinal, Hold::Stopped);
                }
            }
            Some((None, _)) | None if !self.ended.contains(&speak) => {
                self.utterances.remove(&speak);
                if self.reported.len() >= EVENTS {
                    self.note_full(Queue::Reported);
                } else {
                    self.reported.push_back(PlaybackEvent::Stopped {
                        speak,
                        clause: 0,
                        samples_played: 0,
                    });
                }
            }
            Some((None, _)) | None => {}
        }
        self.end(speak);
    }

    /// The next event of playback, if one is waiting. Logs events the renderer lost.
    pub(crate) fn next_event(&mut self) -> Option<PlaybackEvent> {
        let lost = self.shared.lost.load(Ordering::Relaxed);
        if lost != self.lost {
            self.lost = lost;
            let capacity = self.events.buffer().capacity();
            tracing::warn!(
                name: names::CHANNEL_FULL,
                channel = "audio.playback.events",
                capacity,
                times = lost
            );
        }
        let head = self.shared.head.load(Ordering::SeqCst);
        self.utterances.retain(|_, u| u.ordinal.is_none_or(|ordinal| ordinal >= head));
        if let Some(event) = self.reported.pop_front() {
            return Some(event);
        }
        while let Ok(event) = self.events.pop() {
            if !self.is_stale(&event) {
                return Some(event);
            }
        }
        None
    }

    /// Whether `event` is a `Drained` that audio queued since has made stale: one for the newest
    /// utterance while samples are queued, so the renderer ran dry before that audio came. Passed
    /// on after the audio's `ClauseReady`, it would end a closed reply in the core with its last
    /// clause unplayed; the renderer reports the next dry spell when that audio has played.
    pub(crate) fn is_stale(&self, event: &PlaybackEvent) -> bool {
        matches!(*event, PlaybackEvent::Drained { speak } if self.newest == Some(speak))
            && self.has_audio()
    }
}
