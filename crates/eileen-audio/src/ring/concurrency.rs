//! The handoff between the capture thread and the reader.
//!
//! Under `--cfg loom` each `model` test explores every order of the ring's own atomics: the next
//! `seq`, which the writer takes and the reader reads for a frame's age, and the count of frames
//! the handoff lost, which the writer adds to while the reader takes it. rtrb's atomics are
//! outside loom's view. In every other build the same bodies run once on a real thread, and
//! `a_reader_draining_beside_a_writer_that_wraps_the_handoff_keeps_every_rule` drives a small
//! handoff through thousands of wraps on real threads.

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

/// A frame written: its `seq`, its first sample and its flags.
type Written = (u64, i16, AudioFlags);

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

/// Writes every frame left on the connection into `into`, then takes the count of frames lost,
/// while the capture thread may still be pushing.
fn drain(ring: &mut SpeechRing, into: &mut Vec<Written>) -> u64 {
    while let Some(frame) = ring.next_frame() {
        into.push((frame.seq, frame.samples[0], frame.flags));
    }
    ring.take_dropped()
}

/// As [`drain`], but confirms each frame once written, as the speech process's `Pong` would, so
/// however long the writer keeps the reader draining, the store never drops a frame already
/// written, which would count it twice.
#[cfg(not(loom))]
fn drain_confirming(ring: &mut SpeechRing, into: &mut Vec<Written>) -> u64 {
    while let Some(frame) = ring.next_frame() {
        let seq = frame.seq;
        into.push((seq, frame.samples[0], frame.flags));
        ring.confirm_through(seq);
    }
    ring.take_dropped()
}

/// The frames written on one connection after the frame `before`: in order, each with its own
/// samples, and with the break flag exactly where frames are missing, and on the first when
/// capture had stopped before it.
fn check_written(written: &[Written], before: u64, after_stop: bool) {
    let mut previous = before;
    for (n, &(seq, sample, flags)) in written.iter().enumerate() {
        assert!(seq > previous, "in order: {written:?}");
        assert_eq!(u64::try_from(sample).unwrap(), seq, "its own samples: {written:?}");
        let gap = seq != previous + 1 || (n == 0 && after_stop);
        assert_eq!(
            flags.contains(AudioFlags::BREAK),
            gap,
            "break exactly after a gap: {written:?}"
        );
        previous = seq;
    }
}

fn count(written: &[Written]) -> u64 {
    u64::try_from(written.len()).unwrap()
}

#[test]
fn the_handoff_keeps_order_and_accounts_for_every_frame() {
    model(|| {
        let mut ring = SpeechRing::new(8, 1);
        let mut writer = ring.open_capture();
        let capture = on_capture_thread(move || push_range(&mut writer, 1, 3));
        ring.connected();
        let mut written = Vec::new();
        let mut dropped = drain(&mut ring, &mut written);
        dropped += drain(&mut ring, &mut written);
        let handed = capture.join().unwrap();
        dropped += drain(&mut ring, &mut written);

        check_written(&written, 0, false);
        assert_eq!(count(&written), handed);
        assert_eq!(handed + dropped, 3, "every frame reached the store or was counted");
    });
}

#[test]
fn a_later_capture_stream_goes_on_numbering_where_the_last_stopped() {
    model(|| {
        let mut ring = SpeechRing::new(8, 1);
        let mut first = ring.open_capture();
        let first_handed = on_capture_thread(move || push_range(&mut first, 1, 1)).join().unwrap();
        assert_eq!(first_handed, 1);
        ring.connected();
        let mut before = Vec::new();
        assert_eq!(drain(&mut ring, &mut before), 0);
        check_written(&before, 0, false);
        ring.confirm_through(1);
        ring.capture_stopped();

        // Same connection: 2 follows the 1 written with no gap, so only the mute breaks it.
        let mut second = ring.open_capture();
        let capture = on_capture_thread(move || push_range(&mut second, 2, 3));
        let mut written = Vec::new();
        let mut dropped = drain(&mut ring, &mut written);
        dropped += drain(&mut ring, &mut written);
        let handed = capture.join().unwrap();
        dropped += drain(&mut ring, &mut written);

        assert!(written.iter().all(|(seq, _, _)| (2..=3).contains(seq)), "{written:?}");
        check_written(&written, 1, true);
        assert_eq!(count(&written), handed);
        assert_eq!(handed + dropped, 2, "whatever the second stream lost is counted");
    });
}

#[test]
fn nothing_captured_before_a_stop_is_written_after_it() {
    model(|| {
        let mut ring = SpeechRing::new(8, 1);
        let mut writer = ring.open_capture();
        let capture = on_capture_thread(move || push_range(&mut writer, 1, 3));
        ring.connected();
        let mut written = Vec::new();
        let mut dropped = drain(&mut ring, &mut written);
        // The stream stops: its callback, and with it the writer, is never called again.
        let handed = capture.join().unwrap();
        ring.capture_stopped();
        dropped += ring.take_dropped();
        ring.connected();

        assert!(ring.next_frame().is_none(), "the store and the handoff were emptied");
        check_written(&written, 0, false);
        assert_eq!(handed + dropped, 3, "a frame the stop discarded was handed, not lost");
    });
}

#[cfg(not(loom))]
#[test]
fn a_reader_draining_beside_a_writer_that_wraps_the_handoff_keeps_every_rule() {
    const PUSHED: i16 = 10_000;
    let mut ring = SpeechRing::new(super::STORE_FRAMES, 4);
    let mut writer = ring.open_capture();
    let capture = on_capture_thread(move || push_range(&mut writer, 1, PUSHED));
    ring.connected();
    let mut written = Vec::new();
    let mut dropped = 0;
    loop {
        // Read before draining, so the last pass sees every frame the writer handed.
        let finished = capture.is_finished();
        dropped += drain_confirming(&mut ring, &mut written);
        if finished {
            break;
        }
        std::thread::yield_now();
    }
    capture.join().unwrap();
    dropped += drain_confirming(&mut ring, &mut written);

    check_written(&written, 0, false);
    // A reader slower than the writer can also overrun the store, which drops frames handed but
    // not yet written, so the count covers both kinds of loss.
    assert_eq!(
        count(&written) + dropped,
        u64::try_from(PUSHED).unwrap(),
        "every frame written or counted once"
    );
}
