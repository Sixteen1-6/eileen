use super::{Canceller, EnergyDetector, EnergyWindow, ONSET_WINDOWS, WINDOW, WindowAssembler};
use crate::aec::{CHUNK, ChunkReport, Reference};
use crate::ring::CaptureFlags;

/// A window whose level is `dbfs`: a square wave, whose mean square is its amplitude squared.
fn at(dbfs: f32) -> [f32; WINDOW] {
    let amplitude = 10_f32.powf(dbfs / 20.0);
    std::array::from_fn(|n| if n % 2 == 0 { amplitude } else { -amplitude })
}

/// Feeds `windows`, each as the canceller passed it whole, and returns what the detector made
/// of each.
fn feed(
    detector: &mut EnergyDetector,
    windows: &[[f32; WINDOW]],
    other: bool,
) -> Vec<EnergyWindow> {
    windows
        .iter()
        .map(|window| detector.window(window, window, other, Canceller::Converged))
        .collect()
}

/// A detector whose floor settled on `dbfs`.
fn settled_on(dbfs: f32) -> EnergyDetector {
    let mut detector = EnergyDetector::new();
    let quiet = feed(&mut detector, &vec![at(dbfs); 10], false);
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
    let mut detector = settled_on(-52.0);
    let seen = feed(&mut detector, &[at(-37.0); 3], true);
    assert!(seen.iter().all(|window| !window.speech), "not speech while other audio plays");

    let mut detector = settled_on(-52.0);
    let seen = feed(&mut detector, &[at(-37.0); 3], false);
    assert!(seen.iter().all(|window| window.speech), "speech otherwise");
    assert!(seen[2].onset, "and an onset at the third");

    let mut detector = settled_on(-52.0);
    let seen = feed(&mut detector, &[at(-32.0); 3], true);
    assert!(seen[2].onset, "20 dB over the floor counts with other audio too");
}

#[test]
fn a_window_under_minus_40_dbfs_is_never_speech() {
    let mut detector = settled_on(-100.0);
    let seen = feed(&mut detector, &[at(-41.0); 5], false);
    assert!(seen.iter().all(|window| !window.speech), "59 dB over the floor, but too quiet");
    let seen = feed(&mut detector, &[at(-39.0); 3], false);
    assert!(seen[2].onset, "a voice at -39 dBFS counts");
}

#[test]
fn a_window_the_canceller_took_more_than_6_db_from_is_not_speech() {
    // A loud remainder of echo: -30 dBFS after the canceller, from -22 dBFS at the microphone.
    let mut detector = settled_on(-70.0);
    let (cancelled, heard) = (at(-30.0), at(-22.0));
    let seen: Vec<EnergyWindow> =
        (0..5).map(|_| detector.window(&cancelled, &heard, false, Canceller::Converged)).collect();
    assert!(seen.iter().all(|window| !window.speech), "8 dB cancelled: echo, not a voice");

    // Bypassed, the 8 dB came off in noise suppression, which a voice can lose as it starts.
    let seen: Vec<EnergyWindow> =
        (0..3).map(|_| detector.window(&cancelled, &heard, false, Canceller::Bypassed)).collect();
    assert!(seen[2].onset, "8 dB suppressed with AEC3 bypassed: a voice");

    // A voice over the echo, after a quiet window ends that run: -23 dBFS after the canceller,
    // from -21 dBFS.
    let quiet = at(-70.0);
    assert!(!detector.window(&quiet, &quiet, false, Canceller::Converged).speech, "a quiet window");
    let (cancelled, heard) = (at(-23.0), at(-21.0));
    let seen: Vec<EnergyWindow> =
        (0..3).map(|_| detector.window(&cancelled, &heard, false, Canceller::Converged)).collect();
    assert!(seen[2].onset, "2 dB cancelled: a voice");
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
    // A fan that starts at -35 dBFS over a quiet room counts once, then stops counting as the
    // floor rises 5 dB a second: 23 dB takes 4.6 s, 144 windows.
    let mut detector = settled_on(-70.0);
    let seen = feed(&mut detector, &vec![at(-35.0); 180], false);
    let onsets = seen.iter().filter(|window| window.onset).count();
    assert_eq!(onsets, 1, "one onset when the noise starts");
    let last_speech = seen.iter().rposition(|window| window.speech).unwrap();
    assert!((130..=150).contains(&last_speech), "speech until window {last_speech}");

    // The floor drops to a quieter window at once, so speech over the quiet room counts again.
    let seen = feed(&mut detector, &[at(-70.0), at(-35.0), at(-35.0), at(-35.0)], false);
    assert!(seen[3].onset, "an onset over the room's own floor");
}

#[test]
fn the_first_window_sets_the_floor() {
    // A noise present from the start never counts.
    let mut detector = EnergyDetector::new();
    let seen = feed(&mut detector, &vec![at(-35.0); 50], false);
    assert!(seen.iter().all(|window| !window.speech), "a noise from the start is the floor");
}

#[test]
fn no_window_is_speech_while_aec3_learns_the_room() {
    // Echo AEC3 has not learned yet: as loud as a voice, and little taken.
    let mut detector = settled_on(-70.0);
    let loud = at(-25.0);
    let seen: Vec<EnergyWindow> =
        (0..5).map(|_| detector.window(&loud, &loud, false, Canceller::Learning)).collect();
    assert!(seen.iter().all(|window| !window.speech), "nothing counts while AEC3 learns");
    assert!(seen.iter().all(|window| window.short_db.is_infinite()), "and nothing comes close");

    // Once it has learned the room, the same windows count.
    let seen = feed(&mut detector, &[loud; 3], false);
    assert!(seen[2].onset, "an onset once AEC3 has converged");
    assert!(seen.iter().all(|window| window.short_db <= 0.0), "speech is short by nothing");
}

/// A report of a chunk cancelled against the loopback.
const fn report(learning: bool, other_audio: bool) -> ChunkReport {
    ChunkReport {
        reference: Reference::Loopback,
        learning,
        repaired: false,
        flags: CaptureFlags { playback: false, other_audio },
    }
}

#[test]
fn a_window_pairs_its_samples_and_carries_the_flags_of_every_chunk_it_spans() {
    // Sixteen 10 ms chunks make five 32 ms windows. Chunk 3 (samples 480 to 640) spans windows 0
    // and 1, and chunk 9 (1,440 to 1,600) windows 2 and 3. The canceller passes each chunk at
    // half amplitude, 6.02 dB down.
    let mut assembler = WindowAssembler::new();
    let mut windows = Vec::new();
    for k in 0..16 {
        let mic: [f32; CHUNK] =
            std::array::from_fn(|n| if (k * CHUNK + n).is_multiple_of(2) { 0.1 } else { -0.1 });
        let out = mic.map(|sample| 0.5 * sample);
        let flags = report(k == 9, k == 3);
        windows.extend(assembler.push(&out, &mic, flags));
    }
    assert_eq!(windows.len(), 5, "a window every 512 samples");
    let other: Vec<bool> = windows.iter().map(|window| window.other_audio).collect();
    assert_eq!(other, [true, true, false, false, false], "chunk 3's other audio in both windows");
    let canceller: Vec<Canceller> = windows.iter().map(|window| window.canceller).collect();
    let (converged, learning) = (Canceller::Converged, Canceller::Learning);
    assert_eq!(
        canceller,
        [converged, converged, learning, learning, converged],
        "chunk 9's learning in both windows"
    );
    for window in &windows {
        let taken = window.heard_dbfs - window.level_dbfs;
        assert!((taken - 6.02).abs() < 0.01, "each window beside its own microphone samples");
        assert!((window.heard_dbfs + 20.0).abs() < 0.01, "the microphone at -20 dBFS");
    }
}

#[test]
fn a_bypassed_chunk_leaves_a_window_bypassed_only_if_all_its_chunks_were() {
    let bypassed = ChunkReport { reference: Reference::Bypassed, ..report(false, false) };
    let mut assembler = WindowAssembler::new();
    let silence = [0.0; CHUNK];
    let mut windows = Vec::new();
    for k in 0..16 {
        let chunk = if k == 10 { report(false, false) } else { bypassed };
        windows.extend(assembler.push(&silence, &silence, chunk));
    }
    let canceller: Vec<Canceller> = windows.iter().map(|window| window.canceller).collect();
    let (bypassed, converged) = (Canceller::Bypassed, Canceller::Converged);
    assert_eq!(
        canceller,
        [bypassed, bypassed, bypassed, converged, bypassed],
        "chunk 10's AEC3, inside window 3"
    );
}

#[test]
fn judging_a_window_allocates_nothing() {
    let mut detector = settled_on(-70.0);
    let loud = at(-30.0);
    let quiet = at(-70.0);
    let cancellers = [Canceller::Converged, Canceller::Bypassed, Canceller::Learning];
    let mut onsets = 0;
    let counted = allocation_counter::measure(|| {
        for n in 0..64 {
            let window = if n % 8 < 5 { &loud } else { &quiet };
            let canceller = cancellers[(n / 8) % 3];
            onsets += u32::from(detector.window(window, window, n % 16 < 8, canceller).onset);
        }
    });
    assert_eq!(counted, allocation_counter::AllocationInfo::default(), "no allocation or free");
    assert!(onsets > 0, "the loop judged speech");
}

#[test]
fn assembling_windows_allocates_nothing() {
    let mut assembler = WindowAssembler::new();
    let loud: [f32; CHUNK] = std::array::from_fn(|n| if n % 2 == 0 { 0.03 } else { -0.03 });
    let quiet = [0.0; CHUNK];
    let mut windows = 0;
    let counted = allocation_counter::measure(|| {
        for k in 0..200 {
            let chunk = if k % 40 < 20 { &loud } else { &quiet };
            let judged = assembler.push(chunk, chunk, report(k < 10, k % 50 < 25));
            windows += u32::from(judged.is_some());
        }
    });
    assert_eq!(counted, allocation_counter::AllocationInfo::default(), "no allocation or free");
    assert_eq!(windows, 62, "a window every 3.2 chunks");
}
