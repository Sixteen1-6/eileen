//! The component's side of playback (BUILD.md section 6.6, "The core end of the speech pipe" and
//! "Duck, and what waits for W2"): the render stream of `AudioConfig::playback` with the renderer
//! in its callback, the speech process's PCM taken straight into the player, never through
//! `Core::step` (section 3.3.7), the five playback commands, and what playback reports, which the
//! component passes to the core as `Input::Playback`.
//!
//! The render callback cannot wake the component, so its events are polled: the pump takes them
//! every 10 ms while capture runs, and otherwise while playback has had something to do within
//! [`PLAYBACK_SETTLES`].
//!
//! The core reads `Drained` as every sample of the utterance it was told of having played, so a
//! `Drained` must never reach it after a `ClauseReady` whose audio is not yet queued: it would end
//! a closed reply with that clause unplayed. The contract has the speech process send a clause's
//! PCM before its `ClauseReady` (`VoiceEvent::ClauseReady`), and then the player's own filter of a
//! stale `Drained` is enough. Should a `ClauseReady` come first anyway, or its PCM span reads the
//! pump runs between, the component holds a `Drained` for that utterance until the announced
//! samples have all come. It then drops it, since the renderer reports the next dry spell once
//! they have played, or passes it on if no audio was queued since. A later utterance's audio, a
//! refusal of the clause's PCM or the connection closing ends the wait the same way, and a `Stop`
//! drops a held `Drained`, since the renderer then reports `Stopped`, which ends the utterance in
//! the core.
//!
//! A sink that was given and did not start cannot sound, so the core hears that rather than
//! nothing: the first PCM of each utterance is answered with `Stalled`, which the core turns into
//! a stop, and each `Stop` with `Stopped` at the first sample.

use std::sync::PoisonError;
use std::time::Duration;

use eileen_contracts::core_io::{AudioCmd, PlaybackEvent};
use eileen_contracts::ids::SpeakId;
use eileen_contracts::speech::PcmHeader;
use eileen_stdx::num;
use tokio::time::Instant;

use crate::SharedSink;
use crate::playback::{self, PlayError, Player};
use crate::stream::{AudioStream, RenderInfo};

/// How long after playback last had something to do (audio queued, an event reported, a frame or
/// a command taken) the pump still takes its events every 10 ms while capture is off: longer than
/// any endpoint's callback period, so what the callback after a stop, a pause or the queue's last
/// sample reports reaches the core within a pump. Past it the once-a-second report takes them.
const PLAYBACK_SETTLES: Duration = Duration::from_millis(200);

/// Eileen's speakers, as the component has them.
pub(super) enum Speakers {
    /// `AudioConfig::playback` gave no sink, as the stub runs: PCM is dropped and nothing is
    /// reported.
    Absent,
    /// A sink was given and its stream did not start.
    Failed {
        /// The utterance last reported `Stalled`.
        stalled: Option<SpeakId>,
    },
    /// The player, and the render stream its renderer fills.
    Running(Box<Playback>),
}

impl Speakers {
    /// Starts the render stream of `sink`, if one was given, with the renderer in its callback. A
    /// sink at a rate playback has no conversion to, or one whose stream does not start, is logged
    /// at warn and leaves the speakers failed.
    pub(super) fn start(sink: Option<&SharedSink>) -> Self {
        let Some(sink) = sink else { return Self::Absent };
        match Playback::start(sink) {
            Some(playback) => Self::Running(Box::new(playback)),
            None => Self::Failed { stalled: None },
        }
    }

    /// Whether the pump still takes playback's events: it had something to do within
    /// [`PLAYBACK_SETTLES`].
    pub(super) fn active(&self) -> bool {
        matches!(self, Self::Running(playback) if playback.active())
    }

    /// Plays a PCM frame from the speech process. Returns `Stalled` for the first frame of each
    /// utterance when the sink did not start.
    pub(super) fn play(&mut self, header: PcmHeader, samples: &[i16]) -> Option<PlaybackEvent> {
        if let Self::Running(playback) = self {
            playback.play(header, samples);
            return None;
        }
        tracing::debug!(
            speak = %header.speak,
            clause = header.clause,
            "there is no playback; PCM dropped"
        );
        let Self::Failed { stalled } = self else { return None };
        if *stalled == Some(header.speak) {
            return None;
        }
        *stalled = Some(header.speak);
        Some(PlaybackEvent::Stalled)
    }

    /// Notes a `ClauseReady` the component passes to the core, so no `Drained` from before that
    /// clause's audio reaches the core after it.
    pub(super) fn announce(&mut self, speak: SpeakId, clause: u16, samples: u32) {
        if let Self::Running(playback) = self {
            playback.announce(ClauseSamples { speak, clause, samples });
        }
    }

    /// The speech connection closed: no audio announced on it is coming.
    pub(super) fn connection_closed(&mut self) {
        if let Self::Running(playback) = self {
            playback.release();
        }
    }

    /// Serves `Duck`, `Unduck`, `Pause`, `Resume` or `Stop` while playback runs, and returns
    /// whether it did. Each is an atomic store or a bounded queue operation that never blocks, so
    /// the component serves one as it arrives, even while a write on the pipe waits (invariant 2).
    /// Any other command, or any command without running playback, is the loop's.
    pub(super) fn serve(&mut self, cmd: &AudioCmd) -> bool {
        match self {
            Self::Running(playback) => playback.apply(cmd),
            Self::Failed { .. } | Self::Absent => false,
        }
    }

    /// A playback command the loop takes: served as [`Self::serve`] serves it, or, when the sink
    /// did not start, a `Stop` answered with `Stopped` at the first sample, as the player reports
    /// an utterance stopped before its audio.
    pub(super) fn command(&mut self, cmd: &AudioCmd) -> Option<PlaybackEvent> {
        if self.serve(cmd) {
            return None;
        }
        tracing::debug!(?cmd, "there is no playback; the command changes nothing");
        match (self, cmd) {
            (Self::Failed { .. }, &AudioCmd::Stop { speak }) => {
                Some(PlaybackEvent::Stopped { speak, clause: 0, samples_played: 0 })
            }
            _ => None,
        }
    }

    /// The next event playback reports, if one waits.
    pub(super) fn next_event(&mut self) -> Option<PlaybackEvent> {
        match self {
            Self::Running(playback) => playback.next_event(),
            Self::Failed { .. } | Self::Absent => None,
        }
    }

    /// The component is stopping: the render stream stops.
    pub(super) fn finish(&mut self) {
        if let Self::Running(playback) = std::mem::replace(self, Self::Absent) {
            let Playback { stream, .. } = *playback;
            stream.stop();
        }
    }
}

/// A clause of an utterance, with a count of its 24 kHz samples.
#[derive(Debug, Clone, Copy)]
struct ClauseSamples {
    speak: SpeakId,
    clause: u16,
    samples: u32,
}

impl ClauseSamples {
    /// Whether this clause, the one whose PCM came last, means `announced` has all its audio: a
    /// later clause of the same utterance, or the same clause with all its samples.
    fn covers(self, announced: Self) -> bool {
        self.speak == announced.speak
            && (self.clause > announced.clause
                || (self.clause == announced.clause && self.samples >= announced.samples))
    }
}

/// The player and its render stream, when playback last had something to do, and the wait for
/// audio the core has been told of.
pub(super) struct Playback {
    player: Player,
    stream: Box<dyn AudioStream>,
    /// When it last had something to do; `None` before it first had.
    active_at: Option<Instant>,
    /// The clause whose PCM came last, with its samples so far.
    received: Option<ClauseSamples>,
    /// A clause whose `ClauseReady` the core has and whose PCM has not all come.
    announced: Option<ClauseSamples>,
    /// The utterance of a `Drained` held while `announced` waits.
    held: Option<SpeakId>,
    /// The utterance of a held `Drained` the core is owed, passed on before any later event.
    owed: Option<SpeakId>,
}

impl Playback {
    /// Starts the render stream of `sink`, with the renderer in its callback; `None`, logged, when
    /// the sink is at a rate playback has no conversion to or its stream does not start.
    fn start(sink: &SharedSink) -> Option<Self> {
        let mut sink = sink.lock().unwrap_or_else(PoisonError::into_inner);
        let format = sink.format();
        let (player, mut renderer) =
            match playback::playback(format, playback::default_queue(format)) {
                Ok(halves) => halves,
                Err(error) => {
                    tracing::warn!(%error, ?format, "playback did not start");
                    return None;
                }
            };
        let started =
            sink.start(Box::new(move |out: &mut [f32], info: RenderInfo| renderer.fill(out, info)));
        drop(sink);
        match started {
            Ok(stream) => Some(Self {
                player,
                stream,
                active_at: None,
                received: None,
                announced: None,
                held: None,
                owed: None,
            }),
            Err(error) => {
                tracing::warn!(%error, ?format, "playback did not start");
                None
            }
        }
    }

    /// Notes that playback has something to do now.
    fn touch(&mut self) {
        self.active_at = Some(Instant::now());
    }

    /// Whether it had something to do within [`PLAYBACK_SETTLES`].
    fn active(&self) -> bool {
        self.active_at.is_some_and(|at| at.elapsed() < PLAYBACK_SETTLES)
    }

    /// Queues a PCM frame in the player, and ends the wait for announced audio that it ends.
    fn play(&mut self, header: PcmHeader, samples: &[i16]) {
        let pushed = self.player.push(header, samples);
        self.touch();
        let PcmHeader { speak, clause, .. } = header;
        match pushed {
            // A frame with no room was logged by the player as `channel.full`.
            Ok(()) | Err(PlayError::Full { .. } | PlayError::Utterances { .. }) => {}
            // Synthesis runs ahead of playback, so clauses of a stopped utterance still come.
            Err(error @ PlayError::Ended) => tracing::debug!(
                %speak,
                clause,
                %error,
                "PCM for an utterance that takes no more audio was refused"
            ),
            Err(error @ PlayError::Rate { .. }) => tracing::warn!(
                %speak,
                clause,
                %error,
                "PCM at a rate playback does not take was refused"
            ),
        }
        // Late audio of a stopped or overtaken utterance says nothing of the newest one's.
        if pushed != Err(PlayError::Ended) {
            let before = self.received.filter(|last| last.speak == speak && last.clause == clause);
            let count = num::saturating_u32_from_usize(samples.len());
            let samples = before.map_or(0, |last| last.samples).saturating_add(count);
            self.received = Some(ClauseSamples { speak, clause, samples });
        }
        let Some(announced) = self.announced else { return };
        // Its audio has all come, a later utterance began, so it takes no more, or its own audio
        // was refused.
        let came = self.received.is_some_and(|last| last.covers(announced));
        let overtaken = pushed.is_ok() && speak != announced.speak;
        let refused = pushed.is_err() && speak == announced.speak;
        if came || overtaken || refused {
            self.release();
        }
    }

    /// Notes a `ClauseReady` passed to the core, unless its audio has all come already, as the
    /// contract orders it. One for another utterance ends the wait for the last.
    fn announce(&mut self, clause: ClauseSamples) {
        if self.received.is_some_and(|last| last.covers(clause)) {
            return;
        }
        if self.announced.is_some_and(|waiting| waiting.speak != clause.speak) {
            self.release();
        }
        self.announced = Some(clause);
    }

    /// Ends the wait for announced audio. A held `Drained` is owed to the core, unless audio
    /// queued since has made it stale, when the renderer reports the next dry spell instead.
    fn release(&mut self) {
        self.announced = None;
        let Some(speak) = self.held.take() else { return };
        if !self.player.is_stale(&PlaybackEvent::Drained { speak }) {
            self.owed = Some(speak);
            self.touch();
        }
    }

    /// Serves a playback command; false for any other. A `Stop` drops a `Drained` held for its
    /// utterance: the renderer reports `Stopped` instead, which ends the utterance in the core.
    fn apply(&mut self, cmd: &AudioCmd) -> bool {
        if !self.player.apply(cmd) {
            return false;
        }
        if let AudioCmd::Stop { speak } = *cmd
            && self.announced.is_some_and(|waiting| waiting.speak == speak)
        {
            self.announced = None;
            self.held = None;
        }
        self.touch();
        true
    }

    /// The next event playback reports, if one waits: a `Drained` the core is owed first. An
    /// event, or audio still queued, counts as something to do.
    fn next_event(&mut self) -> Option<PlaybackEvent> {
        if let Some(speak) = self.owed.take() {
            return Some(PlaybackEvent::Drained { speak });
        }
        loop {
            let event = self.player.next_event();
            if event.is_some() || self.player.has_audio() {
                self.touch();
            }
            match event? {
                PlaybackEvent::Drained { speak }
                    if self.announced.is_some_and(|waiting| waiting.speak == speak) =>
                {
                    self.held = Some(speak);
                }
                event => return Some(event),
            }
        }
    }
}
