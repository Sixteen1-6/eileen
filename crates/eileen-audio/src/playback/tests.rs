//! The playback queue's rules, each over a `CaptureSink` on paused tokio time, so what the
//! renderer hands the device is read back from the WAV file it wrote, and the render callback's
//! code inside `allocation_counter::measure`.

use std::path::Path;
use std::time::Duration;

use allocation_counter::AllocationInfo;
use eileen_contracts::core_io::{AudioCmd, PlaybackEvent};
use eileen_contracts::ids::SpeakId;
use eileen_contracts::speech::{PCM_BODY_MAX, PCM_HEADER_MAX, PcmHeader};
use eileen_trace::names;
use tokio::time::sleep;

use super::{EVENTS, MARKERS, PlayError, Player, Renderer, SYNTH_RATE, default_queue, playback};
use crate::pace::Pace;
use crate::stream::{AudioSink as _, AudioStream, RenderInfo, StreamFormat};
use crate::testing::{BUFFER, MONO_48K, Scratch, bits, ms};
use crate::wav::{CaptureSink, WavClip};

/// The queue the tests give a player: 2 s at 48 kHz.
const QUEUE: usize = 96_000;

fn speak(n: u128) -> SpeakId {
    SpeakId::from_parts(1_700_000_000_000, n)
}

fn header(speak: SpeakId, clause: u16) -> PcmHeader {
    PcmHeader { speak, clause, sample_rate: SYNTH_RATE }
}

/// `len` synthesized samples of a tone that starts at sample `from`.
#[expect(clippy::cast_possible_truncation, reason = "a test tone within i16")]
fn tone(from: usize, len: usize) -> Vec<i16> {
    (from..from + len)
        .map(|n| (f64::from(u32::try_from(n).unwrap()) * 0.13).sin() * 12_000.0)
        .map(|sample| sample as i16)
        .collect()
}

/// What the renderer should hand a 48 kHz device for `input`, played without a break: the same
/// interpolator over the whole input in one call, which gives the same samples bit for bit as
/// any cut into buffers.
fn upsampled(input: &[i16]) -> Vec<f32> {
    super::rendered(48_000, input).unwrap()
}

/// A player and a `CaptureSink` running its renderer from time zero.
struct Rig {
    player: Player,
    sink: CaptureSink,
    stream: Box<dyn AudioStream>,
    _dir: Scratch,
    path: std::path::PathBuf,
}

impl Rig {
    fn start(test: &str) -> Self {
        let dir = Scratch::new(test);
        let path = dir.join("played.wav");
        let (player, mut renderer) = playback(MONO_48K, QUEUE).unwrap();
        let mut sink = CaptureSink::create(&path, MONO_48K, Pace::RealTime).unwrap();
        let stream = sink
            .start(Box::new(move |out: &mut [f32], info: RenderInfo| renderer.fill(out, info)))
            .unwrap();
        Self { player, sink, stream, _dir: dir, path }
    }

    /// Stops the sink and returns every sample it wrote.
    fn finish(self) -> (Player, Vec<f32>) {
        self.stream.stop();
        self.sink.finish().unwrap();
        (self.player, read(&self.path))
    }
}

fn read(path: &Path) -> Vec<f32> {
    WavClip::read(path).unwrap().samples().to_vec()
}

fn events(player: &mut Player) -> Vec<PlaybackEvent> {
    std::iter::from_fn(|| player.next_event()).collect()
}

/// Buffers start at 0, 10, 20 ms; a command given at 25 ms applies from the 30 ms buffer, the
/// 1440th sample.
const AT_30_MS: usize = 3 * BUFFER;

#[tokio::test(start_paused = true)]
async fn pcm_plays_upsampled_from_the_next_buffer_and_silence_after_it() {
    let input = tone(0, 2_000);
    let mut rig = Rig::start("plays");
    sleep(ms(25)).await;
    rig.player.push(header(speak(1), 0), &input).unwrap();
    sleep(ms(200)).await;
    let (_, played) = rig.finish();

    let mut expected = vec![0.0; AT_30_MS];
    expected.extend(upsampled(&input));
    expected.resize(played.len(), 0.0);
    assert_eq!(bits(&played), bits(&expected), "silence, the clause at twice its rate, silence");
}

#[tokio::test(start_paused = true)]
async fn a_duck_applies_at_the_next_callback_and_an_unduck_lifts_it() {
    let input = tone(0, 12_000);
    let mut rig = Rig::start("duck");
    rig.player.push(header(speak(1), 0), &input).unwrap();
    sleep(ms(25)).await;
    assert!(rig.player.apply(&AudioCmd::Duck { db: -20 }), "a playback command");
    sleep(ms(30)).await;
    assert!(rig.player.apply(&AudioCmd::Unduck), "a playback command");
    sleep(ms(100)).await;
    let (_, played) = rig.finish();

    let full = upsampled(&input);
    let ramp = super::ramp_frames(48_000);
    let gain = 10_f32.powf(-1.0);
    // The 0 to 20 ms buffers play at full gain, exactly.
    assert_eq!(bits(&played[..AT_30_MS]), bits(&full[..AT_30_MS]), "before the duck");
    // The 30 ms buffer ramps down over its first millisecond, then holds the duck.
    let ramped = AT_30_MS..AT_30_MS + ramp;
    for (heard, whole) in played[ramped.clone()].iter().zip(&full[ramped]) {
        assert!(heard.abs() <= whole.abs() + f32::EPSILON, "the ramp only lowers");
    }
    let ducked = AT_30_MS + ramp..6 * BUFFER;
    let expected: Vec<f32> = full[ducked.clone()].iter().map(|s| s * gain).collect();
    assert_eq!(bits(&played[ducked]), bits(&expected), "20 dB down from the next callback on");
    // The unduck at 55 ms applies from the 60 ms buffer, after its ramp.
    let lifted = 6 * BUFFER + ramp..10 * BUFFER;
    assert_eq!(bits(&played[lifted.clone()]), bits(&full[lifted]), "full gain again");
}

#[tokio::test(start_paused = true)]
async fn a_pause_stops_at_a_sample_and_resume_starts_at_exactly_that_sample() {
    let input = tone(0, 6_000);
    let mut rig = Rig::start("pause");
    let one = speak(1);
    rig.player.push(header(one, 3), &input).unwrap();
    sleep(ms(25)).await;
    assert!(rig.player.apply(&AudioCmd::Pause { speak: one }), "a playback command");
    sleep(ms(40)).await;
    assert_eq!(
        events(&mut rig.player),
        [PlaybackEvent::Paused { speak: one, clause: 3, samples_played: 720 }],
        "paused after 1440 device samples, 720 of the clause's own"
    );
    assert!(rig.player.apply(&AudioCmd::Resume { speak: one }), "a playback command");
    sleep(ms(300)).await;
    let (_, played) = rig.finish();

    // Paused through the 30, 40, 50 and 60 ms buffers; resumed from the 70 ms one.
    let full = upsampled(&input);
    let gap = 4 * BUFFER;
    let mut expected = full[..AT_30_MS].to_vec();
    expected.extend(std::iter::repeat_n(0.0, gap));
    expected.extend(&full[AT_30_MS..]);
    expected.resize(played.len(), 0.0);
    assert_eq!(bits(&played), bits(&expected), "the clause cut at a sample and continued from it");
}

#[tokio::test(start_paused = true)]
async fn a_stop_drops_the_utterance_and_the_next_one_plays() {
    let mut rig = Rig::start("stop");
    let (one, two) = (speak(1), speak(2));
    rig.player.push(header(one, 0), &tone(0, 4_000)).unwrap();
    rig.player.push(header(one, 1), &tone(4_000, 4_000)).unwrap();
    sleep(ms(25)).await;
    assert!(rig.player.apply(&AudioCmd::Stop { speak: one }), "a playback command");
    assert_eq!(
        rig.player.push(header(one, 2), &tone(8_000, 100)),
        Err(PlayError::Ended),
        "a stopped utterance takes no more audio"
    );
    sleep(ms(20)).await;
    assert_eq!(
        events(&mut rig.player),
        [PlaybackEvent::Stopped { speak: one, clause: 0, samples_played: 720 }],
        "stopped in its first clause"
    );
    let next = tone(0, 1_000);
    rig.player.push(header(two, 0), &next).unwrap();
    sleep(ms(100)).await;
    let (_, played) = rig.finish();

    // Silent from the 30 ms buffer until the second utterance starts with the 50 ms one.
    let mut expected = upsampled(&tone(0, 4_000))[..AT_30_MS].to_vec();
    expected.resize(5 * BUFFER, 0.0);
    expected.extend(upsampled(&next));
    expected.resize(played.len(), 0.0);
    assert_eq!(bits(&played), bits(&expected), "nothing more of the stopped utterance");
}

#[tokio::test(start_paused = true)]
async fn played_reports_each_clause_every_50_ms_and_drained_comes_when_the_queue_runs_dry() {
    let mut rig = Rig::start("played");
    let one = speak(1);
    // Clause 0 holds 3000 device samples, clause 1 holds 4000.
    rig.player.push(header(one, 0), &tone(0, 1_500)).unwrap();
    rig.player.push(header(one, 1), &tone(1_500, 2_000)).unwrap();
    sleep(ms(200)).await;
    let (mut player, _) = rig.finish();
    assert_eq!(
        events(&mut player),
        [
            // After 2400 device samples, the 50 ms boundary, in clause 0.
            PlaybackEvent::Played { speak: one, clause: 0, samples_played: 1_200 },
            // After 4800: clause 1 began at 3000, so 1800 of its samples are in.
            PlaybackEvent::Played { speak: one, clause: 1, samples_played: 900 },
            PlaybackEvent::Drained { speak: one },
        ],
        "at most every 50 ms, each clause counted from its start"
    );
}

#[tokio::test(start_paused = true)]
async fn a_reply_in_two_batches_plays_both_and_drains_after_each() {
    // One reply, one SpeakId: the speech process synthesizes the first batch of clauses and
    // reports `SpeakDone` for it, which reaches no player call, and the second batch comes
    // after the first has played out.
    let mut rig = Rig::start("batches");
    let one = speak(1);
    let input = tone(0, 2_000);
    rig.player.push(header(one, 0), &input[..1_000]).unwrap();
    sleep(ms(65)).await;
    assert_eq!(
        events(&mut rig.player),
        [PlaybackEvent::Drained { speak: one }],
        "the first batch's 2000 device samples played out in the 40 ms buffer"
    );
    rig.player.push(header(one, 1), &input[1_000..]).unwrap();
    sleep(ms(100)).await;
    assert_eq!(
        events(&mut rig.player),
        [
            // 2000 samples of clause 0 and 480 of clause 1 cross the 50 ms mark.
            PlaybackEvent::Played { speak: one, clause: 1, samples_played: 240 },
            PlaybackEvent::Drained { speak: one },
        ],
        "the second batch plays and drains again"
    );
    let (_, played) = rig.finish();

    // The second batch continues the first's interpolator from the 70 ms buffer.
    let full = upsampled(&input);
    let mut expected = full[..2_000].to_vec();
    expected.resize(7 * BUFFER, 0.0);
    expected.extend(&full[2_000..]);
    expected.resize(played.len(), 0.0);
    assert_eq!(bits(&played), bits(&expected), "both batches, a gap between them");
}

#[tokio::test(start_paused = true)]
async fn a_paused_utterance_holds_the_next_until_it_stops() {
    let mut rig = Rig::start("pause-holds");
    let (one, two) = (speak(1), speak(2));
    let first = tone(0, 4_000);
    let next = tone(0, 1_000);
    rig.player.push(header(one, 0), &first).unwrap();
    rig.player.push(header(two, 0), &next).unwrap();
    sleep(ms(25)).await;
    assert!(rig.player.apply(&AudioCmd::Pause { speak: one }), "a playback command");
    sleep(ms(60)).await;
    assert_eq!(
        events(&mut rig.player),
        [PlaybackEvent::Paused { speak: one, clause: 0, samples_played: 720 }],
        "the first paused, and nothing of the second played"
    );
    assert!(rig.player.apply(&AudioCmd::Stop { speak: one }), "a playback command");
    sleep(ms(100)).await;
    assert_eq!(
        events(&mut rig.player),
        [
            PlaybackEvent::Stopped { speak: one, clause: 0, samples_played: 720 },
            // 1440 samples of the first and 960 of the second cross the 50 ms mark.
            PlaybackEvent::Played { speak: two, clause: 0, samples_played: 480 },
            PlaybackEvent::Drained { speak: two },
        ],
        "stopped where it paused, then the second played out"
    );
    let (_, played) = rig.finish();

    // Silent from the 30 ms buffer while paused; the stop at 85 ms lets the second play from
    // the 90 ms buffer.
    let mut expected = upsampled(&first)[..AT_30_MS].to_vec();
    expected.resize(9 * BUFFER, 0.0);
    expected.extend(upsampled(&next));
    expected.resize(played.len(), 0.0);
    assert_eq!(bits(&played), bits(&expected), "nobody talks over the paused utterance");
}

#[tokio::test(start_paused = true)]
async fn a_pause_before_any_audio_holds_the_utterance_until_it_resumes() {
    let mut rig = Rig::start("early-pause");
    let one = speak(1);
    assert!(rig.player.apply(&AudioCmd::Pause { speak: one }), "a playback command");
    let input = tone(0, 1_000);
    rig.player.push(header(one, 0), &input).unwrap();
    sleep(ms(25)).await;
    assert_eq!(
        events(&mut rig.player),
        [PlaybackEvent::Paused { speak: one, clause: 0, samples_played: 0 }],
        "held at its first sample"
    );
    assert!(rig.player.apply(&AudioCmd::Resume { speak: one }), "a playback command");
    sleep(ms(100)).await;
    let (_, played) = rig.finish();

    let mut expected = vec![0.0; AT_30_MS];
    expected.extend(upsampled(&input));
    expected.resize(played.len(), 0.0);
    assert_eq!(bits(&played), bits(&expected), "it starts at the buffer after the resume");
}

#[tokio::test(start_paused = true)]
async fn a_stop_before_any_audio_reports_nothing_played_and_refuses_its_audio() {
    let (mut player, _renderer) = playback(MONO_48K, QUEUE).unwrap();
    let one = speak(1);
    assert!(player.apply(&AudioCmd::Stop { speak: one }), "a playback command");
    assert_eq!(
        events(&mut player),
        [PlaybackEvent::Stopped { speak: one, clause: 0, samples_played: 0 }],
        "nothing of it played"
    );
    assert_eq!(player.push(header(one, 0), &tone(0, 10)), Err(PlayError::Ended), "refused");
}

#[test]
fn audio_of_an_utterance_after_a_later_one_began_is_refused() {
    let (mut player, _renderer) = playback(MONO_48K, QUEUE).unwrap();
    player.push(header(speak(1), 0), &tone(0, 10)).unwrap();
    player.push(header(speak(2), 0), &tone(0, 10)).unwrap();
    assert_eq!(
        player.push(header(speak(1), 1), &tone(10, 10)),
        Err(PlayError::Ended),
        "utterances play in order, each whole"
    );
}

#[test]
fn pcm_at_another_rate_and_commands_of_other_parts_are_not_taken() {
    let (mut player, _renderer) = playback(MONO_48K, QUEUE).unwrap();
    let pcm = PcmHeader { speak: speak(1), clause: 0, sample_rate: 22_050 };
    assert_eq!(
        player.push(pcm, &tone(0, 10)),
        Err(PlayError::Rate { sample_rate: 22_050 }),
        "synthesis comes at 24 kHz"
    );
    assert!(!player.apply(&AudioCmd::SetCapture(false)), "capture is not playback's");
}

#[test]
fn a_device_rate_without_a_conversion_is_refused() {
    let format = StreamFormat { sample_rate_hz: 96_000, channels: 2 };
    assert!(
        matches!(playback(format, QUEUE), Err(PlayError::Rate { sample_rate: 96_000 })),
        "no 24 to 96 kHz conversion"
    );
}

#[test]
fn a_full_queue_refuses_the_frame_and_logs_the_drop() {
    let (mut player, _renderer) = playback(MONO_48K, 4_800).unwrap();
    let one = speak(1);
    player.push(header(one, 0), &tone(0, 2_000)).unwrap();
    let (refused, captured) =
        eileen_trace::test_support::capture(|| player.push(header(one, 1), &tone(2_000, 500)));
    assert_eq!(refused, Err(PlayError::Full { samples: 1_000 }), "4000 queued, 1000 do not fit");
    let logged = captured.events_named(names::CHANNEL_FULL);
    assert_eq!(logged.len(), 1, "one drop logged: {captured:?}");
    let fields = &logged[0].fields;
    assert_eq!(fields["channel"], "audio.playback", "the queue named");
    assert_eq!(fields["capacity"], 4_800, "its capacity");
    assert_eq!(fields["times"], 1, "the first drop");
    // Room for what fits is still taken.
    player.push(header(one, 1), &tone(2_000, 300)).unwrap();
}

#[test]
fn each_queue_counts_its_own_drops() {
    let (mut player, _renderer) = playback(MONO_48K, 4_800).unwrap();
    let one = speak(1);
    player.push(header(one, 0), &tone(0, 2_000)).unwrap();
    for n in 100..116 {
        player.pause(speak(n));
    }
    let ((), captured) = eileen_trace::test_support::capture(|| {
        assert!(player.push(header(one, 1), &tone(0, 500)).is_err(), "the samples do not fit");
        player.pause(speak(116));
        assert!(player.push(header(one, 1), &tone(0, 500)).is_err(), "still no room");
    });
    let logged = captured.events_named(names::CHANNEL_FULL);
    let expected = [("audio.playback", 1), ("audio.playback.waiting", 1), ("audio.playback", 2)];
    assert_eq!(logged.len(), expected.len(), "three drops logged: {captured:?}");
    for (event, (channel, times)) in logged.iter().zip(expected) {
        assert_eq!(event.fields["channel"], channel, "the channel named");
        assert_eq!(event.fields["times"], times, "a drop on one channel does not count another's");
    }
}

#[test]
fn a_full_marker_queue_refuses_the_frame_and_logs_it() {
    let (mut player, _renderer) = playback(MONO_48K, QUEUE).unwrap();
    let one = speak(1);
    for clause in 0..MARKERS {
        player.push(header(one, u16::try_from(clause).unwrap()), &tone(0, 1)).unwrap();
    }
    let (refused, captured) =
        eileen_trace::test_support::capture(|| player.push(header(one, 0), &tone(0, 1)));
    assert_eq!(refused, Err(PlayError::Full { samples: 2 }), "a marker per frame, and none left");
    let logged = captured.events_named(names::CHANNEL_FULL);
    assert_eq!(logged.len(), 1, "the drop logged: {captured:?}");
    assert_eq!(logged[0].fields["channel"], "audio.playback.markers", "the marker queue named");
    assert_eq!(logged[0].fields["capacity"], MARKERS, "its size, not the sample queue's");
    assert_eq!(logged[0].fields["times"], 1, "its first drop");
}

#[test]
fn an_utterance_past_the_live_limit_is_refused_and_logged() {
    let (mut player, _renderer) = playback(MONO_48K, QUEUE).unwrap();
    // The renderer has reached none, so seven may wait.
    for n in 1..=7 {
        player.push(header(speak(n), 0), &tone(0, 10)).unwrap();
    }
    let (refused, captured) =
        eileen_trace::test_support::capture(|| player.push(header(speak(8), 0), &tone(0, 10)));
    assert_eq!(refused, Err(PlayError::Utterances { live: 7 }), "an eighth does not fit");
    let logged = captured.events_named(names::CHANNEL_FULL);
    assert_eq!(logged.len(), 1, "the drop logged: {captured:?}");
    assert_eq!(logged[0].fields["channel"], "audio.playback.utterances", "the limit named");
}

#[test]
fn the_default_queue_holds_the_largest_pcm_frame() {
    let largest = (usize::try_from(PCM_BODY_MAX).unwrap() - PCM_HEADER_MAX) / 2;
    for rate in [48_000, 44_100] {
        let format = StreamFormat { sample_rate_hz: rate, channels: 2 };
        let (mut player, _renderer) = playback(format, default_queue(format)).unwrap();
        player.push(header(speak(1), 0), &vec![0; largest]).unwrap();
    }
}

#[test]
fn a_stereo_device_hears_each_sample_on_both_channels() {
    let format = StreamFormat { sample_rate_hz: 48_000, channels: 2 };
    let (mut player, mut renderer) = playback(format, QUEUE).unwrap();
    let input = tone(0, 240);
    player.push(header(speak(1), 0), &input).unwrap();
    let mut out = vec![1.0; 2 * BUFFER];
    renderer.fill(&mut out, RenderInfo { plays_at: Duration::ZERO });
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    let right: Vec<f32> = out.iter().skip(1).step_by(2).copied().collect();
    assert_eq!(bits(&left), bits(&upsampled(&input)), "the left channel");
    assert_eq!(bits(&right), bits(&left), "the right channel the same");
}

/// Fills one buffer inside `allocation_counter::measure` and asserts it allocated nothing.
fn fill_measured(renderer: &mut Renderer, out: &mut [f32], what: &str) {
    let counted =
        allocation_counter::measure(|| renderer.fill(out, RenderInfo { plays_at: Duration::ZERO }));
    assert_eq!(counted, AllocationInfo::default(), "no allocation or free while {what}");
}

#[test]
fn a_late_command_for_a_played_out_utterance_leaves_the_one_in_its_slot_alone() {
    let (mut player, mut renderer) = playback(MONO_48K, QUEUE).unwrap();
    let mut out = vec![0.0; BUFFER];
    // The first utterance plays out and the renderer reaches the second, which runs dry.
    player.push(header(speak(1), 0), &tone(0, 10)).unwrap();
    player.push(header(speak(2), 0), &tone(0, 10)).unwrap();
    fill_measured(&mut renderer, &mut out, "moving to the second utterance");
    // Seven more, the last in the first one's hold slot; each drains when the next begins, the
    // second's dry spell is reported once, and the last drains when the queue runs dry.
    for n in 3..=9 {
        player.push(header(speak(n), 0), &tone(0, 10)).unwrap();
    }
    player.pause(speak(1));
    player.stop(speak(1));
    fill_measured(&mut renderer, &mut out, "moving from one utterance to the next");
    let drained: Vec<PlaybackEvent> =
        (1..=9).map(|n| PlaybackEvent::Drained { speak: speak(n) }).collect();
    assert_eq!(events(&mut player), drained, "every utterance played out, the ninth untouched");
}

#[test]
fn played_comes_at_most_once_per_50_ms_with_long_buffers() {
    let (mut player, mut renderer) = playback(MONO_48K, QUEUE).unwrap();
    let one = speak(1);
    // Clause 0 holds 4900 device samples, so the second 100 ms buffer crosses into clause 1.
    player.push(header(one, 0), &tone(0, 2_450)).unwrap();
    player.push(header(one, 1), &tone(2_450, 5_000)).unwrap();
    let mut out = vec![0.0; 10 * BUFFER];
    fill_measured(&mut renderer, &mut out, "a 100 ms buffer");
    fill_measured(&mut renderer, &mut out, "a 100 ms buffer across a clause");
    assert_eq!(
        events(&mut player),
        [
            PlaybackEvent::Played { speak: one, clause: 0, samples_played: 2_400 },
            PlaybackEvent::Played { speak: one, clause: 1, samples_played: 2_350 },
        ],
        "one report per buffer, however long"
    );
}

#[test]
fn a_duck_ramp_carries_over_buffers_shorter_than_it() {
    let (mut player, mut renderer) = playback(MONO_48K, QUEUE).unwrap();
    let input = tone(0, 1_000);
    player.push(header(speak(1), 0), &input).unwrap();
    let mut out = vec![0.0; BUFFER];
    renderer.fill(&mut out, RenderInfo { plays_at: Duration::ZERO });
    player.duck(-20);
    let mut out_samples = Vec::new();
    let mut short = [0.0; 16];
    for _ in 0..4 {
        fill_measured(&mut renderer, &mut short, "ramping over 16-sample buffers");
        out_samples.extend(short);
    }
    let full = &upsampled(&input)[BUFFER..BUFFER + 64];
    let target = 10_f32.powf(-1.0);
    let ramp = super::ramp_frames(48_000);
    for (k, (heard, whole)) in out_samples.iter().zip(full).enumerate().take(ramp - 1) {
        if whole.abs() > 1e-3 {
            let gain = heard / whole;
            assert!(gain > target + 1e-4 && gain < 1.0, "still ramping at {k}: {gain}");
        }
    }
    let after: Vec<f32> = full[ramp - 1..].iter().map(|s| s * target).collect();
    assert_eq!(
        bits(&out_samples[ramp - 1..]),
        bits(&after),
        "the duck in full from the ramp's end"
    );
}

#[test]
fn early_pauses_past_their_limit_are_logged() {
    let (mut player, _renderer) = playback(MONO_48K, QUEUE).unwrap();
    for n in 0..16 {
        player.pause(speak(n));
    }
    let ((), captured) = eileen_trace::test_support::capture(|| player.pause(speak(16)));
    let logged = captured.events_named(names::CHANNEL_FULL);
    assert_eq!(logged.len(), 1, "the dropped pause logged: {captured:?}");
    assert_eq!(logged[0].fields["channel"], "audio.playback.waiting", "the limit named");
    assert_eq!(logged[0].fields["capacity"], 16, "its size");
}

#[test]
fn the_render_callback_allocates_and_frees_nothing() {
    let (mut player, mut renderer) = playback(MONO_48K, QUEUE).unwrap();
    let (one, two) = (speak(1), speak(2));
    player.push(header(one, 0), &tone(0, 6_000)).unwrap();
    player.push(header(one, 1), &tone(6_000, 6_000)).unwrap();
    player.push(header(two, 0), &tone(0, 6_000)).unwrap();
    let mut out = vec![0.0; BUFFER];
    fill_measured(&mut renderer, &mut out, "playing");
    player.duck(-20);
    fill_measured(&mut renderer, &mut out, "ramping to a duck");
    for _ in 0..10 {
        fill_measured(&mut renderer, &mut out, "reporting Played");
    }
    player.pause(one);
    fill_measured(&mut renderer, &mut out, "pausing");
    fill_measured(&mut renderer, &mut out, "paused");
    player.resume(one);
    fill_measured(&mut renderer, &mut out, "resuming");
    player.pause(one);
    fill_measured(&mut renderer, &mut out, "pausing again");
    player.stop(one);
    fill_measured(&mut renderer, &mut out, "dropping a paused, stopped utterance");
    for _ in 0..100 {
        fill_measured(&mut renderer, &mut out, "draining and running dry");
    }
    // Played every 2400 samples handed, counted across the pauses; the stop drops the rest of
    // the first utterance, and the second plays out and reports its dry spell.
    assert_eq!(
        events(&mut player),
        [
            PlaybackEvent::Played { speak: one, clause: 0, samples_played: 1_200 },
            PlaybackEvent::Played { speak: one, clause: 0, samples_played: 2_400 },
            PlaybackEvent::Paused { speak: one, clause: 0, samples_played: 2_880 },
            PlaybackEvent::Paused { speak: one, clause: 0, samples_played: 3_120 },
            PlaybackEvent::Stopped { speak: one, clause: 0, samples_played: 3_120 },
            PlaybackEvent::Played { speak: two, clause: 0, samples_played: 480 },
            PlaybackEvent::Played { speak: two, clause: 0, samples_played: 1_680 },
            PlaybackEvent::Played { speak: two, clause: 0, samples_played: 2_880 },
            PlaybackEvent::Played { speak: two, clause: 0, samples_played: 4_080 },
            PlaybackEvent::Played { speak: two, clause: 0, samples_played: 5_280 },
            PlaybackEvent::Drained { speak: two },
        ],
        "every path above ran"
    );
}

#[test]
fn a_full_event_queue_counts_what_it_lost_and_logs_it() {
    let (mut player, mut renderer) = super::playback_with(MONO_48K, QUEUE, 2).unwrap();
    for n in 0..4 {
        let utterance = speak(n);
        player.push(header(utterance, 0), &tone(0, 10)).unwrap();
    }
    let mut out = vec![0.0; BUFFER];
    fill_measured(&mut renderer, &mut out, "losing events");
    let (reported, captured) = eileen_trace::test_support::capture(|| events(&mut player));
    assert_eq!(
        reported,
        [PlaybackEvent::Drained { speak: speak(0) }, PlaybackEvent::Drained { speak: speak(1) }],
        "the two that fit"
    );
    let logged = captured.events_named(names::CHANNEL_FULL);
    assert_eq!(logged.len(), 1, "the loss logged: {captured:?}");
    assert_eq!(logged[0].fields["channel"], "audio.playback.events", "the event queue named");
    assert_eq!(logged[0].fields["times"], 2, "two lost");
}

#[test]
fn stops_before_any_audio_past_the_event_limit_are_logged() {
    let (mut player, _renderer) = playback(MONO_48K, QUEUE).unwrap();
    let limit = u128::try_from(EVENTS).unwrap();
    for n in 0..limit {
        player.stop(speak(n));
    }
    let ((), captured) = eileen_trace::test_support::capture(|| player.stop(speak(limit)));
    let logged = captured.events_named(names::CHANNEL_FULL);
    assert_eq!(logged.len(), 1, "the dropped report logged: {captured:?}");
    assert_eq!(logged[0].fields["channel"], "audio.playback.reported", "the list named");
    assert_eq!(logged[0].fields["capacity"], EVENTS, "its size");
    assert_eq!(logged[0].fields["times"], 1, "its first drop");
    assert_eq!(player.reported.len(), EVENTS, "the list holds no more than its size");
}

/// Fills `count` buffers of `BUFFER` samples.
fn fill_buffers(renderer: &mut Renderer, count: usize) {
    let mut out = vec![0.0; BUFFER];
    for _ in 0..count {
        renderer.fill(&mut out, RenderInfo { plays_at: Duration::ZERO });
    }
}

#[test]
fn a_drained_made_stale_by_later_audio_is_not_passed_on() {
    let (mut player, mut renderer) = playback(MONO_48K, QUEUE).unwrap();
    let one = speak(1);
    player.push(header(one, 0), &tone(0, 100)).unwrap();
    // The renderer runs dry and reports `Drained`, then the next batch's audio is queued before
    // the component reads it, as when the batch's `ClauseReady` reaches the core first.
    fill_buffers(&mut renderer, 1);
    player.push(header(one, 1), &tone(100, 100)).unwrap();
    assert!(events(&mut player).is_empty(), "the stale Drained is dropped while audio waits");
    fill_buffers(&mut renderer, 1);
    assert_eq!(
        events(&mut player),
        [PlaybackEvent::Drained { speak: one }],
        "the next dry spell is reported once the later audio has played"
    );
}

#[test]
fn a_second_pause_while_dry_is_reported() {
    let (mut player, mut renderer) = playback(MONO_48K, QUEUE).unwrap();
    let one = speak(1);
    player.push(header(one, 0), &tone(0, 100)).unwrap();
    fill_buffers(&mut renderer, 1);
    player.pause(one);
    fill_buffers(&mut renderer, 1);
    player.resume(one);
    fill_buffers(&mut renderer, 1);
    player.pause(one);
    fill_buffers(&mut renderer, 1);
    assert_eq!(
        events(&mut player),
        [
            PlaybackEvent::Drained { speak: one },
            PlaybackEvent::Paused { speak: one, clause: 0, samples_played: 100 },
            PlaybackEvent::Paused { speak: one, clause: 0, samples_played: 100 },
        ],
        "each pause reported, though nothing played between them"
    );
}
