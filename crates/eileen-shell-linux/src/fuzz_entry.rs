//! Entry points for the fuzz targets this crate owns (BUILD.md section 4.7): `linux_mountinfo`
//! (`/proc/self/mountinfo`, whose mount points and FUSE sources users choose: a USB stick's
//! label names its folder), `linux_proc_status` (`/proc/<pid>/status`, whose process names
//! users choose) and `linux_tzif` (`/etc/localtime`, which timedated, a package or an
//! administrator writes). The seed corpora under `fuzz/corpus/<target>/` replay through these
//! functions as ordinary tests (`it::fuzz_corpus`). Each returns `Ok` for every input that
//! keeps the invariants it guards and `Err` naming the broken one; it never panics. The test and
//! the cargo-fuzz targets turn an `Err` into a failure.
//!
//! Bytes are decoded lossily, as `uia_text` decodes them, so every input reaches the parser.
//! The cargo-fuzz targets themselves are `fuzz/fuzz_targets/<target>.rs`. This crate builds only
//! on Linux, so its corpora replay only there, which is where fuzzing runs.

use eileen_contracts::ids::WallTime;

use crate::disk::{Mount, parse_mountinfo, unescape};
use crate::pressure::{Status, parse_status};
use crate::zone::{ZoneOffsets, offsets_of, window, write_tzif};

/// The years each `linux_tzif` input is read from: the calendar's first, the year before the
/// epoch, this one, the next (the third common year after a leap year), the last whole year of
/// 32-bit time and the next, and the last two years a `Date` holds. Each is read with the year
/// after it, and the two must agree.
const TZIF_YEARS: [u16; 7] = [1, 1969, 2026, 2027, 2037, 2038, 9998];

/// The most minutes east or west of UTC an offset of a `TZif` file can round to: 26 hours.
const TZIF_MAX_MINUTES: i32 = 26 * 60;

/// What the kernel escapes in a mount point, each as a backslash and three octal digits.
const POINT_ESCAPES: &[(char, &str)] =
    &[(' ', r"\040"), ('\t', r"\011"), ('\n', r"\012"), ('\\', r"\134")];

/// What it escapes in a file system type and a source (its `mangle`): the same and `#`.
const MANGLE_ESCAPES: &[(char, &str)] =
    &[(' ', r"\040"), ('\t', r"\011"), ('\n', r"\012"), ('\\', r"\134"), ('#', r"\043")];

/// `/proc/self/mountinfo` text. Its mounts are bounded by the input: no more of them than it
/// has lines, and no more text than it holds. Resolving a field's escapes never makes it longer,
/// and only a backslash and three octal digits make an escape. And the kernel's escaping
/// round-trips: each mount read, and the input itself as a mount point, a type and a source,
/// written back as the kernel writes them, reads back unchanged, so no escape is left
/// unresolved or resolved twice.
pub fn linux_mountinfo(data: &[u8]) -> Result<(), &'static str> {
    let text = String::from_utf8_lossy(data).into_owned();
    let mounts = parse_mountinfo(&text);
    if mounts.len() > text.lines().count() {
        return Err("mountinfo gave more mounts than it has lines");
    }
    let kept: usize = mounts
        .iter()
        .map(|mount| mount.point.len() + mount.fstype.len() + mount.source.len())
        .sum();
    if kept > text.len() {
        return Err("mountinfo's mounts hold more text than the input");
    }
    for field in text.lines().flat_map(|line| line.split(' ')) {
        let resolved = unescape(field);
        if resolved.len() > field.len() {
            return Err("resolving a field's escapes made it longer");
        }
        if resolved != field && !has_octal_escape(field) {
            return Err("a field without an octal escape changed when its escapes were resolved");
        }
    }
    for mount in &mounts {
        reads_back(mount)?;
    }
    reads_back(&Mount {
        point: text.clone(),
        fstype: escape(&text, MANGLE_ESCAPES),
        source: text.clone(),
    })
}

/// `/proc/<pid>/status` text. A process read from it is named by one of its `Name` lines, so
/// its name is no longer than the input and holds no newline. And a name cannot change what else
/// the file says: the input as a process name, written as the kernel writes one, reads back as
/// written beside the parent and the resident size the file gives.
pub fn linux_proc_status(data: &[u8]) -> Result<(), &'static str> {
    let text = String::from_utf8_lossy(data).into_owned();
    if let Some(status) = parse_status(&text) {
        let named = text
            .lines()
            .filter_map(|line| line.strip_prefix("Name:"))
            .any(|name| name.trim() == status.name);
        if !named {
            return Err("a process's name is not what a Name line says");
        }
    }
    let name = kernel_name(&text);
    let file = format!(
        "Name:\t{name}\nUmask:\t0022\nState:\tS (sleeping)\nTgid:\t4300\nPid:\t4300\n\
         PPid:\t4120\nTracerPid:\t0\nVmRSS:\t  610000 kB\nThreads:\t1\n"
    );
    let expected = Status { name: name.trim().to_owned(), ppid: 4120, rss_kb: Some(610_000) };
    if parse_status(&file) == Some(expected) {
        Ok(())
    } else {
        Err("a process name changed what the rest of its status file says")
    }
}

/// `/etc/localtime`'s bytes, and the input as the `TZ` string of a file that has nothing else.
/// Whether a file reads does not depend on the year. Each read gives its changes in order,
/// within its two years, each a different offset from the one before and within 26 hours of
/// UTC, and no more of them than the file holds transitions and its rule can make. And two
/// consecutive years agree on the year they share: the same offset as it starts and the same
/// changes in it.
pub fn linux_tzif(data: &[u8]) -> Result<(), &'static str> {
    tzif_reads_alike(data)?;
    let rule: Vec<u8> = data.iter().copied().filter(|byte| *byte != b'\n').collect();
    tzif_reads_alike(&write_tzif(b'3', &[], &[(0, false)], &rule))
}

/// Reads `file` for each of [`TZIF_YEARS`] and the year after, checking each read and each pair.
fn tzif_reads_alike(file: &[u8]) -> Result<(), &'static str> {
    let first = offsets_of(file, TZIF_YEARS[0]).is_ok();
    for year in TZIF_YEARS {
        let (this, next) = (offsets_of(file, year), offsets_of(file, year + 1));
        if this.is_ok() != first || next.is_ok() != first {
            return Err("a TZif file read for one year and not for another");
        }
        let (Ok(this), Ok(next)) = (this, next) else { continue };
        tzif_read_holds(&this, year, file.len())?;
        tzif_read_holds(&next, year + 1, file.len())?;
        let start = WallTime(window(year + 1).0 * 1_000_000);
        let until = WallTime(window(year).1 * 1_000_000);
        let at_start = this
            .changes
            .iter()
            .take_while(|(at, _)| *at < start)
            .last()
            .map_or(this.initial_min, |(_, offset)| *offset);
        if at_start != next.initial_min {
            return Err("two years disagree on the offset as the year they share starts");
        }
        let shared = this.changes.iter().filter(|(at, _)| *at >= start);
        if !shared.eq(next.changes.iter().filter(|(at, _)| *at < until)) {
            return Err("two years disagree on the changes in the year they share");
        }
    }
    Ok(())
}

/// The checks of one read of a file `length` bytes long, from `year`.
fn tzif_read_holds(read: &ZoneOffsets, year: u16, length: usize) -> Result<(), &'static str> {
    let (from, until) = window(year);
    let (from, until) = (WallTime(from * 1_000_000), WallTime(until * 1_000_000));
    // A transition takes at least five bytes of a file; a rule makes two changes a year, and a
    // change's time can bring one from each year on either side into the two.
    if read.changes.len() > length / 5 + 8 {
        return Err("a TZif file gave more changes than it can hold");
    }
    let mut previous = (None, read.initial_min);
    for &(at, offset) in &read.changes {
        if at < from || at >= until {
            return Err("a change is outside the two years read");
        }
        if previous.0.is_some_and(|before| at <= before) || offset == previous.1 {
            return Err("a change is out of order or changes nothing");
        }
        previous = (Some(at), offset);
    }
    let mut offsets =
        std::iter::once(read.initial_min).chain(read.changes.iter().map(|(_, offset)| *offset));
    if offsets.any(|offset| offset.abs() > TZIF_MAX_MINUTES) {
        return Err("an offset is more than 26 hours");
    }
    Ok(())
}

/// Writes `mount` as the kernel writes its line, with optional fields, and reads it back: it
/// must come back as the one mount it was.
fn reads_back(mount: &Mount) -> Result<(), &'static str> {
    let line = format!(
        "36 25 0:50 / {} rw,nosuid shared:7 master:2 - {} {} rw,size=1024k\n",
        escape(&mount.point, POINT_ESCAPES),
        mount.fstype,
        escape(&mount.source, MANGLE_ESCAPES),
    );
    match parse_mountinfo(&line).as_slice() {
        [again] if again == mount => Ok(()),
        [_] => Err("a mount written as the kernel writes it read back different"),
        _ => Err("a mount written as the kernel writes it did not read back as one mount"),
    }
}

/// `field` as the kernel writes it in mountinfo, with `escapes`.
fn escape(field: &str, escapes: &[(char, &str)]) -> String {
    let mut out = String::with_capacity(field.len());
    for c in field.chars() {
        match escapes.iter().find(|(special, _)| *special == c) {
            Some((_, code)) => out.push_str(code),
            None => out.push(c),
        }
    }
    out
}

/// Whether `text` holds an octal escape: a backslash and three octal digits, up to `\377`.
fn has_octal_escape(text: &str) -> bool {
    text.split('\\')
        .skip(1)
        .any(|after| matches!(after.as_bytes(), [b'0'..=b'3', b'0'..=b'7', b'0'..=b'7', ..]))
}

/// `name` as the kernel writes it on a status file's `Name` line: a newline as `\n` and a
/// backslash as `\\`, the rest as it is.
fn kernel_name(name: &str) -> String {
    name.replace('\\', r"\\").replace('\n', r"\n")
}
