use allocation_counter::AllocationInfo;
use realfft::RealFftPlanner;

use super::{Conversion, ResampleError, Resampler, downmix};

/// The test tones' amplitude.
const AMPLITUDE: f64 = 0.5;
/// The measured spectra's bin width, and the step between test tones' frequencies a multiple of
/// it: every tone falls on a bin and fills the window with whole periods, so a rectangular
/// window measures it with no leakage.
const BIN_HZ: u32 = 10;
/// The step between test tones.
const TONE_STEP_HZ: u32 = 100;
/// More input samples than any filter's history, fed before the measured window.
const SETTLE: usize = 256;

/// `len` samples of a sine at `hz`, sampled at `rate_hz`.
fn sine(hz: u32, rate_hz: u32, len: usize) -> Vec<f32> {
    let step = std::f64::consts::TAU * f64::from(hz) / f64::from(rate_hz);
    (0..u32::try_from(len).unwrap())
        .map(|n| narrow(AMPLITUDE * (step * f64::from(n)).sin()))
        .collect()
}

#[expect(clippy::cast_possible_truncation, reason = "a test signal within f32's range")]
const fn narrow(value: f64) -> f32 {
    value as f32
}

/// Feeds a fresh resampler a tone at `hz` and returns the spectrum of its steady output, one
/// tenth of a second long: each bin's amplitude relative to the tone's, bin `k` at `k * BIN_HZ`.
fn response(conversion: Conversion, hz: u32) -> Vec<f64> {
    let window_in = usize::try_from(conversion.input_hz() / BIN_HZ).unwrap();
    let window_out = usize::try_from(conversion.output_hz() / BIN_HZ).unwrap();
    let input = sine(hz, conversion.input_hz(), window_in + SETTLE);
    let mut resampler = Resampler::new(conversion);
    let mut output = vec![0.0; resampler.output_len(input.len())];
    let written = resampler.process(&input, &mut output).unwrap();
    assert!(written >= window_out, "{conversion:?}: {written} samples, too few to measure");

    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(window_out);
    let mut signal: Vec<f64> =
        output[written - window_out..written].iter().copied().map(f64::from).collect();
    let mut spectrum = fft.make_output_vec();
    fft.process(&mut signal, &mut spectrum).unwrap();
    let scale = 2.0 / (f64::from(conversion.output_hz() / BIN_HZ) * AMPLITUDE);
    spectrum.iter().map(|bin| bin.norm() * scale).collect()
}

fn bin(hz: u32) -> usize {
    usize::try_from(hz / BIN_HZ).unwrap()
}

fn db(amplitude: f64) -> f64 {
    20.0 * amplitude.max(1e-12).log10()
}

/// The test tones below half the input rate.
fn tones(conversion: Conversion) -> impl Iterator<Item = u32> {
    let nyquist = conversion.input_hz() / 2;
    (1..).map(|n| n * TONE_STEP_HZ).take_while(move |&hz| hz < nyquist)
}

/// The bar for capture (BUILD.md section 6.6): flat within 0.1 dB up to 7 kHz, and every tone
/// from 9 kHz up at least 60 dB down wherever it lands in the output, so nothing aliases.
fn assert_capture_bar(conversion: Conversion) {
    let mut worst_flat: (f64, u32) = (0.0, 0);
    let mut worst_alias: (f64, u32) = (f64::NEG_INFINITY, 0);
    for hz in tones(conversion) {
        let spectrum = response(conversion, hz);
        if hz <= 7_000 {
            let gain = db(spectrum[bin(hz)]);
            if gain.abs() > worst_flat.0.abs() {
                worst_flat = (gain, hz);
            }
        }
        if hz >= 9_000 {
            let loudest = db(spectrum.iter().copied().fold(0.0, f64::max));
            if loudest > worst_alias.0 {
                worst_alias = (loudest, hz);
            }
        }
    }
    assert!(
        worst_flat.0.abs() <= 0.1,
        "{conversion:?}: {:.4} dB at {} Hz, outside 0.1 dB of flat",
        worst_flat.0,
        worst_flat.1
    );
    assert!(
        worst_alias.0 <= -60.0,
        "{conversion:?}: a tone at {} Hz comes through at {:.1} dB",
        worst_alias.1,
        worst_alias.0
    );
}

/// The bar for synthesis (BUILD.md section 6.6): flat within 0.1 dB up to 10 kHz, and nothing
/// any tone makes at 14 kHz or above comes within 60 dB of it, so its images are that far down.
fn assert_synthesis_bar(conversion: Conversion) {
    let mut worst_flat: (f64, u32) = (0.0, 0);
    let mut worst_image: (f64, u32) = (f64::NEG_INFINITY, 0);
    for hz in tones(conversion) {
        let spectrum = response(conversion, hz);
        if hz <= 10_000 {
            let gain = db(spectrum[bin(hz)]);
            if gain.abs() > worst_flat.0.abs() {
                worst_flat = (gain, hz);
            }
        }
        let image = db(spectrum[bin(14_000)..].iter().copied().fold(0.0, f64::max));
        if image > worst_image.0 {
            worst_image = (image, hz);
        }
    }
    assert!(
        worst_flat.0.abs() <= 0.1,
        "{conversion:?}: {:.4} dB at {} Hz, outside 0.1 dB of flat",
        worst_flat.0,
        worst_flat.1
    );
    assert!(
        worst_image.0 <= -60.0,
        "{conversion:?}: a tone at {} Hz leaves an image at {:.1} dB from 14 kHz up",
        worst_image.1,
        worst_image.0
    );
}

#[test]
fn the_decimator_is_flat_to_7_khz_and_60_db_down_from_9_khz() {
    assert_capture_bar(Conversion::From48kTo16k);
}

#[test]
fn the_interpolator_is_flat_to_10_khz_with_images_60_db_down_from_14_khz() {
    assert_synthesis_bar(Conversion::From24kTo48k);
}

#[test]
fn capture_at_44k1_meets_the_decimator_bar() {
    assert_capture_bar(Conversion::From44k1To16k);
}

#[test]
fn synthesis_to_44k1_meets_the_interpolator_bar() {
    assert_synthesis_bar(Conversion::From24kTo44k1);
}

/// A deterministic test signal with energy across the band: a few tones and a linear
/// congruential noise, so no two neighbouring samples are alike.
fn signal(len: usize) -> Vec<f32> {
    let mut state: u32 = 0x2545_f491;
    (0..u32::try_from(len).unwrap())
        .map(|n| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = f64::from(state >> 8) / f64::from(1_u32 << 24) - 0.5;
            let t = f64::from(n);
            narrow(0.3 * (t * 0.031).sin() + 0.2 * (t * 0.47).sin() + 0.1 * noise)
        })
        .collect()
}

/// Runs `input` through a fresh resampler in pieces of the given sizes, in turn, and returns
/// the samples it wrote, checking that each piece gives what `output_len` promised.
fn in_pieces(conversion: Conversion, input: &[f32], sizes: &[usize]) -> Vec<f32> {
    let mut resampler = Resampler::new(conversion);
    let mut out = Vec::new();
    let mut rest = input;
    for &size in sizes.iter().cycle() {
        if rest.is_empty() {
            break;
        }
        let (piece, tail) = rest.split_at(size.min(rest.len()));
        rest = tail;
        let promised = resampler.output_len(piece.len());
        let mut buffer = vec![0.0; promised];
        let written = resampler.process(piece, &mut buffer).unwrap();
        assert_eq!(written, promised, "{conversion:?}: output_len for a piece of {}", piece.len());
        out.extend_from_slice(&buffer);
    }
    out
}

fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|s| s.to_bits()).collect()
}

#[test]
fn frame_boundaries_lose_no_sample() {
    for conversion in Conversion::ALL {
        // One second of input.
        let input = signal(usize::try_from(conversion.input_hz()).unwrap());
        let whole = in_pieces(conversion, &input, &[input.len()]);
        assert_eq!(
            whole.len(),
            usize::try_from(conversion.output_hz()).unwrap(),
            "{conversion:?}: one second in gives one second out"
        );
        for sizes in [&[1][..], &[2, 3, 5, 7], &[480, 1, 441, 13, 0, 160], &[4_096]] {
            let pieces = in_pieces(conversion, &input, sizes);
            assert_eq!(bits(&pieces), bits(&whole), "{conversion:?} in pieces of {sizes:?}");
        }
    }
}

#[test]
fn every_10_ms_buffer_gives_10_ms() {
    for conversion in Conversion::ALL {
        let buffer = usize::try_from(conversion.input_hz() / 100).unwrap();
        let expected = usize::try_from(conversion.output_hz() / 100).unwrap();
        let mut resampler = Resampler::new(conversion);
        let input = signal(buffer);
        let mut output = vec![0.0; expected];
        for n in 0..250 {
            let written = resampler.process(&input, &mut output).unwrap();
            assert_eq!(written, expected, "{conversion:?}: buffer {n}");
        }
    }
}

#[test]
fn a_short_output_is_refused_before_any_input_is_taken() {
    let input = signal(480);
    let mut refused = Resampler::new(Conversion::From48kTo16k);
    let mut short = [0.0; 159];
    assert_eq!(
        refused.process(&input, &mut short),
        Err(ResampleError::OutputTooShort { needed: 160, got: 159 })
    );

    let mut fresh = Resampler::new(Conversion::From48kTo16k);
    let (mut after_refusal, mut from_fresh) = ([0.0; 160], [0.0; 160]);
    assert_eq!(refused.process(&input, &mut after_refusal), Ok(160));
    assert_eq!(fresh.process(&input, &mut from_fresh), Ok(160));
    assert_eq!(bits(&after_refusal), bits(&from_fresh), "the refusal took no input");
}

#[test]
fn only_the_four_conversions_are_offered() {
    for conversion in Conversion::ALL {
        let resampler = Resampler::between(conversion.input_hz(), conversion.output_hz()).unwrap();
        assert_eq!(resampler.conversion(), conversion);
    }
    assert_eq!(
        Resampler::between(32_000, 16_000).err(),
        Some(ResampleError::UnsupportedRates { input_hz: 32_000, output_hz: 16_000 })
    );
    assert_eq!(Conversion::between(16_000, 48_000), None, "no capture rate of 16 kHz");
}

#[test]
fn the_downmix_averages_the_channels() {
    let stereo = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0, 0.25, -0.75];
    let mut mono = [9.0; 5];
    assert_eq!(downmix(&stereo, 2, &mut mono), Ok(4));
    assert_eq!(bits(&mono), bits(&[0.5, 0.5, 0.0, -0.25, 9.0]), "the fifth slot is untouched");

    let one = [0.25, -0.5];
    let mut copy = [0.0; 2];
    assert_eq!(downmix(&one, 1, &mut copy), Ok(2));
    assert_eq!(bits(&copy), bits(&one), "mono passes through");
}

#[test]
fn the_downmix_refuses_a_partial_frame_no_channels_and_a_short_output() {
    let mut mono = [0.0; 4];
    assert_eq!(
        downmix(&[0.0; 5], 2, &mut mono),
        Err(ResampleError::PartialFrame { samples: 5, channels: 2 })
    );
    assert_eq!(downmix(&[0.0; 4], 0, &mut mono), Err(ResampleError::NoChannels));
    assert_eq!(
        downmix(&[0.0; 10], 2, &mut mono),
        Err(ResampleError::OutputTooShort { needed: 5, got: 4 })
    );
}

#[test]
fn the_callback_path_allocates_nothing() {
    // Everything a callback touches is made before it runs: the resamplers and the buffers.
    let stereo_48k = signal(960);
    let stereo_44k1 = signal(882);
    let synthesis = signal(240);
    let mut capture_48k = Resampler::new(Conversion::From48kTo16k);
    let mut capture_44k1 = Resampler::new(Conversion::From44k1To16k);
    let mut render_48k = Resampler::new(Conversion::From24kTo48k);
    let mut render_44k1 = Resampler::new(Conversion::From24kTo44k1);
    let mut mono = [0.0; 480];
    let mut speech = [0.0; 160];
    let mut playback = [0.0; 480];
    let mut written = Vec::with_capacity(16);

    let counted = allocation_counter::measure(|| {
        for _ in 0..4 {
            let frames = downmix(&stereo_48k, 2, &mut mono).unwrap_or(0);
            written.push(capture_48k.process(&mono[..frames], &mut speech));
            let frames = downmix(&stereo_44k1, 2, &mut mono).unwrap_or(0);
            written.push(capture_44k1.process(&mono[..frames], &mut speech));
            written.push(render_48k.process(&synthesis, &mut playback));
            written.push(render_44k1.process(&synthesis, &mut playback));
        }
    });
    assert_eq!(counted, AllocationInfo::default(), "no allocation, reallocation or free");
    let expected = [Ok(160), Ok(160), Ok(480), Ok(441)];
    assert_eq!(written, expected.repeat(4));
}

/// The cost bar of BUILD.md section 6.6: each conversion takes at most 25 us per 10 ms buffer,
/// on average, in a release build on the owner's PC, an eighth of the core's 2 percent idle
/// budget. A capture conversion's cost includes the stereo downmix. Run it with nothing else
/// compiling, through `cargo xtask test --owner --release -p eileen-audio owner_pc::` (a release
/// build takes both build slots) with `--no-capture` added to see the numbers: each conversion
/// prints its mean, median, 99th percentile and worst buffer, which the pull request's
/// `owner-run:` line records. It is a test only without debug assertions, so a debug run neither
/// passes nor fails it, but every test build compiles it, so CI's lints cover it.
#[cfg_attr(debug_assertions, expect(dead_code, reason = "a test only in release builds"))]
mod owner_pc {
    use std::hint::black_box;

    use eileen_stdx::clock::{Clock, SystemClock};

    use super::signal;
    use crate::resample::{Conversion, Resampler, downmix};

    const WARM_UP: usize = 500;
    const MEASURED: usize = 5_000;
    const BAR_NS: u64 = 25_000;

    /// Nanoseconds per 10 ms buffer, sorted.
    fn costs(conversion: Conversion) -> Vec<u64> {
        let clock = SystemClock::new();
        let capture = conversion.output_hz() == 16_000;
        let frames = usize::try_from(conversion.input_hz() / 100).unwrap();
        let input = signal(if capture { 2 * frames } else { frames });
        let mut mono = vec![0.0; frames];
        let mut resampler = Resampler::new(conversion);
        let mut output = vec![0.0; resampler.output_len(frames) + 1];
        let mut costs = Vec::with_capacity(MEASURED);
        for n in 0..WARM_UP + MEASURED {
            let start = clock.mono();
            let written = if capture {
                let got = downmix(black_box(&input), 2, &mut mono).unwrap();
                resampler.process(&mono[..got], &mut output).unwrap()
            } else {
                resampler.process(black_box(&input), &mut output).unwrap()
            };
            let end = clock.mono();
            black_box((written, &output));
            if n >= WARM_UP {
                costs.push(end.0.saturating_sub(start.0));
            }
        }
        costs.sort_unstable();
        costs
    }

    #[cfg_attr(not(debug_assertions), test)]
    fn each_conversion_takes_at_most_25_us_per_10_ms_buffer() {
        let mut over = Vec::new();
        for conversion in Conversion::ALL {
            let costs = costs(conversion);
            let count = u64::try_from(costs.len()).unwrap();
            let mean = costs.iter().sum::<u64>() / count;
            let at = |per_mille: usize| costs[(costs.len() - 1) * per_mille / 1_000];
            println!(
                "{conversion:?}: mean {mean} ns, p50 {} ns, p99 {} ns, max {} ns per 10 ms buffer",
                at(500),
                at(990),
                at(1_000)
            );
            if mean > BAR_NS {
                over.push((conversion, mean));
            }
        }
        assert!(over.is_empty(), "over the 25 us bar: {over:?}");
    }
}
