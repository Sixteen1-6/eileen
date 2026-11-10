//! `TZif` files, as RFC 9636 specifies them, versions 1 to 4: a header and a data block of 32-bit
//! times, and from version 2 a second header and data block of 64-bit times and a footer with
//! the rule for the time after the last transition ([`Rule`]).
//!
//! - A version 2 or later file is read from its second header on, as section 4 asks; the first
//!   data block is only skipped. A version after 4 is read as version 4, since each version is
//!   built so that a reader of the one before can read it.
//! - Every count is checked against what is left of the file before anything is read for it,
//!   so a header cannot make the reader allocate more than the file holds (section 7).
//! - Transition times are counted with leap seconds when the file has a leap-second table (the
//!   `right/` zones); each is moved back by the correction in force, so all times here are Unix
//!   times, as `WallTime` is. The footer's rule is taken as Unix time too: the seconds it would
//!   be off by do not change a minute's offset.
//! - Local time before the first transition is type 0's, and after the last the footer's, or
//!   the last transition's type's when the footer is empty, as readers commonly do (Appendix A).

use super::rule::Rule;

/// The length of a header.
const HEADER: usize = 44;
/// The bytes of a local time type record.
const TYPE_RECORD: usize = 6;
/// The fewest seconds east of UTC an offset may be: just over 25 hours west (section 3.2).
const MIN_OFFSET: i32 = -89_999;
/// The most seconds east of UTC an offset may be: just under 26 hours east.
const MAX_OFFSET: i32 = 93_599;
/// What a file too short for what its header says is.
const TRUNCATED: &str = "the file is shorter than its header says";

/// The local time a `TZif` file describes, its offsets in seconds east of UTC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Zone {
    /// Each transition, in order: its Unix time and the offset from then on.
    transitions: Vec<(i64, i32)>,
    /// The offset before the first transition: local time type 0's.
    first: i32,
    /// The rule after the last transition; `None` in a version 1 file or with an empty footer.
    footer: Option<Rule>,
}

/// A header's counts.
#[derive(Debug, Clone, Copy)]
struct Counts {
    /// UT/local indicators: none, or one per type.
    utc: usize,
    /// Standard/wall indicators: none, or one per type.
    standard: usize,
    /// Leap-second records.
    leaps: usize,
    /// Transitions.
    times: usize,
    /// Local time types, at least one.
    types: usize,
    /// Bytes of time zone designations, at least one.
    chars: usize,
}

impl Zone {
    /// Reads a `TZif` file.
    pub(super) fn parse(file: &[u8]) -> Result<Self, &'static str> {
        let (version, counts, rest) = header(file)?;
        if version == 0 {
            let (transitions, first, _) = block(rest, counts, 4)?;
            return Ok(Self { transitions, first, footer: None });
        }
        let rest = rest.get(counts.block_length(4)?..).ok_or(TRUNCATED)?;
        let (again, counts, rest) = header(rest)?;
        if again != version {
            return Err("the two headers give different versions");
        }
        let (mut transitions, first, rest) = block(rest, counts, 8)?;
        let footer = footer(rest)?;
        // From the last transition on, the footer gives local time (section 3.2), as glibc reads
        // it; in a file whose footer agrees with that transition, as section 3.3 asks, the two
        // are the same.
        if let (Some(rule), Some(last)) = (&footer, transitions.last_mut()) {
            last.1 = rule.offset_at(last.0);
        }
        Ok(Self { transitions, first, footer })
    }

    /// The offset in force just before `from` and each change at or after `from` and before
    /// `until`, in order, as (Unix time, the offset from then on). Two changes may share an
    /// instant, where a leap correction brought them together; the later one holds.
    pub(super) fn offsets(&self, from: i64, until: i64) -> (i32, Vec<(i64, i32)>) {
        let initial = self.offset_at(from.saturating_sub(1));
        let mut changes: Vec<(i64, i32)> =
            self.transitions.iter().copied().filter(|(at, _)| from <= *at && *at < until).collect();
        if let Some(rule) = &self.footer {
            let after =
                self.transitions.last().map_or(from, |(last, _)| from.max(last.saturating_add(1)));
            if after < until {
                changes.extend(rule.changes(after, until));
            }
        }
        (initial, changes)
    }

    /// The offset in force at `at`.
    fn offset_at(&self, at: i64) -> i32 {
        match (self.transitions.last(), &self.footer) {
            (None, Some(rule)) => rule.offset_at(at),
            (Some((last, _)), Some(rule)) if at >= *last => rule.offset_at(at),
            _ => {
                let after = self.transitions.partition_point(|(when, _)| *when <= at);
                after
                    .checked_sub(1)
                    .and_then(|last| self.transitions.get(last))
                    .map_or(self.first, |(_, offset)| *offset)
            }
        }
    }
}

impl Counts {
    /// The length of a data block with these counts and `time_size`-byte times; an error when
    /// it could not fit in any file.
    fn block_length(self, time_size: usize) -> Result<usize, &'static str> {
        let parts = [
            self.times.checked_mul(time_size + 1),
            self.types.checked_mul(TYPE_RECORD),
            Some(self.chars),
            self.leaps.checked_mul(time_size + 4),
            Some(self.standard),
            Some(self.utc),
        ];
        parts
            .into_iter()
            .try_fold(0_usize, |sum, part| part.and_then(|part| sum.checked_add(part)))
            .ok_or(TRUNCATED)
    }
}

/// Reads a header: the version byte (0 for version 1, else the version's digit), the counts,
/// and the rest of the file.
fn header(bytes: &[u8]) -> Result<(u8, Counts, &[u8]), &'static str> {
    let (head, rest) =
        bytes.split_first_chunk::<HEADER>().ok_or("the file ends inside a header")?;
    let mut read = Reader(head);
    if read.take(4)? != b"TZif" {
        return Err("the file is not TZif: no magic");
    }
    let version = read.byte()?;
    if version != 0 && !(b'2'..=b'9').contains(&version) {
        return Err("the file's TZif version is unknown");
    }
    read.take(15)?;
    let mut count = || read.u32().map(|count| usize::try_from(count).unwrap_or(usize::MAX));
    let counts = Counts {
        utc: count()?,
        standard: count()?,
        leaps: count()?,
        times: count()?,
        types: count()?,
        chars: count()?,
    };
    if counts.types == 0 || counts.chars == 0 {
        return Err("the file has no local time type or no designation");
    }
    if ![0, counts.types].contains(&counts.utc) || ![0, counts.types].contains(&counts.standard) {
        return Err("the file's indicators do not match its types");
    }
    Ok((version, counts, rest))
}

/// A data block read: its transitions as (Unix time, offset), type 0's offset, and the rest of
/// the file.
type Block<'a> = (Vec<(i64, i32)>, i32, &'a [u8]);

/// Reads a data block with `time_size`-byte times.
fn block(bytes: &[u8], counts: Counts, time_size: usize) -> Result<Block<'_>, &'static str> {
    let (block, rest) = bytes.split_at_checked(counts.block_length(time_size)?).ok_or(TRUNCATED)?;
    let mut read = Reader(block);
    let times = read.take(counts.times * time_size)?;
    let indexes = read.take(counts.times)?;
    let offsets = types(read.take(counts.types * TYPE_RECORD)?, counts.chars)?;
    let designations = read.take(counts.chars)?;
    if designations.last() != Some(&0) {
        return Err("the file's designations do not end with a NUL");
    }
    let leaps = leaps(read.take(counts.leaps * (time_size + 4))?, time_size)?;
    let mut transitions = Vec::with_capacity(counts.times);
    let mut previous = None;
    for (time, index) in times.chunks_exact(time_size).zip(indexes) {
        let at = Reader(time).time(time_size)?;
        if previous.is_some_and(|previous| at <= previous) {
            return Err("the file's transitions are out of order");
        }
        previous = Some(at);
        let offset =
            *offsets.get(usize::from(*index)).ok_or("a transition names a type the file lacks")?;
        transitions.push((at.saturating_sub(correction(&leaps, at)), offset));
    }
    let first = offsets.first().copied().ok_or("the file has no local time type")?;
    Ok((transitions, first, rest))
}

/// The offsets of the local time type records, checked: each offset within a day or so of UTC,
/// a daylight saving flag of 0 or 1, and a designation index within the designations.
fn types(records: &[u8], chars: usize) -> Result<Vec<i32>, &'static str> {
    let (records, _) = records.as_chunks::<TYPE_RECORD>();
    records
        .iter()
        .map(|&[a, b, c, d, dst, designation]| {
            let offset = i32::from_be_bytes([a, b, c, d]);
            if !(MIN_OFFSET..=MAX_OFFSET).contains(&offset) {
                Err("a local time type's offset is a day or more")
            } else if dst > 1 || usize::from(designation) >= chars {
                Err("a local time type is malformed")
            } else {
                Ok(offset)
            }
        })
        .collect()
}

/// The leap-second records, as (occurrence, correction), in strictly ascending order.
fn leaps(records: &[u8], time_size: usize) -> Result<Vec<(i64, i64)>, &'static str> {
    let mut leaps: Vec<(i64, i64)> = Vec::with_capacity(records.len() / (time_size + 4));
    for record in records.chunks_exact(time_size + 4) {
        let mut read = Reader(record);
        let at = read.time(time_size)?;
        let correction = i64::from(read.i32()?);
        if leaps.last().is_some_and(|(previous, _)| at <= *previous) {
            return Err("the file's leap seconds are out of order");
        }
        leaps.push((at, correction));
    }
    Ok(leaps)
}

/// The leap-second correction in force at `at`: that of the last record at or before it, or none
/// before the first.
fn correction(leaps: &[(i64, i64)], at: i64) -> i64 {
    let after = leaps.partition_point(|(when, _)| *when <= at);
    after.checked_sub(1).and_then(|last| leaps.get(last)).map_or(0, |(_, correction)| *correction)
}

/// Reads the footer: a newline, the `TZ` string and a newline. What follows is not read.
fn footer(bytes: &[u8]) -> Result<Option<Rule>, &'static str> {
    let text = bytes.strip_prefix(b"\n").ok_or("the file has no footer")?;
    let end =
        text.iter().position(|byte| *byte == b'\n').ok_or("the footer has no closing newline")?;
    match text.get(..end) {
        None | Some([]) => Ok(None),
        Some(rule) => Rule::parse(rule).map(Some),
    }
}

/// What is left of a header or a record to read, big-endian.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    /// The next `length` bytes.
    fn take(&mut self, length: usize) -> Result<&'a [u8], &'static str> {
        let (taken, rest) = self.0.split_at_checked(length).ok_or(TRUNCATED)?;
        self.0 = rest;
        Ok(taken)
    }

    /// The next byte.
    fn byte(&mut self) -> Result<u8, &'static str> {
        Ok(self.array::<1>()?[0])
    }

    /// The next `N` bytes.
    fn array<const N: usize>(&mut self) -> Result<[u8; N], &'static str> {
        let (taken, rest) = self.0.split_first_chunk::<N>().ok_or(TRUNCATED)?;
        self.0 = rest;
        Ok(*taken)
    }

    /// A four-byte unsigned count.
    fn u32(&mut self) -> Result<u32, &'static str> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    /// A four-byte signed value.
    fn i32(&mut self) -> Result<i32, &'static str> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    /// A time of `size` bytes, 4 or 8.
    fn time(&mut self, size: usize) -> Result<i64, &'static str> {
        if size == 4 { self.i32().map(i64::from) } else { Ok(i64::from_be_bytes(self.array()?)) }
    }
}

/// A `TZif` file of `version` (b'2' or later) with the same data in both blocks, for tests and the
/// fuzz entry: `transitions` as (Unix time, type), `types` as (seconds east of UTC, daylight
/// saving), each type designated `"ZZZ"`, and `footer` as its `TZ` string. A 32-bit block cannot
/// hold a time outside its range, so the first block keeps only the transitions that fit.
pub(crate) fn write(
    version: u8,
    transitions: &[(i64, u8)],
    types: &[(i32, bool)],
    footer: &[u8],
) -> Vec<u8> {
    let narrow: Vec<(i64, u8)> =
        transitions.iter().copied().filter(|(at, _)| i32::try_from(*at).is_ok()).collect();
    let mut file = Vec::new();
    write_block(&mut file, version, &narrow, types, 4);
    write_block(&mut file, version, transitions, types, 8);
    file.push(b'\n');
    file.extend_from_slice(footer);
    file.push(b'\n');
    file
}

/// Appends a header and its data block.
fn write_block(
    file: &mut Vec<u8>,
    version: u8,
    transitions: &[(i64, u8)],
    types: &[(i32, bool)],
    time_size: usize,
) {
    let count = |count: usize| u32::try_from(count).unwrap_or(u32::MAX).to_be_bytes();
    file.extend_from_slice(b"TZif");
    file.push(version);
    file.extend_from_slice(&[0; 15]);
    for value in [0, 0, 0, transitions.len(), types.len(), 4] {
        file.extend_from_slice(&count(value));
    }
    for (at, _) in transitions {
        let bytes = at.to_be_bytes();
        file.extend_from_slice(bytes.get(8 - time_size..).unwrap_or_default());
    }
    file.extend(transitions.iter().map(|(_, index)| *index));
    for (offset, dst) in types {
        file.extend_from_slice(&offset.to_be_bytes());
        file.extend_from_slice(&[u8::from(*dst), 0]);
    }
    file.extend_from_slice(b"ZZZ\0");
}

#[cfg(test)]
mod tests;
