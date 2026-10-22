//! Entry points for the fuzz targets this crate owns (BUILD.md section 4.7): `linux_mountinfo`
//! (`/proc/self/mountinfo`, whose mount points and FUSE sources users choose: a USB stick's
//! label names its folder) and `linux_proc_status` (`/proc/<pid>/status`, whose process names
//! users choose). The seed corpora under `fuzz/corpus/<target>/` replay through these
//! functions as ordinary tests (`it::fuzz_corpus`). Each returns `Ok` for every input that
//! keeps the invariants it guards and `Err` naming the broken one; it never panics. The test and
//! the cargo-fuzz targets turn an `Err` into a failure.
//!
//! Bytes are decoded lossily, as `uia_text` decodes them, so every input reaches the parser.
//! The cargo-fuzz targets themselves (`fuzz/fuzz_targets/<target>.rs`) wait for WP0's
//! `fuzz/Cargo.toml`. This crate builds only on Linux, so its corpora replay only there, which
//! is where fuzzing runs.

use crate::disk::{Mount, parse_mountinfo, unescape};
use crate::pressure::{Status, parse_status};

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
