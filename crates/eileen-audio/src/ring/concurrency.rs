//! The handoff between the capture thread and the reader. Under `--cfg loom` each test is a loom
//! model that explores every order of the ring's own atomics; in every other build the same body
//! runs once on a real thread, so the tests always compile and run.

use std::time::Duration;

use eileen_contracts::speech::{AUDIO_FRAME_SAMPLES, AudioFlags};

use super::{CaptureFlags, CaptureWriter, SpeechRing};

#[cfg(loom)]
fn model(body: impl Fn() + Sync + Send + 'static) {
    loom::model(body);
}

#[cfg(not(loom))]
fn model(body: impl Fn() + Sync + Send + 'static) {
    body();
}

#[cfg(loom)]
fn on_capture_thread<T: Send + 'static>(
    body: impl FnOnce() -> T + Send + 'static,
) -> loom::thread::JoinHandle<T> {
    loom::thread::spawn(body)
}

#[cfg(not(loom))]
fn on_capture_thread<T: Send + 'static>(
    body: impl FnOnce() -> T + Send + 'static,
) -> std::thread::JoinHandle<T> {
    eileen_stdx::thread::spawn_named("ring-capture", body).expect("the capture thread starts")
}

/// Pushes the frames numbered `from` to `to`, each frame's samples its number, and returns how
/// many reached the handoff.
fn push_range(writer: &mut CaptureWriter, from: i16, to: i16) -> u64 {
    let mut handed = 0;
    for n in from..=to {
        let at = Duration::from_millis(32 * u64::try_from(n).unwrap());
        if writer.push(&[n; AUDIO_FRAME_SAMPLES], at, CaptureFlags::default()) {
            handed += 1;
        }
    }
    handed
}

/// The frames written, each as its `seq`, its first sample and its flags.
fn write_all(ring: &mut SpeechRing, into: &mut Vec<(u64, i16, AudioFlags)>) {
    while let Some(frame) = ring.next_frame() {
        into.push((frame.seq, frame.samples[0], frame.flags));
    }
}

#[test]
fn the_handoff_keeps_order_and_accounts_for_every_frame() {
    model(|| {
        let mut ring = SpeechRing::new(8, 1);
        let mut writer = ring.open_capture();
        let capture = on_capture_thread(move || push_range(&mut writer, 1, 3));
        ring.connected();
        let mut written = Vec::new();
        write_all(&mut ring, &mut written);
        let handed = capture.join().unwrap();
        write_all(&mut ring, &mut written);

        assert!(written.windows(2).all(|pair| pair[0].0 < pair[1].0), "in order: {written:?}");
        assert!(
            written.iter().all(|(seq, first, _)| u64::try_from(*first).unwrap() == *seq),
            "each frame kept its own number and samples: {written:?}"
        );
        assert_eq!(u64::try_from(written.len()).unwrap(), handed);
        assert_eq!(handed + ring.take_dropped(), 3, "every frame reached the store or was counted");
        for pair in written.windows(2) {
            let gap = pair[1].0 != pair[0].0 + 1;
            assert_eq!(pair[1].2.contains(AudioFlags::BREAK), gap, "break exactly after a gap");
        }
    });
}

#[test]
fn a_later_capture_stream_goes_on_numbering_where_the_last_stopped() {
    model(|| {
        let mut ring = SpeechRing::new(8, 4);
        let mut first = ring.open_capture();
        let handed = on_capture_thread(move || push_range(&mut first, 1, 2)).join().unwrap();
        assert_eq!(handed, 2);
        ring.capture_stopped();

        let mut second = ring.open_capture();
        let capture = on_capture_thread(move || push_range(&mut second, 3, 4));
        ring.connected();
        let mut written = Vec::new();
        write_all(&mut ring, &mut written);
        assert_eq!(capture.join().unwrap(), 2);
        write_all(&mut ring, &mut written);

        let seqs: Vec<u64> = written.iter().map(|(seq, _, _)| *seq).collect();
        assert_eq!(seqs, [3, 4], "no number reused, none skipped");
        assert!(written[0].2.contains(AudioFlags::BREAK), "capture had stopped before 3");
        assert!(!written[1].2.contains(AudioFlags::BREAK));
    });
}

#[test]
fn nothing_captured_before_a_stop_is_written_after_it() {
    model(|| {
        let mut ring = SpeechRing::new(8, 2);
        let mut writer = ring.open_capture();
        let capture = on_capture_thread(move || push_range(&mut writer, 1, 3));
        ring.connected();
        let mut written = Vec::new();
        write_all(&mut ring, &mut written);
        // The stream stops: its callback, and with it the writer, is never called again.
        capture.join().unwrap();
        ring.capture_stopped();
        ring.connected();
        assert!(ring.next_frame().is_none(), "the store and the handoff were emptied");
    });
}
