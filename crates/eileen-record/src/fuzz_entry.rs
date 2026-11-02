//! The entry point of the `recording` fuzz target (BUILD.md section 4.7): recording input lines
//! into the reader. Its seed corpus, `fuzz/corpus/recording/`, replays as a test
//! (`it::fuzz_corpus`). It returns `Ok` for every input that keeps the invariants it guards and
//! `Err` naming the broken one; it never panics, because the runtime links the crate.

use std::io::Cursor;

use eileen_contracts::core_io::Input;
use eileen_contracts::record::RecordLine;
use serde_json::Value;

use crate::blob::{Blobs, MemoryBlobs};
use crate::line;
use crate::reader::{Reader, release_blobs};
use crate::trim::{is_setup, trim};
use crate::writer::Writer;

/// The inline limit the round trips use: small, so most strings of a seed go to the store.
const SMALL_INLINE: usize = 8;

/// Recording input lines: read as a recording against an empty store, every line that reads
/// writes and reads back to the same `RecordLine` with its long strings in the store, and
/// releasing what was written empties the store; any JSON line reads back to the same value
/// through stubs and escapes; and trimming what was read keeps the window, the setup inputs
/// before it and each kept hook input's answer, in order.
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
    trim_keeps_what_it_promises(&lines)
}

/// A JSON line reads back to the value written, whatever stubs and escapes it holds.
fn values_round_trip(raw: &[u8]) -> Result<(), &'static str> {
    let Ok(value) = serde_json::from_slice::<Value>(raw) else { return Ok(()) };
    let mut blobs = MemoryBlobs::default();
    let Ok(written) = line::encode(&value, SMALL_INLINE, &mut blobs) else {
        return Err("a JSON value could not be written");
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

/// The lines write and read back unchanged, and releasing them empties the store.
fn lines_round_trip(lines: &[RecordLine]) -> Result<(), &'static str> {
    let mut writer = Writer::new(Vec::new(), MemoryBlobs::default()).with_max_inline(SMALL_INLINE);
    for line in lines {
        writer.write(line).map_err(|_| "a line that was read could not be written")?;
    }
    let (text, mut blobs) = writer.finish().map_err(|_| "the writer could not finish")?;
    let read = Reader::new(Cursor::new(&text))
        .read_all(&blobs)
        .map_err(|_| "a written recording did not read back")?;
    if read != lines {
        return Err("a written recording read back different");
    }
    let report = release_blobs(Cursor::new(&text), &mut blobs)
        .map_err(|_| "a written recording's blobs could not be released")?;
    if !blobs.is_empty() || report.absent != 0 {
        return Err("releasing a recording left a blob, or released one it never stored");
    }
    if blobs.put(b"after").is_err() || blobs.len() != 1 {
        return Err("the store did not take a blob after a release");
    }
    Ok(())
}

/// Trimming at a window in the middle of the lines' numbers keeps what it promises.
fn trim_keeps_what_it_promises(lines: &[RecordLine]) -> Result<(), &'static str> {
    let seqs: Vec<u64> = lines
        .iter()
        .filter_map(|line| match line {
            RecordLine::Input(record) => Some(record.seq),
            RecordLine::HookAnswer(_) => None,
        })
        .collect();
    let (Some(low), Some(high)) = (seqs.iter().min(), seqs.iter().max()) else { return Ok(()) };
    let window = low.saturating_add((high - low) / 2)..=*high;
    let kept = trim(lines, &window);
    let mut rest = lines.iter();
    if !kept.iter().all(|line| rest.any(|from| from == line)) {
        return Err("trim kept a line out of order, or one that was not there");
    }
    let hooks: Vec<u64> = kept
        .iter()
        .filter_map(|line| match line {
            RecordLine::Input(record) if matches!(record.input, Input::Hook(..)) => {
                Some(record.seq)
            }
            RecordLine::Input(_) | RecordLine::HookAnswer(_) => None,
        })
        .collect();
    for line in lines {
        let must_keep = match line {
            RecordLine::Input(record) => {
                window.contains(&record.seq)
                    || (record.seq < *window.start() && is_setup(&record.input))
            }
            RecordLine::HookAnswer(answer) => hooks.contains(&answer.input),
        };
        if must_keep && !kept.contains(line) {
            return Err("trim dropped a window input, a setup input or a kept hook's answer");
        }
    }
    Ok(())
}
