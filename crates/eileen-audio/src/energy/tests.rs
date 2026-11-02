use super::{EnergyDetector, EnergyWindow, ONSET_WINDOWS, WINDOW};

/// A window whose level is `dbfs`: a square wave, whose mean square is its amplitude squared.
fn at(dbfs: f32) -> [f32; WINDOW] {
    let amplitude = 10_f32.powf(dbfs / 20.0);
    std::array::from_fn(|n| if n % 2 == 0 { amplitude } else { -amplitude })
}

/// Feeds `windows` and returns what the detector made of each.
fn feed(
    detector: &mut EnergyDetector,
    windows: &[[f32; WINDOW]],
    other: bool,
) -> Vec<EnergyWindow> {
    windows.iter().map(|window| detector.window(window, other)).collect()
}

/// A detector whose floor settled on `dbfs`.
fn settled_on(dbfs: f32) -> EnergyDetector {
    let mut detector = EnergyDetector::new();
    let quiet = feed(&mut detector, &[at(dbfs); 10], false);
    assert!(quiet.iter().all(|window| !window.speech), "the floor itself is not speech");
    detector
}

#[test]
fn an_onset_comes_at_the_third_speech_window_and_once_per_run() {
    let mut detector = settled_on(-70.0);
    let mut windows = vec![at(-30.0); 6];
    windows.push(at(-70.0));
    windows.extend([at(-30.0); 4]);
    let seen = feed(&mut detector, &windows, false);

    let runs: Vec<u32> = seen.iter().map(|window| window.run).collect();
    assert_eq!(runs, [1, 2, 3, 4, 5, 6, 0, 1, 2, 3, 4], "speech windows in a row");
    let onsets: Vec<usize> =
        seen.iter().enumerate().filter(|(_, window)| window.onset).map(|(n, _)| n).collect();
    assert_eq!(onsets, [2, 9], "the third window of each run, once each");
    assert_eq!(ONSET_WINDOWS, 3, "96 ms, where barge-in pauses playback");
}

#[test]
fn other_audio_asks_six_more_decibels_over_the_floor() {
    // 15 dB over the floor: over the 12 dB margin, under the 18 dB one.
    let mut detector = settled_on(-55.0);
    let seen = feed(&mut detector, &[at(-40.0); 3], true);
    assert!(seen.iter().all(|window| !window.speech), "not speech while other audio plays");

    let mut detector = settled_on(-55.0);
    let seen = feed(&mut detector, &[at(-40.0); 3], false);
    assert!(seen.iter().all(|window| window.speech), "speech otherwise");
    assert!(seen[2].onset, "and an onset at the third");

    let mut detector = settled_on(-55.0);
    let seen = feed(&mut detector, &[at(-35.0); 3], true);
    assert!(seen[2].onset, "20 dB over the floor counts with other audio too");
}

#[test]
fn a_window_under_minus_45_dbfs_is_never_speech() {
    let mut detector = settled_on(-100.0);
    let seen = feed(&mut detector, &[at(-46.0); 5], false);
    assert!(seen.iter().all(|window| !window.speech), "54 dB over the floor, but too quiet");
    let seen = feed(&mut detector, &[at(-44.0); 3], false);
    assert!(seen[2].onset, "a voice at -44 dBFS counts");
}

#[test]
fn digital_silence_keeps_the_level_finite() {
    let mut detector = EnergyDetector::new();
    let seen = feed(&mut detector, &[[0.0; WINDOW]; 4], false);
    assert!(seen.iter().all(|window| !window.speech), "silence is not speech");
    let seen = feed(&mut detector, &[at(-30.0); 3], false);
    assert!(seen[2].onset, "and speech after it still counts");
}

#[test]
fn the_floor_rises_to_a_steady_noise_and_drops_at_once() {
    // A fan that starts at -40 dBFS over a quiet room counts once, then stops counting as the
    // floor rises 5 dB a second: 18 dB takes 3.6 s, 113 windows.
    let mut detector = settled_on(-70.0);
    let seen = feed(&mut detector, &[at(-40.0); 150], false);
    let onsets = seen.iter().filter(|window| window.onset).count();
    assert_eq!(onsets, 1, "one onset when the noise starts");
    let last_speech = seen.iter().rposition(|window| window.speech).unwrap();
    assert!((100..=120).contains(&last_speech), "speech until window {last_speech}");

    // The floor drops to a quieter window at once, so speech over the quiet room counts again.
    let seen = feed(&mut detector, &[at(-70.0), at(-40.0), at(-40.0), at(-40.0)], false);
    assert!(seen[3].onset, "an onset over the room's own floor");
}

#[test]
fn the_first_window_sets_the_floor() {
    // A noise present from the start never counts.
    let mut detector = EnergyDetector::new();
    let seen = feed(&mut detector, &[at(-35.0); 50], false);
    assert!(seen.iter().all(|window| !window.speech), "a noise from the start is the floor");
}

#[test]
fn judging_a_window_allocates_nothing() {
    let mut detector = settled_on(-70.0);
    let loud = at(-30.0);
    let quiet = at(-70.0);
    let mut onsets = 0;
    let counted = allocation_counter::measure(|| {
        for n in 0..64 {
            let window = if n % 8 < 5 { &loud } else { &quiet };
            onsets += u32::from(detector.window(window, n % 16 < 8).onset);
        }
    });
    assert_eq!(counted, allocation_counter::AllocationInfo::default(), "no allocation or free");
    assert!(onsets > 0, "the loop judged speech");
}
