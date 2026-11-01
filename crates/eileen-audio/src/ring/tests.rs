// Outside a loom model loom's atomics panic, so under `--cfg loom` only `concurrency` runs.
#![cfg(not(loom))]

use std::time::Duration;

use allocation_counter::AllocationInfo;
use eileen_contracts::ids::{Personal, TraceCtx, TurnId};
use eileen_contracts::speech::{
    AUDIO_FRAME_SAMPLES, AudioFlags, Frame, FrameBody, SpeechEventFrame, TurnEndReason, VoiceEvent,
};

use super::{CaptureFlags, CaptureWriter, HANDOFF_FRAMES, STORE_FRAMES, SpeechRing};

/// A frame's samples, each `n` (below 30 000), so a frame read back shows which push it was.
fn samples(n: u64) -> [i16; AUDIO_FRAME_SAMPLES] {
    [i16::try_from(n % 30_000).unwrap(); AUDIO_FRAME_SAMPLES]
}

/// Pushes `count` frames, each captured 32 ms after the last, the first at `from_ms`; returns
/// whether each reached the handoff.
fn capture(writer: &mut CaptureWriter, from_ms: u64, count: u64) -> Vec<bool> {
    (0..count)
        .map(|n| {
            let at = Duration::from_millis(from_ms + 32 * n);
            writer.push(&samples(n), at, CaptureFlags::default())
        })
        .collect()
}

/// Every frame left to write on the connection: its `seq` and flags.
fn write_all(ring: &mut SpeechRing) -> Vec<(u64, AudioFlags)> {
    let mut written = Vec::new();
    while let Some(frame) = ring.next_frame() {
        written.push((frame.seq, frame.flags));
    }
    written
}

fn seqs(written: &[(u64, AudioFlags)]) -> Vec<u64> {
    written.iter().map(|(seq, _)| *seq).collect()
}

/// The `seq`s among `written` that carry `flag`.
fn flagged(written: &[(u64, AudioFlags)], flag: AudioFlags) -> Vec<u64> {
    written.iter().filter(|(_, flags)| flags.contains(flag)).map(|(seq, _)| *seq).collect()
}

fn pong(seq: u64, turn: [u8; 16]) -> Frame {
    Frame { seq, turn, ..Frame::new(FrameBody::Pong) }
}

fn event(seq: u64, event: VoiceEvent) -> Frame {
    let body = FrameBody::Event(SpeechEventFrame { ctx: TraceCtx::default(), span: 1, event });
    Frame { seq, ..Frame::new(body) }
}

fn end_of_turn(turn: TurnId) -> VoiceEvent {
    VoiceEvent::EndOfTurn {
        turn,
        text: Personal::new("open notepad".to_owned()),
        smart_turn_pct: 90,
        silence_ms: 300,
        asr_confidence_pct: 95,
        reason: TurnEndReason::SmartTurn,
        source: 0,
        level_db: -30,
    }
}

/// A ring with `pushed` frames captured and all of them written on a first connection.
fn written_ring(pushed: u64) -> (SpeechRing, CaptureWriter) {
    let mut ring = SpeechRing::new(64, 64);
    let mut writer = ring.open_capture();
    capture(&mut writer, 0, pushed);
    ring.connected();
    assert_eq!(seqs(&write_all(&mut ring)), (1..=pushed).collect::<Vec<_>>(), "the first write");
    (ring, writer)
}

#[test]
fn seq_counts_captured_frames_from_one_across_capture_streams() {
    let (mut ring, mut first) = written_ring(3);
    assert!(first.push(&samples(9), Duration::from_millis(96), CaptureFlags::default()));
    ring.capture_stopped();
    let mut second = ring.open_capture();
    capture(&mut second, 200, 2);
    assert_eq!(seqs(&write_all(&mut ring)), [5, 6], "the frame pushed before the stop used 4");
}

#[test]
fn a_frame_carries_its_seq_capture_time_samples_and_capture_flags() {
    let mut ring = SpeechRing::new(8, 8);
    let mut writer = ring.open_capture();
    let at = Duration::from_millis(1_000);
    assert!(writer.push(&samples(7), at, CaptureFlags { playback: true, other_audio: false }));
    let later = at + Duration::from_millis(32);
    assert!(writer.push(&samples(8), later, CaptureFlags { playback: false, other_audio: true }));
    ring.connected();

    let first = ring.next_frame().unwrap();
    assert_eq!((first.seq, first.captured_at, first.samples), (1, at, &samples(7)));
    assert_eq!(first.flags, AudioFlags::PLAYBACK, "playback only, no break, not replayed");
    let second = ring.next_frame().unwrap();
    assert_eq!((second.seq, second.captured_at, second.samples), (2, later, &samples(8)));
    assert_eq!(second.flags, AudioFlags::OTHER_AUDIO);
    assert!(ring.next_frame().is_none(), "nothing else to write");
}

#[test]
fn a_pong_with_no_turn_open_confirms_through_its_seq() {
    let (mut ring, _writer) = written_ring(5);
    ring.on_speech_frame(&pong(3, [0; 16]));
    ring.connected();
    let replay = write_all(&mut ring);
    assert_eq!(seqs(&replay), [4, 5], "1 to 3 left the store");
    assert_eq!(flagged(&replay, AudioFlags::BREAK), [0_u64; 0], "4 follows the confirmed 3");
}

#[test]
fn a_pong_with_a_turn_open_confirms_nothing() {
    let (mut ring, _writer) = written_ring(5);
    ring.on_speech_frame(&pong(3, [7; 16]));
    ring.connected();
    assert_eq!(seqs(&write_all(&mut ring)), [1, 2, 3, 4, 5]);
}

#[test]
fn the_frame_carrying_an_end_of_turn_confirms_through_its_seq() {
    let (mut ring, _writer) = written_ring(5);
    let turn = TurnId::from_parts(1, 2);
    ring.on_speech_frame(&event(2, VoiceEvent::SpeechStarted { turn, source: 0, replayed: false }));
    ring.on_speech_frame(&event(3, VoiceEvent::SpeechEnded { turn }));
    ring.on_speech_frame(&event(4, end_of_turn(turn)));
    ring.on_speech_frame(&Frame { seq: 5, ..Frame::new(FrameBody::Ping) });
    ring.connected();
    assert_eq!(seqs(&write_all(&mut ring)), [5], "only the end of turn confirmed, through 4");
}

#[test]
fn a_confirmation_never_reaches_past_the_newest_frame_written() {
    let mut ring = SpeechRing::new(8, 8);
    let mut writer = ring.open_capture();
    capture(&mut writer, 0, 5);
    ring.connected();
    assert_eq!(ring.next_frame().map(|frame| frame.seq), Some(1));
    assert_eq!(ring.next_frame().map(|frame| frame.seq), Some(2));
    ring.on_speech_frame(&pong(5, [0; 16]));
    ring.connected();
    assert_eq!(seqs(&write_all(&mut ring)), [3, 4, 5], "3 to 5 were never sent");
}

#[test]
fn a_confirmation_never_goes_back() {
    let (mut ring, _writer) = written_ring(5);
    ring.confirm_through(4);
    ring.confirm_through(2);
    ring.connected();
    assert_eq!(seqs(&write_all(&mut ring)), [5]);
}

#[test]
fn a_new_connection_replays_every_frame_not_confirmed_oldest_first() {
    let (mut ring, mut writer) = written_ring(4);
    ring.connected();
    let replay = write_all(&mut ring);
    assert_eq!(seqs(&replay), [1, 2, 3, 4]);
    assert_eq!(flagged(&replay, AudioFlags::BREAK), [0_u64; 0], "nothing is missing");

    capture(&mut writer, 128, 1);
    assert_eq!(seqs(&write_all(&mut ring)), [5], "the stream goes on after the replay");
}

#[test]
fn an_overrun_drops_the_oldest_and_breaks_the_first_frame_written_after_the_gap() {
    let mut ring = SpeechRing::new(4, 16);
    let mut writer = ring.open_capture();
    capture(&mut writer, 0, 2);
    ring.connected();
    assert_eq!(seqs(&write_all(&mut ring)), [1, 2]);

    capture(&mut writer, 64, 6);
    let written = write_all(&mut ring);
    assert_eq!(seqs(&written), [5, 6, 7, 8], "the store keeps its newest 4");
    assert_eq!(flagged(&written, AudioFlags::BREAK), [5], "3 and 4 were never written");
    assert_eq!(ring.take_dropped(), 4, "1 to 4 left the full store");
    assert_eq!(ring.take_dropped(), 0, "the count was taken");
}

#[test]
fn an_overrun_before_the_first_connection_breaks_the_replay_first_frame() {
    let mut ring = SpeechRing::new(2, 16);
    let mut writer = ring.open_capture();
    capture(&mut writer, 0, 3);
    ring.pull();
    ring.connected();
    let replay = write_all(&mut ring);
    assert_eq!(seqs(&replay), [2, 3]);
    assert_eq!(flagged(&replay, AudioFlags::BREAK), [2], "frame 1 was dropped unwritten");
}

#[test]
fn an_overrun_of_frames_written_but_not_confirmed_breaks_the_next_connection() {
    let mut ring = SpeechRing::new(2, 16);
    let mut writer = ring.open_capture();
    capture(&mut writer, 0, 2);
    ring.connected();
    assert_eq!(seqs(&write_all(&mut ring)), [1, 2]);
    capture(&mut writer, 64, 1);
    ring.pull();

    let written = write_all(&mut ring);
    assert_eq!(flagged(&written, AudioFlags::BREAK), [0_u64; 0], "this connection has read 1");
    ring.connected();
    let replay = write_all(&mut ring);
    assert_eq!(seqs(&replay), [2, 3]);
    assert_eq!(flagged(&replay, AudioFlags::BREAK), [2], "a new connection never got 1");
}

#[test]
fn a_store_at_its_cap_drops_nothing() {
    let mut ring = SpeechRing::new(4, 16);
    let mut writer = ring.open_capture();
    capture(&mut writer, 0, 4);
    ring.connected();
    let written = write_all(&mut ring);
    assert_eq!(seqs(&written), [1, 2, 3, 4]);
    assert_eq!(flagged(&written, AudioFlags::BREAK), [0_u64; 0]);
    assert_eq!(ring.take_dropped(), 0);
}

#[test]
fn the_product_store_holds_180_s_of_frames() {
    assert_eq!(STORE_FRAMES * AUDIO_FRAME_SAMPLES, 180 * 16_000, "180 s at 16 kHz");

    let mut ring = SpeechRing::new(STORE_FRAMES, HANDOFF_FRAMES);
    let mut writer = ring.open_capture();
    let frames = u64::try_from(STORE_FRAMES).unwrap() + 1;
    for n in 0..frames {
        assert!(writer.push(&samples(n), Duration::from_millis(32 * n), CaptureFlags::default()));
        ring.pull();
    }
    ring.connected();
    let replay = write_all(&mut ring);
    assert_eq!(replay.len(), STORE_FRAMES, "180 s, then the oldest goes");
    assert_eq!(replay.first().map(|(seq, _)| *seq), Some(2));
    assert_eq!(ring.take_dropped(), 1);
}

#[test]
fn a_full_handoff_loses_the_newest_and_breaks_the_first_frame_after_the_gap() {
    let mut ring = SpeechRing::new(16, 2);
    let mut writer = ring.open_capture();
    assert_eq!(capture(&mut writer, 0, 4), [true, true, false, false], "room for two");
    ring.connected();
    let written = write_all(&mut ring);
    assert_eq!(seqs(&written), [1, 2]);

    capture(&mut writer, 128, 1);
    let after = write_all(&mut ring);
    assert_eq!(seqs(&after), [5], "3 and 4 used their numbers");
    assert_eq!(flagged(&after, AudioFlags::BREAK), [5]);
    assert_eq!(ring.take_dropped(), 2);
}

#[test]
fn a_frame_written_more_than_250_ms_after_capture_is_replayed() {
    let mut ring = SpeechRing::new(16, 16);
    let mut writer = ring.open_capture();
    // Captured at 0, 32, ..., 320 ms, all written once the frame of 320 ms is in: ages 320,
    // 288, 256, 224 ms and less.
    capture(&mut writer, 0, 11);
    ring.connected();
    let written = write_all(&mut ring);
    assert_eq!(flagged(&written, AudioFlags::REPLAYED), [1, 2, 3]);
}

#[test]
fn a_frame_exactly_250_ms_old_is_not_replayed() {
    let mut ring = SpeechRing::new(4, 4);
    let mut writer = ring.open_capture();
    assert!(writer.push(&samples(0), Duration::ZERO, CaptureFlags::default()));
    assert!(writer.push(&samples(1), Duration::from_millis(250), CaptureFlags::default()));
    ring.connected();
    assert_eq!(flagged(&write_all(&mut ring), AudioFlags::REPLAYED), [0_u64; 0]);

    let mut ring = SpeechRing::new(4, 4);
    let mut writer = ring.open_capture();
    assert!(writer.push(&samples(0), Duration::ZERO, CaptureFlags::default()));
    assert!(writer.push(&samples(1), Duration::from_millis(251), CaptureFlags::default()));
    ring.connected();
    assert_eq!(flagged(&write_all(&mut ring), AudioFlags::REPLAYED), [1], "251 ms old");
}

#[test]
fn frames_waiting_when_capture_stops_are_never_written() {
    let mut ring = SpeechRing::new(16, 16);
    let mut writer = ring.open_capture();
    capture(&mut writer, 0, 2);
    ring.connected();
    assert_eq!(ring.next_frame().map(|frame| frame.seq), Some(1));
    // Frame 2 waits in the store, 3 and 4 in the handoff.
    capture(&mut writer, 64, 2);
    ring.capture_stopped();

    assert!(ring.next_frame().is_none(), "not on this connection");
    ring.connected();
    assert!(ring.next_frame().is_none(), "nor on the next");

    let mut resumed = ring.open_capture();
    capture(&mut resumed, 500, 2);
    let written = write_all(&mut ring);
    assert_eq!(seqs(&written), [5, 6]);
    assert_eq!(flagged(&written, AudioFlags::BREAK), [5], "the first frame after capture resumed");
}

#[test]
fn the_first_frame_after_a_stop_breaks_even_when_nothing_was_waiting() {
    let (mut ring, _writer) = written_ring(3);
    ring.capture_stopped();
    let mut resumed = ring.open_capture();
    capture(&mut resumed, 500, 2);
    let written = write_all(&mut ring);
    assert_eq!(seqs(&written), [4, 5], "no number was skipped");
    assert_eq!(flagged(&written, AudioFlags::BREAK), [4]);
}

#[test]
fn a_frame_pushed_after_capture_stopped_is_never_written() {
    let (mut ring, mut writer) = written_ring(1);
    ring.capture_stopped();
    capture(&mut writer, 32, 1);
    ring.connected();
    assert!(ring.next_frame().is_none(), "the stopped stream's handoff is gone");
}

#[test]
fn the_capture_push_allocates_and_frees_nothing() {
    let mut ring = SpeechRing::new(4, 2);
    let mut writer = ring.open_capture();
    let frame = samples(1);
    let flags = CaptureFlags { playback: true, other_audio: true };
    let mut handed = [false; 4];
    let counted = allocation_counter::measure(|| {
        // Two fit and two find the handoff full, so both paths run.
        for (n, slot) in (0_u64..).zip(handed.iter_mut()) {
            *slot = writer.push(&frame, Duration::from_millis(32 * n), flags);
        }
    });
    assert_eq!(counted, AllocationInfo::default(), "no allocation, reallocation or free");
    assert_eq!(handed, [true, true, false, false]);
}
