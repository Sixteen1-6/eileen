use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::sleep;

use super::{Bed, BedSource, RoomConfig, RoomMixer};
use crate::pace::Pace;
use crate::stream::{
    AudioError, AudioSink as _, AudioSource as _, RenderCallback, RenderInfo, SetupError,
    StreamFormat,
};
use crate::testing::{BUFFER, Heard, MONO_48K, bits, ms, ramp, recorder, samples, times};
use crate::wav::WavClip;

/// A quiet room at 48 kHz mono with this echo.
fn room(echo_delay: Duration, echo_gain: f32) -> RoomConfig {
    RoomConfig {
        format: MONO_48K,
        pace: Pace::RealTime,
        echo_delay,
        echo_gain,
        near: None,
        bed: None,
    }
}

/// A speaker callback that plays one impulse of 1.0 at interleaved sample `at`, counted from its
/// stream's first buffer, and records each buffer's time.
fn impulse_at(at: usize) -> (RenderCallback, Arc<Mutex<Vec<Duration>>>) {
    let played = Arc::new(Mutex::new(Vec::new()));
    let plays_at = Arc::clone(&played);
    let mut next = 0;
    let fill: RenderCallback = Box::new(move |buffer: &mut [f32], info: RenderInfo| {
        for sample in &mut *buffer {
            *sample = if next == at { 1.0 } else { 0.0 };
            next += 1;
        }
        plays_at.lock().unwrap().push(info.plays_at);
    });
    (fill, played)
}

/// A speaker callback that plays silence.
fn silence() -> RenderCallback {
    impulse_at(usize::MAX).0
}

/// `len` samples of silence with `value` at each of `at`.
fn with_at(len: usize, at: &[(usize, f32)]) -> Vec<f32> {
    let mut samples = vec![0.0; len];
    for &(n, value) in at {
        samples[n] = value;
    }
    samples
}

/// The buffer times every 10 ms from `from_ms`, `count` of them.
fn every_10_ms(from_ms: u64, count: u64) -> Vec<Duration> {
    (0..count).map(|k| ms(from_ms + 10 * k)).collect()
}

/// A bed of a ramp `len` samples long at 48 kHz mono.
fn ramp_bed(len: usize, gain: f32, from: BedSource) -> Bed {
    Bed { clip: WavClip::new(MONO_48K, (0..len).map(ramp).collect()).unwrap(), gain, from }
}

#[tokio::test(start_paused = true)]
async fn the_echo_is_the_playback_delayed_and_attenuated() {
    // 37 ms is 1 776 frames at 48 kHz, so the echo lands in the fourth buffer, not on a boundary.
    let mixer = RoomMixer::new(room(ms(37), 0.25)).unwrap();
    let (fill, played) = impulse_at(100);
    let (callback, heard) = recorder();
    let speaker = mixer.sink().start(fill).unwrap();
    let microphone = mixer.source().start(callback).unwrap();
    sleep(ms(95)).await;
    microphone.stop();
    speaker.stop();

    let expected = with_at(10 * BUFFER, &[(100 + 1_776, 0.25)]);
    assert_eq!(bits(&samples(&heard)), bits(&expected), "one echo, 37 ms late and 12 dB down");
    assert_eq!(times(&heard), every_10_ms(0, 10), "the microphone's buffers every 10 ms");
    assert_eq!(*played.lock().unwrap(), every_10_ms(0, 10), "the speaker's on the same clock");
}

#[tokio::test(start_paused = true)]
async fn the_loopback_carries_the_playback_in_the_same_buffer_at_full_gain() {
    let mixer = RoomMixer::new(room(ms(37), 0.25)).unwrap();
    let (fill, played) = impulse_at(100);
    let (callback, heard) = recorder();
    let (looped_back, loopback) = recorder();
    let _loopback = mixer.loopback().start(looped_back).unwrap();
    let _speaker = mixer.sink().start(fill).unwrap();
    let _microphone = mixer.source().start(callback).unwrap();
    sleep(ms(45)).await;

    let expected = with_at(5 * BUFFER, &[(100, 1.0)]);
    assert_eq!(bits(&samples(&loopback)), bits(&expected), "the impulse as played, undelayed");
    let expected = with_at(5 * BUFFER, &[(100 + 1_776, 0.25)]);
    assert_eq!(bits(&samples(&heard)), bits(&expected), "and its echo at the microphone");
    let ticks = every_10_ms(0, 5);
    assert_eq!(times(&loopback), ticks, "the loopback's buffers");
    assert_eq!(*played.lock().unwrap(), ticks, "the speaker's, at the same times");
    assert_eq!(times(&heard), ticks, "the microphone's, at the same times");
}

#[tokio::test(start_paused = true)]
async fn a_bed_from_the_speakers_is_in_the_loopback_and_echoes_at_the_microphone() {
    let clip = WavClip::new(MONO_48K, with_at(2_000, &[(50, 1.0)])).unwrap();
    let config = RoomConfig {
        bed: Some(Bed { clip, gain: 0.5, from: BedSource::Speakers }),
        ..room(ms(10), 0.25)
    };
    let mixer = RoomMixer::new(config).unwrap();
    let (callback, heard) = recorder();
    let (looped_back, loopback) = recorder();
    let _loopback = mixer.loopback().start(looped_back).unwrap();
    let _microphone = mixer.source().start(callback).unwrap();
    sleep(ms(55)).await;

    // No speaker stream runs, so the PC plays the bed alone, looped every 2 000 samples.
    let expected = with_at(6 * BUFFER, &[(50, 0.5), (2_050, 0.5)]);
    assert_eq!(bits(&samples(&loopback)), bits(&expected), "the bed at its gain, looped");
    let expected = with_at(6 * BUFFER, &[(50 + 480, 0.125), (2_050 + 480, 0.125)]);
    assert_eq!(
        bits(&samples(&heard)),
        bits(&expected),
        "its echo, 10 ms late and a quarter as loud"
    );
}

#[tokio::test(start_paused = true)]
async fn an_echo_without_delay_arrives_in_the_same_buffer() {
    let mixer = RoomMixer::new(room(Duration::ZERO, 0.5)).unwrap();
    let (fill, _played) = impulse_at(7);
    let (callback, heard) = recorder();
    let _speaker = mixer.sink().start(fill).unwrap();
    let _microphone = mixer.source().start(callback).unwrap();
    sleep(ms(5)).await;
    let expected = with_at(BUFFER, &[(7, 0.5)]);
    assert_eq!(
        bits(&samples(&heard)),
        bits(&expected),
        "the speaker plays before the room is heard"
    );
}

#[tokio::test(start_paused = true)]
async fn the_room_adds_the_near_talker_and_a_looped_bed_in_the_room() {
    let near = WavClip::new(MONO_48K, vec![0.5; 720]).unwrap();
    let bed = ramp_bed(1_000, 0.5, BedSource::Room);
    let config = RoomConfig { near: Some(near), bed: Some(bed.clone()), ..room(ms(20), 0.25) };
    let mixer = RoomMixer::new(config).unwrap();
    let (callback, heard) = recorder();
    let (looped_back, loopback) = recorder();
    let microphone = mixer.source().start(callback).unwrap();
    let loopback_stream = mixer.loopback().start(looped_back).unwrap();
    sleep(ms(25)).await;
    microphone.stop();
    loopback_stream.stop();

    // The speaker never started and the bed is not the PC's, so nothing echoes.
    let expected: Vec<f32> = (0..3 * BUFFER)
        .map(|n| {
            let near = if n < 720 { 0.5 } else { 0.0 };
            near + 0.5 * bed.clip.samples()[n % 1_000]
        })
        .collect();
    assert_eq!(bits(&samples(&heard)), bits(&expected), "the talker once, the bed looped");
    assert_eq!(bits(&samples(&loopback)), bits(&[0.0; 3 * BUFFER]), "the PC played nothing");
}

#[tokio::test(start_paused = true)]
async fn a_stereo_echo_stays_in_its_channel() {
    let stereo = StreamFormat { channels: 2, ..MONO_48K };
    let mixer = RoomMixer::new(RoomConfig { format: stereo, ..room(ms(37), 0.25) }).unwrap();
    // Frame 100's right channel.
    let (fill, _played) = impulse_at(2 * 100 + 1);
    let (callback, heard) = recorder();
    let _speaker = mixer.sink().start(fill).unwrap();
    let _microphone = mixer.source().start(callback).unwrap();
    sleep(ms(45)).await;
    let expected = with_at(5 * 2 * BUFFER, &[(2 * (100 + 1_776) + 1, 0.25)]);
    assert_eq!(bits(&samples(&heard)), bits(&expected), "1 776 frames late, still on the right");
}

#[tokio::test(start_paused = true)]
async fn each_stream_stops_alone_and_the_room_runs_on() {
    let mixer = RoomMixer::new(room(ms(10), 1.0)).unwrap();
    // In the speaker's third buffer, 5 ms before it stops.
    let (fill, played) = impulse_at(960);
    let (callback, heard) = recorder();
    let speaker = mixer.sink().start(fill).unwrap();
    let mut source = mixer.source();
    let microphone = source.start(callback).unwrap();
    sleep(ms(25)).await;
    speaker.stop();
    sleep(ms(20)).await;
    assert_eq!(played.lock().unwrap().len(), 3, "the speaker stopped after three buffers");
    assert_eq!(times(&heard), every_10_ms(0, 5), "the microphone went on");
    let expected = with_at(5 * BUFFER, &[(960 + 480, 1.0)]);
    assert_eq!(bits(&samples(&heard)), bits(&expected), "the last echo arrived after the stop");

    let mut sink = mixer.sink();
    let _again = sink.start(silence()).unwrap();
    let mut loopback = mixer.loopback();
    let _loopback = loopback.start(recorder().0).unwrap();
    let refused = |result: Result<_, AudioError>| {
        matches!(result, Err(AudioError::Setup(SetupError::AlreadyRunning)))
    };
    assert!(refused(sink.start(silence())), "one speaker stream at a time");
    assert!(refused(source.start(recorder().0)), "one microphone stream at a time");
    assert!(refused(loopback.start(recorder().0)), "one loopback stream at a time");
    microphone.stop();
}

/// Plays an impulse at frame 100 into a room with a 30 ms echo and a bed, stops every stream at
/// 15 ms, before the echo is due at 32 ms, and starts them again at `restart_ms`. Returns what
/// the microphone and the loopback heard after the restart, over 20 ms.
async fn restart_at(restart_ms: u64, bed: Bed) -> (Heard, Heard) {
    let mixer = RoomMixer::new(RoomConfig { bed: Some(bed), ..room(ms(30), 0.5) }).unwrap();
    let speaker = mixer.sink().start(impulse_at(100).0).unwrap();
    let microphone = mixer.source().start(recorder().0).unwrap();
    sleep(ms(15)).await;
    speaker.stop();
    microphone.stop();
    sleep(ms(restart_ms - 15)).await;

    let (callback, heard) = recorder();
    let (looped_back, loopback) = recorder();
    let _speaker = mixer.sink().start(silence()).unwrap();
    let _microphone = mixer.source().start(callback).unwrap();
    let _loopback = mixer.loopback().start(looped_back).unwrap();
    sleep(ms(20)).await;
    (heard, loopback)
}

#[tokio::test(start_paused = true)]
async fn a_restarted_room_keeps_an_echo_still_due_and_the_beds_place() {
    // Started again at 25 ms: the next buffer is the one at 30 ms, frame 1 440, and the impulse's
    // echo, due at frame 1 540, is still on its way. The bed in the room plays on meanwhile.
    let bed = ramp_bed(1_000, 0.25, BedSource::Room);
    let (heard, _loopback) = restart_at(25, bed.clone()).await;
    assert_eq!(times(&heard), every_10_ms(30, 2), "the microphone takes up at 30 ms");
    let expected: Vec<f32> = (0..2 * BUFFER)
        .map(|n| {
            let mut sample = 0.0;
            sample += 0.25 * bed.clip.samples()[(1_440 + n) % 1_000];
            sample += 0.5 * if n == 100 { 1.0 } else { 0.0 };
            sample
        })
        .collect();
    assert_eq!(
        bits(&samples(&heard)),
        bits(&expected),
        "the echo at frame 1 540, the bed in place"
    );
}

#[tokio::test(start_paused = true)]
async fn a_restarted_room_drops_an_echo_that_fell_due_while_it_was_stopped() {
    // Started again at 45 ms: the next buffer is the one at 50 ms, and the echo fell due at 32 ms.
    let bed = ramp_bed(1_000, 0.0, BedSource::Room);
    let (heard, _loopback) = restart_at(45, bed).await;
    assert_eq!(times(&heard), every_10_ms(50, 2), "the microphone takes up at 50 ms");
    assert_eq!(bits(&samples(&heard)), bits(&[0.0; 2 * BUFFER]), "nothing left to hear");
}

#[tokio::test(start_paused = true)]
async fn a_restarted_room_echoes_the_bed_the_speakers_played_while_it_was_stopped() {
    // Started again at 45 ms: the echo line holds what the speakers played from 20 ms to 50 ms,
    // the bed alone, which no buffer ran.
    let bed = ramp_bed(1_000, 1.0, BedSource::Speakers);
    let (heard, loopback) = restart_at(45, bed.clone()).await;
    let ramp_from = |from: usize| -> Vec<f32> {
        (0..2 * BUFFER).map(|n| bed.clip.samples()[(from + n) % 1_000]).collect()
    };
    let echoed: Vec<f32> = ramp_from(960).into_iter().map(|sample| 0.5 * sample).collect();
    assert_eq!(bits(&samples(&heard)), bits(&echoed), "the bed from 20 ms, 30 ms late");
    assert_eq!(bits(&samples(&loopback)), bits(&ramp_from(2_400)), "the bed at 50 ms, as played");
}

#[test]
fn a_room_refuses_a_clip_in_another_format_and_a_bad_gain() {
    let stereo = StreamFormat { channels: 2, ..MONO_48K };
    let clip = WavClip::new(stereo, vec![0.0; 4]).unwrap();
    let config = RoomConfig { near: Some(clip), ..room(ms(10), 0.1) };
    assert!(
        matches!(RoomMixer::new(config), Err(AudioError::Unsupported(f)) if f == stereo),
        "the talker's clip must be in the room's format"
    );
    let setup = |config: RoomConfig| match RoomMixer::new(config) {
        Err(AudioError::Setup(error)) => Some(error),
        _ => None,
    };
    assert_eq!(setup(room(ms(10), f32::NAN)), Some(SetupError::BadGain), "a gain not a number");
    assert_eq!(setup(room(ms(10), -0.5)), Some(SetupError::BadGain), "a negative gain");
    let bed = ramp_bed(10, f32::INFINITY, BedSource::Speakers);
    let config = RoomConfig { bed: Some(bed), ..room(ms(10), 0.1) };
    assert_eq!(setup(config), Some(SetupError::BadGain), "a bed's infinite gain");
    assert_eq!(setup(room(ms(1_001), 0.1)), Some(SetupError::EchoTooLong), "an echo over 1 s");
}
