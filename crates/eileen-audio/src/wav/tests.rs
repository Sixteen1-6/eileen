use std::io;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};
use tokio::time::sleep;

use super::{CaptureSink, WavClip, WavSource};
use crate::pace::Pace;
use crate::stream::{
    AudioError, AudioSink as _, AudioSource as _, RenderInfo, SetupError, StreamFormat,
};
use crate::testing::{BUFFER, MONO_48K, Scratch, bits, ms, ramp, recorder, samples, times};

/// A clip of `len` ramp samples at 48 kHz mono.
fn ramp_clip(len: usize) -> WavClip {
    WavClip::new(MONO_48K, (0..len).map(ramp).collect()).unwrap()
}

/// The buffer times every 10 ms from `from_ms`, `count` of them.
fn every_10_ms(from_ms: u64, count: u64) -> Vec<Duration> {
    (0..count).map(|k| ms(from_ms + 10 * k)).collect()
}

#[tokio::test(start_paused = true)]
async fn a_capture_sink_and_a_wav_source_round_trip_a_float_wav() {
    let dir = Scratch::new("round-trip");
    let path = dir.join("said.wav");
    let mut sink = CaptureSink::create(&path, MONO_48K, Pace::RealTime).unwrap();
    let played = Arc::new(Mutex::new(Vec::new()));
    let plays_at = Arc::clone(&played);
    let mut next = 0;
    let stream = sink
        .start(Box::new(move |buffer: &mut [f32], info: RenderInfo| {
            for sample in &mut *buffer {
                *sample = ramp(next);
                next += 1;
            }
            plays_at.lock().unwrap().push(info.plays_at);
        }))
        .unwrap();
    // Buffers fall due at 0, 10, ..., 90 ms.
    sleep(ms(95)).await;
    stream.stop();
    assert_eq!(sink.finish().unwrap(), 4_800, "ten 10 ms buffers written");
    assert_eq!(*played.lock().unwrap(), every_10_ms(0, 10), "each buffer's time on the sink");

    let expected: Vec<f32> = (0..4_800).map(ramp).collect();
    let clip = WavClip::read(&path).unwrap();
    assert_eq!(clip.format(), MONO_48K, "the file's format");
    assert_eq!(bits(clip.samples()), bits(&expected), "every sample, exactly");
    assert_eq!(clip.duration(), ms(100), "its length");

    let mut source = WavSource::open(&path, Pace::RealTime).unwrap();
    let (callback, heard) = recorder();
    let stream = source.start(callback).unwrap();
    sleep(ms(95)).await;
    stream.stop();
    assert_eq!(times(&heard), every_10_ms(0, 10), "each buffer's time on the source");
    assert_eq!(bits(&samples(&heard)), bits(&expected), "the source gives back the same samples");
}

#[test]
fn integer_wavs_read_at_full_scale() {
    let dir = Scratch::new("integers");
    let write = |name: &str, bits_per_sample: u16, values: &[i32]| {
        let path = dir.join(name);
        let spec = WavSpec {
            channels: 2,
            sample_rate: 16_000,
            bits_per_sample,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for &value in values {
            writer.write_sample(value).unwrap();
        }
        writer.finalize().unwrap();
        WavClip::read(&path).unwrap()
    };

    let sixteen = write("16.wav", 16, &[-32_768, -16_384, 0, 16_384, 32_767, 1]);
    let format = StreamFormat { sample_rate_hz: 16_000, channels: 2 };
    assert_eq!(sixteen.format(), format, "the file's rate and channels");
    let expected: [f32; 6] = [-1.0, -0.5, 0.0, 0.5, 32_767.0 / 32_768.0, 1.0 / 32_768.0];
    assert_eq!(bits(sixteen.samples()), bits(&expected), "16-bit samples over 32 768");

    let twenty_four = write("24.wav", 24, &[-8_388_608, 4_194_304]);
    assert_eq!(bits(twenty_four.samples()), bits(&[-1.0_f32, 0.5]), "24-bit samples over 2^23");

    let eight = write("8.wav", 8, &[-128, 64, 0, 127]);
    let expected: [f32; 4] = [-1.0, 0.5, 0.0, 127.0 / 128.0];
    assert_eq!(bits(eight.samples()), bits(&expected), "8-bit samples over 128");

    let thirty_two = write("32.wav", 32, &[i32::MIN, 1 << 30, -1, i32::MAX]);
    let least = 0.5_f32.powi(31);
    let expected: [f32; 4] = [-1.0, 0.5, -least, 1.0];
    assert_eq!(bits(thirty_two.samples()), bits(&expected), "32-bit samples over 2^31, to f32");
}

/// Writes `frames` frames of silence as 16-bit integers at this rate and channel count.
fn silent_wav(path: &Path, sample_rate: u32, channels: u16, frames: usize) {
    let spec =
        WavSpec { channels, sample_rate, bits_per_sample: 16, sample_format: SampleFormat::Int };
    let mut writer = WavWriter::create(path, spec).unwrap();
    for _ in 0..frames * usize::from(channels) {
        writer.write_sample(0_i16).unwrap();
    }
    writer.finalize().unwrap();
}

/// The setup error a read fails with, if it fails with one.
fn read_setup_error(path: &Path) -> Option<SetupError> {
    match WavClip::read(path) {
        Err(AudioError::Setup(error)) => Some(error),
        _ => None,
    }
}

#[test]
fn a_clip_over_20_minutes_is_refused_however_few_its_samples() {
    let dir = Scratch::new("long");
    // 100 Hz keeps the files small: 20 minutes are 120 000 samples.
    let path = dir.join("1200s.wav");
    silent_wav(&path, 100, 1, 120_000);
    assert_eq!(WavClip::read(&path).unwrap().duration(), ms(1_200_000), "20 minutes are read");
    let path = dir.join("1201s.wav");
    silent_wav(&path, 100, 1, 120_100);
    assert_eq!(read_setup_error(&path), Some(SetupError::ClipTooLong), "a second more is not");
}

/// A WAV file's bytes: a header for `channels` channels of `bits`-bit samples at `rate` in the
/// format `tag` (1 integer, 3 float), claiming `data_len` bytes of samples, then `data`.
fn handmade_wav(
    tag: u16,
    channels: u16,
    rate: u32,
    bits: u16,
    data_len: u32,
    data: &[u8],
) -> Vec<u8> {
    let block_align = channels * (bits / 8);
    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data_len).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16_u32.to_le_bytes());
    file.extend_from_slice(&tag.to_le_bytes());
    file.extend_from_slice(&channels.to_le_bytes());
    file.extend_from_slice(&rate.to_le_bytes());
    file.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    file.extend_from_slice(&block_align.to_le_bytes());
    file.extend_from_slice(&bits.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data_len.to_le_bytes());
    file.extend_from_slice(data);
    file
}

#[test]
fn a_header_claiming_more_samples_than_the_cap_is_refused_before_reading() {
    let dir = Scratch::new("claim");
    let path = dir.join("claim.wav");
    // Two minutes of 8 channels at 192 kHz: within 20 minutes, but 184 million samples, more
    // than 20 minutes of 48 kHz stereo. The file holds only the header.
    let data_len = 2 * 60 * 192_000 * 8 * 4;
    std::fs::write(&path, handmade_wav(3, 8, 192_000, 32, data_len, &[])).unwrap();
    assert_eq!(read_setup_error(&path), Some(SetupError::ClipTooLong), "refused by its claim");
}

#[test]
fn a_wav_in_a_format_no_test_clip_has_is_refused() {
    let dir = Scratch::new("formats");
    let unsupported = |name: &str, sample_rate: u32, channels: u16| {
        let path = dir.join(name);
        silent_wav(&path, sample_rate, channels, 10);
        let format = StreamFormat { sample_rate_hz: sample_rate, channels };
        matches!(WavClip::read(&path), Err(AudioError::Unsupported(f)) if f == format)
    };
    assert!(unsupported("nine.wav", 48_000, 9), "more than 8 channels");
    assert!(unsupported("11025.wav", 11_025, 1), "a rate without whole 10 ms buffers");
    assert!(unsupported("384k.wav", 384_000, 1), "a rate over 192 kHz");
    assert!(!unsupported("eight.wav", 192_000, 8), "8 channels at 192 kHz are read");

    let path = dir.join("f64.wav");
    std::fs::write(&path, handmade_wav(3, 1, 48_000, 64, 8, &0.5_f64.to_le_bytes())).unwrap();
    let refused = WavClip::read(&path);
    assert!(
        matches!(refused, Err(AudioError::Io(e)) if e.kind() == io::ErrorKind::InvalidData),
        "64-bit floats are not read"
    );
}

#[tokio::test(start_paused = true)]
async fn the_source_paces_one_buffer_per_10_ms_of_the_paused_clock() {
    let clip = ramp_clip(48_000);
    let mut source = WavSource::new(clip.clone(), Pace::RealTime).unwrap();
    let (callback, heard) = recorder();
    let stream = source.start(callback).unwrap();

    sleep(ms(25)).await;
    assert_eq!(times(&heard), every_10_ms(0, 3), "the buffers due by 25 ms, and no more");
    sleep(ms(10)).await;
    assert_eq!(times(&heard), every_10_ms(0, 4), "one more by 35 ms");
    stream.stop();

    let expected = clip.samples().get(..4 * BUFFER).unwrap();
    assert_eq!(bits(&samples(&heard)), bits(expected), "the clip's samples in order");
}

#[tokio::test(start_paused = true)]
async fn a_faster_pace_runs_more_buffers_with_the_same_times() {
    let mut source =
        WavSource::new(ramp_clip(48_000), Pace::Faster(NonZeroU32::new(4).unwrap())).unwrap();
    let (callback, heard) = recorder();
    let stream = source.start(callback).unwrap();
    // Four times faster: a buffer every 2.5 ms, so ten by 24 ms. Tokio's timer rounds a deadline
    // up to the next millisecond, so the tenth, due at 22.5 ms, runs at 23 ms and the eleventh at
    // 25 ms.
    sleep(ms(24)).await;
    stream.stop();
    assert_eq!(times(&heard), every_10_ms(0, 10), "times count audio, not the pace");
}

#[tokio::test(start_paused = true)]
async fn a_stopped_source_calls_nothing_and_a_restart_skips_what_played_meanwhile() {
    let clip = ramp_clip(48_000);
    let mut source = WavSource::new(clip.clone(), Pace::RealTime).unwrap();
    let (callback, heard) = recorder();
    let stream = source.start(callback).unwrap();
    sleep(ms(25)).await;
    stream.stop();
    sleep(ms(50)).await;
    assert_eq!(times(&heard), every_10_ms(0, 3), "nothing after the stop");

    // Started again at 75 ms: the next buffer due is the one at 80 ms.
    let (callback, heard) = recorder();
    let stream = source.start(callback).unwrap();
    sleep(ms(10)).await;
    drop(stream);
    sleep(ms(30)).await;
    assert_eq!(times(&heard), vec![ms(80)], "one buffer, at 80 ms, and none after the drop");
    let expected = clip.samples().get(80 * 48..80 * 48 + BUFFER).unwrap();
    assert_eq!(bits(&samples(&heard)), bits(expected), "the clip at 80 ms");
}

#[tokio::test(start_paused = true)]
async fn the_source_captures_silence_after_its_clip() {
    let clip = WavClip::new(MONO_48K, vec![0.25; 720]).unwrap();
    let mut source = WavSource::new(clip, Pace::RealTime).unwrap();
    let (callback, heard) = recorder();
    let stream = source.start(callback).unwrap();
    sleep(ms(35)).await;
    stream.stop();
    let mut expected = vec![0.25; 720];
    expected.resize(4 * BUFFER, 0.0);
    assert_eq!(bits(&samples(&heard)), bits(&expected), "15 ms of clip, then silence");
}

#[tokio::test(start_paused = true)]
async fn the_capture_keeps_the_sinks_timeline_across_a_pause() {
    let dir = Scratch::new("pause");
    let path = dir.join("said.wav");
    let mut sink = CaptureSink::create(&path, MONO_48K, Pace::RealTime).unwrap();
    let ones = || Box::new(|buffer: &mut [f32], _: RenderInfo| buffer.fill(1.0));
    let stream = sink.start(ones()).unwrap();
    sleep(ms(15)).await;
    stream.stop();
    sleep(ms(20)).await;
    // Started again at 35 ms: the next buffer is the one at 40 ms.
    let stream = sink.start(ones()).unwrap();
    sleep(ms(12)).await;
    stream.stop();
    assert_eq!(sink.finish().unwrap(), 2_400, "50 ms of file");

    let mut expected = vec![1.0; 960];
    expected.resize(1_920, 0.0);
    expected.resize(2_400, 1.0);
    let clip = WavClip::read(&path).unwrap();
    assert_eq!(bits(clip.samples()), bits(&expected), "the pause is silence in the file");
}

#[tokio::test(start_paused = true)]
async fn a_second_stream_and_a_rate_without_whole_10_ms_buffers_are_refused() {
    let mut source = WavSource::new(ramp_clip(480), Pace::RealTime).unwrap();
    let (callback, _heard) = recorder();
    let _stream = source.start(callback).unwrap();
    let (callback, _heard) = recorder();
    assert!(
        matches!(source.start(callback), Err(AudioError::Setup(SetupError::AlreadyRunning))),
        "one stream at a time"
    );

    let odd = StreamFormat { sample_rate_hz: 11_025, channels: 1 };
    let clip = WavClip::new(odd, vec![0.0; 10]).unwrap();
    assert!(
        matches!(WavSource::new(clip, Pace::RealTime), Err(AudioError::Unsupported(f)) if f == odd),
        "11 025 Hz does not split into 10 ms buffers"
    );
    assert!(
        matches!(
            WavClip::new(StreamFormat { channels: 2, ..MONO_48K }, vec![0.0; 3]),
            Err(AudioError::Setup(SetupError::PartialFrame))
        ),
        "a clip holds whole frames"
    );
}

#[test]
fn a_stream_needs_a_tokio_runtime() {
    let mut source = WavSource::new(ramp_clip(480), Pace::RealTime).unwrap();
    let (callback, _heard) = recorder();
    assert!(
        matches!(source.start(callback), Err(AudioError::Setup(SetupError::NoRuntime))),
        "no runtime, no stream, and no panic"
    );
}

#[tokio::test(start_paused = true)]
async fn a_capture_stops_at_its_budget_and_finish_reports_it() {
    let dir = Scratch::new("budget");
    let path = dir.join("said.wav");
    // Room for two and a half buffers: the third would cross the budget.
    let mut sink = CaptureSink::create_with(&path, MONO_48K, Pace::RealTime, 1_200).unwrap();
    let ones = Box::new(|buffer: &mut [f32], _: RenderInfo| buffer.fill(1.0));
    let stream = sink.start(ones).unwrap();
    sleep(ms(45)).await;
    stream.stop();
    assert!(
        matches!(sink.finish(), Err(AudioError::Setup(SetupError::CaptureTooLong))),
        "the capture reports that it ran out"
    );
    let clip = WavClip::read(&path).unwrap();
    assert_eq!(bits(clip.samples()), bits(&[1.0; 2 * BUFFER]), "the two whole buffers it took");
}

#[tokio::test(start_paused = true)]
async fn a_stopped_capture_writes_silence_until_it_is_finished() {
    let dir = Scratch::new("tail");
    let path = dir.join("said.wav");
    let mut sink = CaptureSink::create(&path, MONO_48K, Pace::RealTime).unwrap();
    let ones = Box::new(|buffer: &mut [f32], _: RenderInfo| buffer.fill(1.0));
    let stream = sink.start(ones).unwrap();
    sleep(ms(5)).await;
    stream.stop();
    sleep(ms(20)).await;
    // The buffers at 10 and 20 ms played silence.
    assert_eq!(sink.finish().unwrap(), 3 * 480, "30 ms of file");
    let mut expected = vec![1.0; BUFFER];
    expected.resize(3 * BUFFER, 0.0);
    let clip = WavClip::read(&path).unwrap();
    assert_eq!(bits(clip.samples()), bits(&expected), "one buffer played, then silence");
}
