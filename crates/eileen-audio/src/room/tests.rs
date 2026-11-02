use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::sleep;

use super::{Bed, RoomConfig, RoomMixer};
use crate::pace::Pace;
use crate::stream::{
    AudioError, AudioSink as _, AudioSource as _, RenderCallback, RenderInfo, StreamFormat,
};
use crate::testing::{BUFFER, MONO_48K, bits, ms, ramp, recorder, samples, times};
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

/// A speaker callback that plays one impulse of 1.0 at `frame`, and records each buffer's time.
fn impulse_at(frame: usize) -> (RenderCallback, Arc<Mutex<Vec<Duration>>>) {
    let played = Arc::new(Mutex::new(Vec::new()));
    let plays_at = Arc::clone(&played);
    let mut next = 0;
    let fill: RenderCallback = Box::new(move |buffer: &mut [f32], info: RenderInfo| {
        for sample in &mut *buffer {
            *sample = if next == frame { 1.0 } else { 0.0 };
            next += 1;
        }
        plays_at.lock().unwrap().push(info.plays_at);
    });
    (fill, played)
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

    let mut expected = vec![0.0; 10 * BUFFER];
    expected[100 + 1_776] = 0.25;
    assert_eq!(bits(&samples(&heard)), bits(&expected), "one echo, 37 ms late and 12 dB down");
    let ticks: Vec<Duration> = (0..10).map(|k| ms(10 * k)).collect();
    assert_eq!(times(&heard), ticks, "the microphone's buffers every 10 ms");
    assert_eq!(*played.lock().unwrap(), ticks, "the speaker's buffers on the same clock");
}

#[tokio::test(start_paused = true)]
async fn an_echo_without_delay_arrives_in_the_same_buffer() {
    let mixer = RoomMixer::new(room(Duration::ZERO, 0.5)).unwrap();
    let (fill, _played) = impulse_at(7);
    let (callback, heard) = recorder();
    let _speaker = mixer.sink().start(fill).unwrap();
    let _microphone = mixer.source().start(callback).unwrap();
    sleep(ms(5)).await;
    let mut expected = vec![0.0; BUFFER];
    expected[7] = 0.5;
    assert_eq!(
        bits(&samples(&heard)),
        bits(&expected),
        "the speaker plays before the room is heard"
    );
}

#[tokio::test(start_paused = true)]
async fn the_room_adds_the_near_talker_and_the_looped_bed() {
    let near = WavClip::new(MONO_48K, vec![0.5; 720]).unwrap();
    let bed = WavClip::new(MONO_48K, (0..1_000).map(ramp).collect()).unwrap();
    let config = RoomConfig {
        near: Some(near),
        bed: Some(Bed { clip: bed.clone(), gain: 0.5 }),
        ..room(ms(20), 0.25)
    };
    let mixer = RoomMixer::new(config).unwrap();
    let (callback, heard) = recorder();
    let microphone = mixer.source().start(callback).unwrap();
    sleep(ms(25)).await;
    microphone.stop();

    // The speaker never started, so it played silence and nothing echoes.
    let expected: Vec<f32> = (0..3 * BUFFER)
        .map(|n| {
            let near = if n < 720 { 0.5 } else { 0.0 };
            let bed = *bed.samples().get(n % 1_000).unwrap();
            near + 0.5 * bed
        })
        .collect();
    assert_eq!(bits(&samples(&heard)), bits(&expected), "the talker once, the bed looped");
}

#[tokio::test(start_paused = true)]
async fn each_half_stops_alone_and_the_room_runs_on() {
    let mixer = RoomMixer::new(room(ms(10), 1.0)).unwrap();
    let (fill, played) = impulse_at(usize::MAX);
    let (callback, heard) = recorder();
    let speaker = mixer.sink().start(fill).unwrap();
    let microphone = mixer.source().start(callback).unwrap();
    sleep(ms(25)).await;
    speaker.stop();
    sleep(ms(20)).await;
    assert_eq!(played.lock().unwrap().len(), 3, "the speaker stopped after three buffers");
    assert_eq!(times(&heard).len(), 5, "the microphone went on");

    let (fill, _played) = impulse_at(usize::MAX);
    let mut sink = mixer.sink();
    let _again = sink.start(fill).unwrap();
    assert!(
        matches!(sink.start(impulse_at(0).0), Err(AudioError::Setup(_))),
        "one speaker stream at a time"
    );
    microphone.stop();
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
    assert!(
        matches!(RoomMixer::new(room(ms(10), f32::NAN)), Err(AudioError::Setup(_))),
        "a gain that is not a number"
    );
    assert!(
        matches!(RoomMixer::new(room(ms(10), -0.5)), Err(AudioError::Setup(_))),
        "a negative gain"
    );
    assert!(
        matches!(RoomMixer::new(room(ms(1_001), 0.1)), Err(AudioError::Setup(_))),
        "an echo over a second late"
    );
}
