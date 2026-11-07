//! The audio component's playback (BUILD.md section 6.6, "The core end of the speech pipe" and
//! "Duck, and what waits for W2"): the PCM a scripted stand-in for the speech process sends goes
//! straight into playback, never through the core, and plays on a 48 kHz mono `CaptureSink`; the
//! playback commands come as effects; and what playback reports reaches the core as
//! `Input::Playback`. All of it runs on paused tokio time over the memory transport, with no
//! models, no microphone and no audio device, and every render callback runs inside
//! `allocation_counter::measure`.
//!
//! The component starts the speakers' stream as it starts, at the test's time zero, so their
//! buffers start at 0, 10, 20 ms. PCM or a command that arrives at 5 ms or 25 ms takes effect
//! from the next buffer, at 10 ms or 30 ms: sample 480 or 1440 of what the speakers played.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use allocation_counter::AllocationInfo;
use eileen_audio::fake::{MemoryLimits, StandIns, memory_transport, played};
use eileen_audio::{
    AudioConfig, AudioError, AudioSink, AudioStream, CaptureSink, Pace, RenderCallback, RenderInfo,
    SharedSink, SpeechConnection, StreamFormat, WavClip,
};
use eileen_contracts::core_io::{AudioCmd, Effect, Input, PlaybackEvent, RecordPurpose};
use eileen_contracts::ids::{SpeakId, TraceCtx};
use eileen_contracts::speech::{
    Frame, FrameBody, FrameReader, PcmHeader, SpeechCmd, SpeechEventFrame, VoiceEvent,
};
use eileen_stdx::soft_assert::{self, Mode};
use tokio::time::sleep;

use crate::support::{Running, WAIT, logged, logged_in, messages, ms};

/// The speakers' rate.
const RATE: u32 = 48_000;

/// The speakers: 48 kHz mono.
const SPEAKERS: StreamFormat = StreamFormat { sample_rate_hz: RATE, channels: 1 };

/// Samples in one of the speakers' 10 ms buffers.
const BUFFER: usize = 480;

/// The samples a duck's gain moves over: the first millisecond of the next callback.
const RAMP: usize = 48;

fn speak(n: u128) -> SpeakId {
    SpeakId::from_parts(1_700_000_000_000, n)
}

/// `len` synthesized samples of a tone that starts at sample `from`.
#[expect(clippy::cast_possible_truncation, reason = "a test tone within i16")]
fn tone(from: usize, len: usize) -> Vec<i16> {
    (from..from + len)
        .map(|n| (f64::from(u32::try_from(n).unwrap()) * 0.13).sin() * 12_000.0)
        .map(|sample| sample as i16)
        .collect()
}

/// What the speakers play for these synthesized samples, as one utterance without a break.
fn heard(samples: &[i16]) -> Vec<f32> {
    played(RATE, samples).unwrap()
}

/// The bit patterns of samples, so tests compare them exactly.
fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().copied().map(f32::to_bits).collect()
}

/// The playback events among `inputs`, in order.
fn playback(inputs: &[Input]) -> Vec<PlaybackEvent> {
    inputs
        .iter()
        .filter_map(|input| if let Input::Playback(event) = input { Some(*event) } else { None })
        .collect()
}

/// The voice events among `inputs`, in order.
fn voice(inputs: &[Input]) -> Vec<VoiceEvent> {
    inputs
        .iter()
        .filter_map(
            |input| if let Input::Voice(event) = input { Some(event.clone()) } else { None },
        )
        .collect()
}

/// Render callbacks the speakers ran, and those that allocated or freed anything.
#[derive(Default)]
struct Measured {
    callbacks: AtomicU64,
    allocating: AtomicU64,
}

/// The speakers: a `CaptureSink` that writes what Eileen plays to a WAV file, with each render
/// callback the component gives it run inside `allocation_counter::measure`, on the sink's task.
struct Speakers {
    format: StreamFormat,
    sink: Option<CaptureSink>,
    measured: Arc<Measured>,
}

impl AudioSink for Speakers {
    fn format(&self) -> StreamFormat {
        self.format
    }

    fn start(&mut self, mut fill: RenderCallback) -> Result<Box<dyn AudioStream>, AudioError> {
        let measured = Arc::clone(&self.measured);
        let measuring = move |out: &mut [f32], info: RenderInfo| {
            let counted = allocation_counter::measure(|| fill(out, info));
            measured.callbacks.fetch_add(1, Ordering::Relaxed);
            if counted != AllocationInfo::default() {
                measured.allocating.fetch_add(1, Ordering::Relaxed);
            }
        };
        self.sink.as_mut().unwrap().start(Box::new(measuring))
    }
}

/// A directory of its own for one test's capture, removed when it drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("eileen-audio-it-{test}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn played(&self) -> PathBuf {
        self.0.join("played.wav")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

/// The audio component playing on the speakers, with the test's side of its transport.
struct Rig {
    running: Running,
    stand_ins: StandIns,
    speakers: Arc<Mutex<Speakers>>,
    measured: Arc<Measured>,
    scratch: Scratch,
}

impl Rig {
    fn start(test: &str) -> Self {
        Self::start_on(test, SPEAKERS)
    }

    /// The rig with speakers of `format`.
    fn start_on(test: &str, format: StreamFormat) -> Self {
        let scratch = Scratch::new(test);
        let sink = CaptureSink::create(&scratch.played(), format, Pace::RealTime).unwrap();
        let measured = Arc::new(Measured::default());
        let speakers = Speakers { format, sink: Some(sink), measured: Arc::clone(&measured) };
        let speakers = Arc::new(Mutex::new(speakers));
        let (transport, stand_ins) = memory_transport(MemoryLimits::default());
        let playback: SharedSink = Arc::<Mutex<Speakers>>::clone(&speakers);
        let config = AudioConfig {
            speech: Some(Arc::new(transport)),
            playback: Some(playback),
            ..AudioConfig::default()
        };
        let running = Running::start(&config, 1_024);
        Self { running, stand_ins, speakers, measured, scratch }
    }

    /// Connects a stand-in, as a speech process that has loaded its models.
    fn connect(&mut self) -> StandIn {
        StandIn {
            connection: self.stand_ins.connect().unwrap(),
            frames: FrameReader::for_speech(),
            unread: Vec::new(),
        }
    }

    /// Sends a playback command, as the core does, and gives the component a millisecond.
    async fn audio(&self, cmd: AudioCmd) {
        self.running.effect(Effect::Audio(cmd)).await;
    }

    async fn speech(&self, cmd: SpeechCmd) {
        self.running.effect(Effect::Speech(cmd)).await;
    }

    fn inputs(&mut self) -> Vec<Input> {
        self.running.inputs()
    }

    /// Stops the component, then the speakers, and returns every sample they played. Checks that
    /// the speakers ran the component's render callback and that no run of it allocated or freed
    /// anything.
    async fn finish(self) -> Vec<f32> {
        self.running.stop().await;
        let sink = self.speakers.lock().unwrap().sink.take().unwrap();
        sink.finish().unwrap();
        let callbacks = self.measured.callbacks.load(Ordering::Relaxed);
        let allocating = self.measured.allocating.load(Ordering::Relaxed);
        assert!(callbacks > 0, "the speakers ran the component's render callback");
        assert_eq!(allocating, 0, "no allocation or free in {callbacks} render callbacks");
        WavClip::read(&self.scratch.played()).unwrap().samples().to_vec()
    }
}

/// The scripted speech process: it reads the core's commands and sends PCM and events.
struct StandIn {
    connection: SpeechConnection,
    frames: FrameReader,
    /// Bytes read and not yet taken as frames.
    unread: Vec<u8>,
}

impl StandIn {
    /// The core's next command; `Ping`s on the way go unanswered, since the component counts no
    /// heartbeat of its own.
    async fn next_command(&mut self) -> SpeechCmd {
        let mut buf = vec![0_u8; 8_192];
        loop {
            while let Some(frame) = self.take_frame() {
                if let FrameBody::Command(command) = frame.body {
                    return command.cmd;
                }
            }
            let n = self.connection.reader().read(&mut buf, WAIT).await.unwrap();
            self.unread.extend_from_slice(&buf[..n]);
        }
    }

    /// The next whole frame among the bytes read, if they hold one.
    fn take_frame(&mut self) -> Option<Frame> {
        while !self.unread.is_empty() {
            let (taken, frame) = self.frames.feed(&self.unread).unwrap();
            self.unread.drain(..taken);
            if frame.is_some() || taken == 0 {
                return frame;
            }
        }
        None
    }

    async fn send(&mut self, frame: Frame) {
        let bytes = frame.encode().unwrap();
        self.connection.writer().write_all(&bytes, WAIT).await.unwrap();
    }

    async fn event(&mut self, event: VoiceEvent) {
        let body = SpeechEventFrame { ctx: TraceCtx::default(), span: 0, event };
        self.send(Frame::new(FrameBody::Event(body))).await;
    }

    /// One PCM frame of a clause at 24 kHz.
    async fn pcm(&mut self, speak: SpeakId, clause: u16, samples: &[i16]) {
        let header = PcmHeader { speak, clause, sample_rate: 24_000 };
        self.send(Frame::new(FrameBody::Pcm { header, samples: samples.to_vec() })).await;
    }

    /// Answers a `Speak` as the speech process does: each clause's `ClauseReady` and PCM, then
    /// `SpeakDone`.
    async fn reply(&mut self, speak: SpeakId, clauses: &[Vec<i16>]) {
        for (clause, samples) in (0_u16..).zip(clauses) {
            let samples_in = u32::try_from(samples.len()).unwrap();
            self.event(VoiceEvent::ClauseReady { speak, clause, chars: 12, samples: samples_in })
                .await;
            self.pcm(speak, clause, samples).await;
        }
        self.event(VoiceEvent::SpeakDone { speak }).await;
    }
}

#[tokio::test(start_paused = true)]
async fn the_clauses_of_a_reply_play_bit_for_bit_and_played_and_drained_reach_the_core() {
    let mut rig = Rig::start("reply");
    let mut stand_in = rig.connect();
    let one = speak(1);
    let clauses = [tone(0, 1_500), tone(1_500, 2_000)];
    sleep(ms(25)).await;
    rig.speech(SpeechCmd::Speak {
        speak: one,
        voice: "default".to_owned(),
        clauses: vec!["Opening Resolve,".to_owned(), "done.".to_owned()],
        whole_line: "Opening Resolve, done.".to_owned(),
    })
    .await;
    let asked = stand_in.next_command().await;
    assert!(matches!(asked, SpeechCmd::Speak { speak, .. } if speak == one), "{asked:?}");
    // At 26 ms, so the reply plays from the 30 ms buffer.
    stand_in.reply(one, &clauses).await;
    sleep(ms(300)).await;
    let inputs = rig.inputs();
    let speakers = rig.finish().await;

    let mut expected = vec![0.0; 3 * BUFFER];
    expected.extend(heard(&clauses.concat()));
    expected.resize(speakers.len(), 0.0);
    assert_eq!(
        bits(&speakers),
        bits(&expected),
        "silence, both clauses as one stream from the 30 ms buffer, then silence"
    );
    assert_eq!(
        playback(&inputs),
        [
            // Clause 0 is 3000 device samples, clause 1 is 4000; 2400 played is the 50 ms mark.
            PlaybackEvent::Played { speak: one, clause: 0, samples_played: 1_200 },
            // At 4800, 1800 of clause 1's device samples have played.
            PlaybackEvent::Played { speak: one, clause: 1, samples_played: 900 },
            PlaybackEvent::Drained { speak: one },
        ],
        "every 50 ms of audio, each clause counted from its start in its own samples"
    );
    assert_eq!(
        voice(&inputs),
        [
            VoiceEvent::ClauseReady { speak: one, clause: 0, chars: 12, samples: 1_500 },
            VoiceEvent::ClauseReady { speak: one, clause: 1, chars: 12, samples: 2_000 },
            VoiceEvent::SpeakDone { speak: one },
        ],
        "the speech process's events reach the core, and its PCM does not"
    );
}

#[tokio::test(start_paused = true)]
async fn a_duck_takes_effect_from_the_next_output_callback_and_an_unduck_lifts_it() {
    let mut rig = Rig::start("duck");
    let mut stand_in = rig.connect();
    let input = tone(0, 12_000);
    sleep(ms(5)).await;
    stand_in.pcm(speak(1), 0, &input).await;
    sleep(ms(20)).await;
    rig.audio(AudioCmd::Duck { db: -20 }).await;
    sleep(ms(29)).await;
    rig.audio(AudioCmd::Unduck).await;
    sleep(ms(100)).await;
    let speakers = rig.finish().await;

    // The clause plays from the 10 ms buffer; `full[k]` is what its k-th sample is at full gain.
    let full = heard(&input);
    let clause = |range: std::ops::Range<usize>| &full[range.start - BUFFER..range.end - BUFFER];
    let (at_30, at_60) = (3 * BUFFER, 6 * BUFFER);
    assert!(speakers[..BUFFER].iter().all(|s| s.to_bits() == 0), "silence before the clause");
    assert_eq!(bits(&speakers[BUFFER..at_30]), bits(clause(BUFFER..at_30)), "before the duck");
    // The duck sent at 25 ms ramps down over the first millisecond of the 30 ms buffer.
    for (k, (got, whole)) in
        speakers[at_30..at_30 + RAMP].iter().zip(clause(at_30..at_30 + RAMP)).enumerate()
    {
        assert!(got.abs() <= whole.abs() + f32::EPSILON, "the ramp only lowers, at {k}");
    }
    let gain = 10_f32.powf(-1.0);
    let ducked: Vec<f32> = clause(at_30 + RAMP..at_60).iter().map(|s| s * gain).collect();
    assert_eq!(
        bits(&speakers[at_30 + RAMP..at_60]),
        bits(&ducked),
        "20 dB down from the next output callback (invariant 2)"
    );
    // The unduck sent at 55 ms lifts it from the 60 ms buffer, after its ramp.
    let lifted = at_60 + RAMP..10 * BUFFER;
    assert_eq!(bits(&speakers[lifted.clone()]), bits(clause(lifted)), "full gain again");
}

#[tokio::test(start_paused = true)]
async fn a_pause_then_a_resume_continues_from_the_paused_sample() {
    let mut rig = Rig::start("pause");
    let mut stand_in = rig.connect();
    let one = speak(1);
    let input = tone(0, 6_000);
    sleep(ms(5)).await;
    stand_in.pcm(one, 3, &input).await;
    sleep(ms(20)).await;
    rig.audio(AudioCmd::Pause { speak: one }).await;
    sleep(ms(39)).await;
    assert_eq!(
        playback(&rig.inputs()),
        [PlaybackEvent::Paused { speak: one, clause: 3, samples_played: 480 }],
        "paused after the 10 and 20 ms buffers: 960 device samples, 480 of the clause's own"
    );
    rig.audio(AudioCmd::Resume { speak: one }).await;
    sleep(ms(400)).await;
    let inputs = rig.inputs();
    let speakers = rig.finish().await;

    // Paused through the 30, 40, 50 and 60 ms buffers; resumed from the 70 ms one.
    let full = heard(&input);
    let mut expected = vec![0.0; BUFFER];
    expected.extend(&full[..2 * BUFFER]);
    expected.extend(std::iter::repeat_n(0.0, 4 * BUFFER));
    expected.extend(&full[2 * BUFFER..]);
    expected.resize(speakers.len(), 0.0);
    assert_eq!(bits(&speakers), bits(&expected), "cut at a sample and continued from it");
    let at = |samples_played| PlaybackEvent::Played { speak: one, clause: 3, samples_played };
    assert_eq!(
        playback(&inputs),
        [
            at(1_200),
            at(2_400),
            at(3_600),
            at(4_800),
            at(6_000),
            PlaybackEvent::Drained { speak: one },
        ],
        "each 50 ms of audio counted across the pause, then the queue ran dry"
    );
}

#[test]
fn a_stop_drops_the_rest_reports_the_exact_samples_played_and_later_pcm_is_refused() {
    let one = speak(1);
    let ((inputs, speakers), captured) = logged(async {
        let mut rig = Rig::start("stop");
        let mut stand_in = rig.connect();
        sleep(ms(5)).await;
        stand_in.pcm(one, 0, &tone(0, 4_000)).await;
        stand_in.pcm(one, 1, &tone(4_000, 4_000)).await;
        sleep(ms(40)).await;
        rig.audio(AudioCmd::Stop { speak: one }).await;
        sleep(ms(14)).await;
        // Synthesis ahead of playback sends a clause the stop has already ended.
        stand_in.pcm(one, 2, &tone(8_000, 2_000)).await;
        sleep(ms(200)).await;
        let inputs = rig.inputs();
        (inputs, rig.finish().await)
    });

    assert_eq!(
        playback(&inputs),
        [PlaybackEvent::Stopped { speak: one, clause: 0, samples_played: 960 }],
        "stopped at the 50 ms buffer after 1920 device samples, 960 of the clause's own, a \
         position no Played report had given"
    );
    let mut expected = vec![0.0; BUFFER];
    expected.extend(&heard(&tone(0, 4_000))[..4 * BUFFER]);
    expected.resize(speakers.len(), 0.0);
    assert_eq!(bits(&speakers), bits(&expected), "nothing after the stop, nor the later clause");
    let refused = messages(&captured, "PCM for an utterance that takes no more audio was refused");
    assert_eq!(refused.len(), 1, "the later clause is refused and logged: {captured:?}");
    assert_eq!(refused[0].fields["clause"], 2, "with its clause");
}

#[test]
fn play_phrase_and_record_fail_a_soft_assertion_and_are_dropped_with_a_logged_reason() {
    let failed_before = soft_assert::failures();
    let ((inputs, speakers), captured) = logged_in(Mode::Report, async {
        let mut rig = Rig::start("unserved");
        let _stand_in = rig.connect();
        sleep(ms(5)).await;
        let phrase = AudioCmd::PlayPhrase { key: "launch".to_owned(), voice: "default".to_owned() };
        rig.audio(phrase).await;
        rig.audio(AudioCmd::Record { seconds: 5, purpose: RecordPurpose::FirstRun }).await;
        sleep(ms(100)).await;
        let inputs = rig.inputs();
        (inputs, rig.finish().await)
    });

    assert_eq!(soft_assert::failures() - failed_before, 2, "each fails a never! soft assertion");
    let dropped =
        messages(&captured, "an audio command the audio series does not serve was dropped");
    let commands: Vec<&str> =
        dropped.iter().map(|event| event.fields["command"].as_str().unwrap()).collect();
    assert_eq!(commands, ["PlayPhrase", "Record"], "each dropped and logged: {captured:?}");
    for event in &dropped {
        assert_eq!(event.level, "WARN", "at warn");
        assert!(
            event.fields["reason"].as_str().is_some_and(|reason| reason.contains("W2")),
            "with its reason: {event:?}"
        );
    }
    assert!(inputs.is_empty(), "nothing reaches the core: {inputs:?}");
    assert!(speakers.iter().all(|s| s.to_bits() == 0), "and nothing plays");
}

#[test]
fn speakers_at_a_rate_playback_cannot_convert_to_play_nothing_and_say_why() {
    let one = speak(1);
    let ((callbacks, inputs), captured) = logged(async {
        let format = StreamFormat { sample_rate_hz: 96_000, channels: 2 };
        let mut rig = Rig::start_on("unconverted", format);
        let mut stand_in = rig.connect();
        sleep(ms(5)).await;
        stand_in.pcm(one, 0, &tone(0, 1_000)).await;
        rig.audio(AudioCmd::Stop { speak: one }).await;
        sleep(ms(100)).await;
        let inputs = rig.inputs();
        rig.running.stop().await;
        (rig.measured.callbacks.load(Ordering::Relaxed), inputs)
    });

    assert_eq!(callbacks, 0, "no render stream was started");
    let refused = messages(&captured, "playback did not start");
    assert_eq!(refused.len(), 1, "the refusal is logged once: {captured:?}");
    assert_eq!(refused[0].level, "WARN", "at warn");
    assert_eq!(refused[0].fields["error"], "no playback at 96000 Hz", "with its reason");
    let dropped = messages(&captured, "there is no playback; PCM dropped");
    assert_eq!(dropped.len(), 1, "the PCM is dropped and logged: {captured:?}");
    assert!(playback(&inputs).is_empty(), "and nothing is reported: {inputs:?}");
}
