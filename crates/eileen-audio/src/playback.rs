//! Eileen's playback (BUILD.md sections 2.8, 3.3.7 and 6.6): the PCM the speech process sends,
//! queued per utterance and clause and played through the render callback, with duck, pause,
//! resume and stop, and the samples-played accounting the core maps back to the words heard.
//!
//! Two halves. The [`Player`] is the component's: PCM frames go into it straight from the speech
//! connection, never through `Core::step` (section 3.3.7), are converted from 24 kHz synthesis to
//! the device's rate by the resampler of `crate::resample`, and wait in a bounded queue, which
//! refuses and logs a frame that does not fit. The [`Renderer`] is the render callback's: it
//! hands the queued samples to the device and counts them, allocating, freeing, blocking and
//! logging nothing (the crate's invariant), and reports what it played as [`PlaybackEvent`]s
//! over a second bounded queue.
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
//!   50 ms of audio handed to the device, and `Paused` and `Stopped` once each. An utterance
//!   paused before its audio reaches the renderer is held at its first sample (`Paused` with 0
//!   played), and one stopped before any audio arrived is reported by the player itself
//!   (`Stopped` with 0 played).
//! - `Drained` means every sample queued so far for the utterance has played, as the core reads
//!   it: the queue ran dry while it played, or a later utterance's audio began. It comes once per
//!   dry spell, so more than once when synthesis falls behind, and the utterance keeps taking
//!   audio after it. A reply is one utterance whose clauses arrive in batches as its text
//!   streams, and only the core knows when the reply is closed, so nothing here ends an
//!   utterance's intake but a `Stop` or a later utterance.
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

use crate::resample::{Conversion, Resampler};
use crate::stream::{RenderInfo, StreamFormat};

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
    #[error("the playback queue cannot hold {samples} more samples")]
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
    let renderer = Renderer {
        samples: sample_out,
        markers: marker_out,
        events: event_in,
        shared,
        channels: usize::from(format.channels.max(1)),
        rate: u64::from(rate),
        gain: 1.0,
        ramp_to: 1.0,
        ramp: ramp_frames(rate),
        ramp_steps: f32::from(u16::try_from(ramp_frames(rate)).unwrap_or(u16::MAX)),
        ramp_step: 0.0,
        ramp_left: 0,
        played_every: usize::try_from(rate / 20).unwrap_or(1).max(1),
        since_played: 0,
        current: None,
    };
    Ok((player, renderer))
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
        self.input.extend(samples.iter().map(|&s| f32::from(s) / 32_768.0));
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
        self.reported.pop_front().or_else(|| self.events.pop().ok())
    }
}

/// The clause being played.
#[derive(Debug, Clone, Copy)]
struct Current {
    ordinal: u64,
    speak: SpeakId,
    clause: u16,
    /// Device frames left of the marker being played.
    left: usize,
    /// Device frames of this clause handed to the device.
    clause_frames: u64,
    /// Whether its pause was reported.
    paused: bool,
    /// Whether the queue ran dry since its last marker was taken, and `Drained` was reported.
    drained: bool,
}

/// What the renderer does next.
enum Step {
    /// Play up to this many frames of the current clause.
    Play(usize),
    /// Nothing to play: silence.
    Silence,
}

/// The render callback's half. Allocates, frees, blocks and logs nothing.
pub(crate) struct Renderer {
    samples: Consumer<f32>,
    markers: Consumer<Marker>,
    events: Producer<PlaybackEvent>,
    shared: Arc<Shared>,
    channels: usize,
    rate: u64,
    /// The gain of the last frame scaled.
    gain: f32,
    /// The target the latest ramp moves to.
    ramp_to: f32,
    /// Frames a ramp takes (1 ms), as a count and as a divisor.
    ramp: usize,
    ramp_steps: f32,
    /// The gain change per frame of the ramp under way, and its frames left.
    ramp_step: f32,
    ramp_left: usize,
    played_every: usize,
    since_played: usize,
    current: Option<Current>,
}

impl Renderer {
    /// Fills one buffer of interleaved device samples: the queued audio, then silence.
    pub(crate) fn fill(&mut self, out: &mut [f32], _info: RenderInfo) {
        let target = f32::from_bits(self.shared.gain.load(Ordering::Acquire));
        let frames = out.len() / self.channels;
        let mut done = 0;
        while done < frames {
            let Step::Play(run) = self.step() else { break };
            let start = done.saturating_mul(self.channels);
            let room = out.get_mut(start..).unwrap_or_default();
            let played = self.copy(room, run.min(frames - done));
            if played == 0 {
                break;
            }
            done += played;
            self.count(played);
        }
        for sample in out.iter_mut().skip(done.saturating_mul(self.channels)) {
            *sample = 0.0;
        }
        self.apply_gain(out, target);
    }

    /// Decides what plays next, moving through the markers and acting on holds.
    fn step(&mut self) -> Step {
        loop {
            let Some(mut current) = self.current else {
                let Ok(Marker { ordinal, speak, clause, frames }) = self.markers.pop() else {
                    return Step::Silence;
                };
                self.reach(ordinal);
                self.current = Some(Current {
                    ordinal,
                    speak,
                    clause,
                    left: frames,
                    clause_frames: 0,
                    paused: false,
                    drained: false,
                });
                continue;
            };
            let hold = self.shared.hold(current.ordinal);
            if hold == Hold::Stopped {
                self.report(PlaybackEvent::Stopped {
                    speak: current.speak,
                    clause: current.clause,
                    samples_played: self.synth_samples(current.clause_frames),
                });
                self.skip(current.left);
                self.skip_rest_of(current.ordinal);
                self.current = None;
                continue;
            }
            if current.left == 0 {
                match self.markers.peek().copied() {
                    Ok(Marker { ordinal, clause, frames, .. }) if ordinal == current.ordinal => {
                        let _taken = self.markers.pop();
                        if clause != current.clause {
                            current.clause = clause;
                            current.clause_frames = 0;
                        }
                        current.left = frames;
                        current.drained = false;
                        self.current = Some(current);
                    }
                    Ok(_) => {
                        // A later utterance began, so this one takes no more audio. A dry spell
                        // already reported is the same one, so it is not reported again.
                        if !current.drained {
                            self.report(PlaybackEvent::Drained { speak: current.speak });
                        }
                        self.current = None;
                    }
                    Err(_) if hold == Hold::Paused => return self.hold_paused(current),
                    Err(_) => {
                        // Dry: every sample queued so far has played. The utterance stays
                        // current, so its next batch continues it.
                        if !current.drained {
                            current.drained = true;
                            self.current = Some(current);
                            self.report(PlaybackEvent::Drained { speak: current.speak });
                        }
                        return Step::Silence;
                    }
                }
                continue;
            }
            if hold == Hold::Paused {
                return self.hold_paused(current);
            }
            current.paused = false;
            self.current = Some(current);
            return Step::Play(current.left);
        }
    }

    /// Holds a paused clause, reporting the pause once.
    fn hold_paused(&mut self, mut current: Current) -> Step {
        if !current.paused {
            current.paused = true;
            self.current = Some(current);
            self.report(PlaybackEvent::Paused {
                speak: current.speak,
                clause: current.clause,
                samples_played: self.synth_samples(current.clause_frames),
            });
        }
        Step::Silence
    }

    /// Notes that the renderer reached `ordinal`, so the player may reuse older slots.
    fn reach(&self, ordinal: u64) {
        if ordinal > self.shared.head.load(Ordering::SeqCst) {
            self.shared.head.store(ordinal, Ordering::SeqCst);
        }
    }

    /// Copies up to `frames` queued samples into `out`, each on every channel; returns the frames
    /// copied.
    fn copy(&mut self, out: &mut [f32], frames: usize) -> usize {
        let frames = frames.min(self.samples.slots()).min(out.len() / self.channels);
        let Ok(chunk) = self.samples.read_chunk(frames) else { return 0 };
        let (first, second) = chunk.as_slices();
        for (sample, frame) in first.iter().chain(second).zip(out.chunks_exact_mut(self.channels)) {
            frame.fill(*sample);
        }
        chunk.commit_all();
        frames
    }

    /// Counts `frames` handed to the device for the current clause, and reports `Played` each
    /// time another 50 ms has gone out.
    fn count(&mut self, frames: usize) {
        let Some(mut current) = self.current else { return };
        current.left -= frames.min(current.left);
        current.clause_frames += num::saturating_u64_from_usize(frames);
        self.current = Some(current);
        self.since_played += frames;
        if self.since_played >= self.played_every {
            // One report however long the buffer, so two never come within 50 ms of audio.
            self.since_played %= self.played_every;
            self.report(PlaybackEvent::Played {
                speak: current.speak,
                clause: current.clause,
                samples_played: self.synth_samples(current.clause_frames),
            });
        }
    }

    /// Drops `frames` queued samples unplayed.
    fn skip(&mut self, frames: usize) {
        let frames = frames.min(self.samples.slots());
        if let Ok(chunk) = self.samples.read_chunk(frames) {
            chunk.commit_all();
        }
    }

    /// Drops the queued markers of `ordinal`, and their samples.
    fn skip_rest_of(&mut self, ordinal: u64) {
        while let Ok(marker) = self.markers.peek().copied() {
            if marker.ordinal != ordinal {
                break;
            }
            let _taken = self.markers.pop();
            self.skip(marker.frames);
        }
    }

    /// `frames` device frames as samples of the 24 kHz clause.
    fn synth_samples(&self, frames: u64) -> u32 {
        let scaled = u128::from(frames) * u128::from(SYNTH_RATE) / u128::from(self.rate.max(1));
        num::saturating_u32_from_u64(num::saturating_u64_from_u128(scaled))
    }

    /// Queues an event, or counts it lost when the queue is full.
    fn report(&mut self, event: PlaybackEvent) {
        if self.events.push(event).is_err() {
            self.shared.lost.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Scales the buffer by the gain. A new `target` starts a ramp from the gain now to it over
    /// the next millisecond of frames, which runs on into later buffers when this one is shorter.
    fn apply_gain(&mut self, out: &mut [f32], target: f32) {
        if target.to_bits() != self.ramp_to.to_bits() {
            self.ramp_to = target;
            self.ramp_left = self.ramp;
            self.ramp_step = (target - self.gain) / self.ramp_steps;
        }
        for frame in out.chunks_exact_mut(self.channels) {
            if self.ramp_left > 0 {
                self.ramp_left -= 1;
                self.gain = if self.ramp_left == 0 { target } else { self.gain + self.ramp_step };
            }
            for sample in frame {
                *sample *= self.gain;
            }
        }
    }
}
