//! The entry point of the `recording` fuzz target (BUILD.md section 4.7): recording input lines
//! into the reader. Its seed corpus, `fuzz/corpus/recording/`, replays as a test
//! (`it::fuzz_corpus`). It returns `Ok` for every input that keeps the invariants it guards and
//! `Err` naming the broken one; it never panics, because the runtime links the crate.

use std::collections::BTreeSet;
use std::io::Cursor;

use eileen_contracts::core_io::{Input, UserInput};
use eileen_contracts::record::RecordLine;
use serde_json::Value;

use crate::blob::{Blobs, MemoryBlobs};
use crate::line::{self, LineError};
use crate::reader::{Reader, release_blobs};
use crate::trim::{Selection, SetupKind, setup_kind, trim};
use crate::writer::{WriteError, Writer};

/// The inline limit the round trips use: small, so most strings of a seed go to the store.
const SMALL_INLINE: usize = 8;

/// Recording input lines: read as a recording against an empty store, every line that reads
/// and is not too deep to write writes and reads back to the same `RecordLine` with its long
/// strings in the store, and releasing what was written empties the store; any JSON line that
/// writes reads back to the same value through stubs and escapes, and one that does not write
/// leaves the store empty; and trimming what was read to turns and jobs the input picks keeps
/// exactly what trim promises.
pub fn recording(data: &[u8]) -> Result<(), &'static str> {
    let empty = MemoryBlobs::default();
    let mut reader = Reader::new(Cursor::new(data));
    let mut lines = Vec::new();
    while let Some(read) = reader.next_line(&empty) {
        if let Ok(line) = read {
            lines.push(line);
        }
    }
    for raw in data.split(|byte| *byte == b'\n') {
        values_round_trip(raw)?;
    }
    lines_round_trip(&lines)?;
    trim_keeps_what_it_promises(&lines, data)
}

/// A JSON line reads back to the value written, whatever stubs and escapes it holds, unless it
/// is nested too deep to write, which leaves no reference behind.
fn values_round_trip(raw: &[u8]) -> Result<(), &'static str> {
    let Ok(value) = serde_json::from_slice::<Value>(raw) else { return Ok(()) };
    let mut blobs = MemoryBlobs::default();
    let written = match line::encode(&value, SMALL_INLINE, &mut blobs) {
        Ok(written) => written,
        Err(LineError::TooDeep { .. }) if blobs.is_empty() => return Ok(()),
        Err(LineError::TooDeep { .. }) => return Err("a value too deep to write kept a blob"),
        Err(_) => return Err("a JSON value could not be written"),
    };
    if written.contains('\n') {
        return Err("a written line holds a newline");
    }
    match line::decode::<Value, _>(written.as_bytes(), &blobs) {
        Ok(read) if read == value => Ok(()),
        Ok(_) => Err("a JSON value read back different"),
        Err(_) => Err("a written JSON value did not read back"),
    }
}

/// The lines that write read back unchanged, only one too deep fails to write, and releasing
/// what was written empties the store.
fn lines_round_trip(lines: &[RecordLine]) -> Result<(), &'static str> {
    let mut writer = Writer::new(Vec::new(), MemoryBlobs::default()).with_max_inline(SMALL_INLINE);
    let mut written = Vec::new();
    for line in lines {
        match writer.write(line) {
            Ok(()) => written.push(line.clone()),
            Err(WriteError::Line(LineError::TooDeep { .. })) => {}
            Err(_) => return Err("a line that was read could not be written"),
        }
    }
    let (text, mut blobs) = writer.finish().map_err(|_| "the writer could not finish")?;
    let read = Reader::new(Cursor::new(&text))
        .read_all(&blobs)
        .map_err(|_| "a written recording did not read back")?;
    if read != written {
        return Err("a written recording read back different");
    }
    let report = release_blobs(Cursor::new(&text), &mut blobs)
        .map_err(|_| "a written recording's blobs could not be released")?;
    if !blobs.is_empty() || report.absent != 0 || report.damaged != 0 || report.failed != 0 {
        return Err("releasing a recording left a blob, or released one it never stored");
    }
    if blobs.put(b"after").is_err() || blobs.len() != 1 {
        return Err("the store did not take a blob after a release");
    }
    Ok(())
}

/// Trimming to the turns and jobs a byte of the input picks keeps exactly the lines trim
/// promises, worked out here from the lines alone, position by position.
fn trim_keeps_what_it_promises(lines: &[RecordLine], data: &[u8]) -> Result<(), &'static str> {
    let pick = data.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte));
    let selection = pick_selection(lines, pick);
    let inputs: Vec<_> = lines
        .iter()
        .map(|line| match line {
            RecordLine::Input(record) => Some(record),
            RecordLine::HookAnswer(_) => None,
        })
        .collect();
    let chosen: Vec<bool> =
        inputs.iter().map(|record| record.is_some_and(|r| selection.holds(&r.ctx))).collect();
    let first = chosen.iter().position(|chosen| *chosen);
    let last = chosen.iter().rposition(|chosen| *chosen);
    let hooks: BTreeSet<u64> = inputs
        .iter()
        .zip(&chosen)
        .filter_map(|(record, chosen)| {
            record.filter(|r| *chosen && matches!(r.input, Input::Hook(..))).map(|r| r.seq)
        })
        .collect();
    let kinds: Vec<Option<SetupKind>> =
        inputs.iter().map(|record| record.and_then(|r| setup_kind(&r.input))).collect();
    let set_mute = |at: usize| {
        inputs
            .get(at)
            .copied()
            .flatten()
            .is_some_and(|r| matches!(r.input, Input::User(UserInput::SetMute(_))))
    };
    let mut expected = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let keep = match (line, first, last) {
            (_, None, _) | (_, _, None) => false,
            (RecordLine::HookAnswer(answer), _, _) => hooks.contains(&answer.input),
            (RecordLine::Input(record), Some(first), Some(last)) => {
                let kind = kinds.get(at).copied().flatten();
                let later = |found: &dyn Fn(usize) -> bool| (at + 1..first).any(found);
                let latest = match kind {
                    Some(SetupKind::Mute) => !later(&set_mute),
                    Some(kind) => !later(&|other| kinds.get(other) == Some(&Some(kind))),
                    None => false,
                };
                chosen.get(at).copied().unwrap_or(false)
                    || (at < first && latest)
                    || (first < at
                        && at < last
                        && kind.is_some()
                        && !selection.foreign(&record.ctx))
            }
        };
        if keep {
            expected.push(line.clone());
        }
    }
    if trim(lines, &selection) == expected {
        Ok(())
    } else {
        Err("trim kept other lines than its selection, the setup inputs and the kept answers")
    }
}

/// The turns and jobs named in the lines, each kept or not by one bit of `pick` in turn.
fn pick_selection(lines: &[RecordLine], pick: u8) -> Selection {
    let mut turns = BTreeSet::new();
    let mut jobs = BTreeSet::new();
    for line in lines {
        if let RecordLine::Input(record) = line {
            turns.extend(record.ctx.turn);
            jobs.extend(record.ctx.job);
        }
    }
    let mut bits = (0_u32..).map(|bit| pick.rotate_right(bit) & 1 == 1);
    let mut take = || bits.next().unwrap_or(false);
    Selection {
        turns: turns.into_iter().filter(|_| take()).collect(),
        jobs: jobs.into_iter().filter(|_| take()).collect(),
    }
}
