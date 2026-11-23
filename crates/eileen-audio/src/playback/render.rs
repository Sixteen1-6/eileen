//! The render callback's half of playback (BUILD.md section 6.6): the [`Renderer`] hands the
//! queued samples to the device a buffer at a time, acts on each utterance's hold, moves the gain
//! toward the target over a millisecond, and reports what it played. It allocates, frees, blocks
//! and logs nothing (the crate's invariant); the rules it follows are the parent module's.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use eileen_contracts::core_io::PlaybackEvent;
use eileen_contracts::ids::SpeakId;
use eileen_stdx::num;
use rtrb::{Consumer, Producer};

use super::{Hold, Marker, SYNTH_RATE, Shared, ramp_frames};
use crate::stream::{RenderInfo, StreamFormat};

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
    /// The renderer for a device of `format`, reading the samples and markers the player queues,
    /// reporting into `events`, and sharing `shared` with the player.
    pub(super) fn new(
        samples: Consumer<f32>,
        markers: Consumer<Marker>,
        events: Producer<PlaybackEvent>,
        shared: Arc<Shared>,
        format: StreamFormat,
    ) -> Self {
        let rate = format.sample_rate_hz;
        Self {
            samples,
            markers,
            events,
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
        }
    }

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
            if hold == Hold::Playing && current.paused {
                // Resumed: the next pause is reported, even one that comes while it is dry.
                current.paused = false;
                self.current = Some(current);
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
