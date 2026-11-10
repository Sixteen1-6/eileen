//! The local time zone's offsets from UTC, read from `/etc/localtime`, for `eileen-core`'s
//! `LocalTime` (BUILD.md section 3.3.3, decision D86). [`local_offsets`] has the shape of
//! `eileen-shell-win`'s zone reader: for a year, the offset in force as it starts and each change
//! through the end of the next year, as (`WallTime`, minutes east of UTC) pairs that the
//! composition root turns into `LocalTime`, since this crate may not depend on `eileen-core`.
//!
//! - `/etc/localtime` is a `TZif` file (`tzif` reads versions 1 to 4 as RFC 9636 specifies),
//!   usually a symbolic link into `/usr/share/zoneinfo`, which timedated and the desktop's
//!   settings write. Ubuntu's tzdata ships fat files, with transitions through 2037 and the
//!   footer's rule after that; a slim file, which zic writes by default, has the rule alone for
//!   every year after its last irregular change, so both are read (`rule`).
//! - The `TZ` environment variable, which glibc reads before `/etc/localtime`, is not read. The
//!   system's zone is what the desktop's clock shows and what timedated sets, and the Windows
//!   reader reads the system's zone too, never the C runtime's `TZ`; a `TZ` left in the
//!   environment that started Eileen, from a terminal say, would make Eileen's hour differ from
//!   the clock on the screen.
//! - Offsets are whole minutes, each rounded to the nearest minute, a half minute away from zero.
//!   Every offset with seconds is from before 1972: local mean times, such as New York's -4:56:02
//!   until 1883, and last Monrovia's -0:44:30, until January 1972 (tzdata 2026c). Rounding keeps
//!   a reading within 30 s of the zone's own, and a change it makes no change at all is left out.
//! - No deadline (BUILD.md section 2.1 rule 2): the file is root's, on the system's own disk,
//!   which no other process serves, as WP10's file-deadline record says of `/proc`. The open
//!   never waits for a FIFO's writer, anything but a regular file is refused, and the read is
//!   once, of at most [`MAX_TZIF_BYTES`].
//!
//! # Traps
//!
//! - Under a [`SysRoot`] other than `/`, an absolute symbolic link at `etc/localtime` still points
//!   into the real `/usr/share/zoneinfo`; a test tree links relatively.

mod rule;
mod tzif;

use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;

use eileen_contracts::ids::WallTime;
use rustix::fs::OFlags;

use crate::SysRoot;

pub(crate) use tzif::write as write_tzif;

/// The largest `/etc/localtime` read. The largest file of tzdata 2026c is under 4 KiB, and a file
/// at tzcode's own limits (2000 transitions, 256 types) is under 40 KiB.
pub const MAX_TZIF_BYTES: u64 = 64 * 1024;

/// Where the local time zone is.
const LOCALTIME: &str = "etc/localtime";

/// Microseconds in a second, `WallTime`'s unit.
const MICROS: i64 = 1_000_000;

/// The flags the file is opened with beside read-only: `O_NONBLOCK`, so a FIFO in its place
/// does not hold the open, and `O_NOCTTY`, so a terminal is never made the controlling one.
const OPEN_FLAGS: i32 = OFlags::NONBLOCK.union(OFlags::NOCTTY).bits().cast_signed();

/// The local time zone's offsets from UTC over two years.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneOffsets {
    /// Minutes east of UTC just before the first year starts, and until the first change.
    pub initial_min: i32,
    /// Each change from the first year's start, 00:00 UTC on January 1, until the same instant
    /// two years later: when it takes effect and the minutes east of UTC from then on. In order,
    /// each instant once, each offset different from the one before.
    pub changes: Vec<(WallTime, i32)>,
}

/// Why the local time zone could not be read. The caller logs it and uses UTC, as glibc does
/// when `/etc/localtime` is missing. None holds a path but the fixed `/etc/localtime`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ZoneError {
    /// `/etc/localtime` does not exist, or is a link to nothing: the system keeps UTC.
    #[error("/etc/localtime does not exist")]
    Missing,
    /// It could not be opened or read, or is not a regular file.
    #[error("/etc/localtime is unreadable: {detail}")]
    Unreadable {
        /// What went wrong: the I/O error's kind, or `not a regular file`.
        detail: String,
    },
    /// It is not a `TZif` file Eileen can read, or is over [`MAX_TZIF_BYTES`].
    #[error("/etc/localtime is not a time zone file: {reason}")]
    Malformed {
        /// What is wrong with it.
        reason: &'static str,
    },
}

impl ZoneError {
    /// Maps an error opening or reading the file, keeping only the error's kind: its text can
    /// carry a path.
    fn io(err: &io::Error) -> Self {
        if err.kind() == io::ErrorKind::NotFound {
            Self::Missing
        } else {
            Self::Unreadable { detail: err.kind().to_string() }
        }
    }
}

/// The local time zone's offsets from the start of `year` to the end of the next year, read from
/// `/etc/localtime`.
pub fn local_offsets(year: u16) -> Result<ZoneOffsets, ZoneError> {
    local_offsets_under(&SysRoot::system(), year)
}

/// [`local_offsets`] with `etc/localtime` read under `root`.
pub fn local_offsets_under(root: &SysRoot, year: u16) -> Result<ZoneOffsets, ZoneError> {
    offsets_of(&read_localtime(root)?, year)
}

/// The offsets a `TZif` file gives from the start of `year` to the end of the next year.
pub(crate) fn offsets_of(file: &[u8], year: u16) -> Result<ZoneOffsets, ZoneError> {
    let zone = tzif::Zone::parse(file).map_err(|reason| ZoneError::Malformed { reason })?;
    let (from, until) = window(year);
    let (initial, changes) = zone.offsets(from, until);
    Ok(in_minutes(initial, changes))
}

/// The two years from `year`'s start, as Unix times: the first second and the one after the last.
pub(crate) fn window(year: u16) -> (i64, i64) {
    let year = i64::from(year);
    (rule::start_of(year), rule::start_of(year + 2))
}

/// The bytes of `etc/localtime` under `root`, opened without waiting and read up to the limit.
fn read_localtime(root: &SysRoot) -> Result<Vec<u8>, ZoneError> {
    let file = File::options()
        .read(true)
        .custom_flags(OPEN_FLAGS)
        .open(root.path(LOCALTIME))
        .map_err(|err| ZoneError::io(&err))?;
    if !file.metadata().map_err(|err| ZoneError::io(&err))?.is_file() {
        return Err(ZoneError::Unreadable { detail: "not a regular file".to_owned() });
    }
    let mut bytes = Vec::new();
    // Up to one byte over the limit is read: a file that reaches it is too long.
    file.take(MAX_TZIF_BYTES + 1).read_to_end(&mut bytes).map_err(|err| ZoneError::io(&err))?;
    if u64::try_from(bytes.len()).is_ok_and(|length| length <= MAX_TZIF_BYTES) {
        Ok(bytes)
    } else {
        Err(ZoneError::Malformed { reason: "it is over 64 KiB" })
    }
}

/// Offsets in seconds to [`ZoneOffsets`]: whole minutes, each instant once (the later change at
/// an instant holds), and no change to the offset already in force.
fn in_minutes(initial: i32, changes: Vec<(i64, i32)>) -> ZoneOffsets {
    let initial_min = minutes(initial);
    let mut kept: Vec<(i64, i32)> = Vec::with_capacity(changes.len());
    for (at, offset) in changes {
        let offset = minutes(offset);
        match kept.last_mut() {
            Some(last) if last.0 == at => last.1 = offset,
            _ => kept.push((at, offset)),
        }
        let before = kept
            .len()
            .checked_sub(2)
            .and_then(|index| kept.get(index))
            .map_or(initial_min, |c| c.1);
        if kept.last().is_some_and(|last| last.1 == before) {
            kept.pop();
        }
    }
    let changes = kept
        .into_iter()
        .map(|(at, offset)| (WallTime(at.saturating_mul(MICROS)), offset))
        .collect();
    ZoneOffsets { initial_min, changes }
}

/// Seconds to the nearest whole minute, a half minute away from zero.
const fn minutes(seconds: i32) -> i32 {
    if seconds >= 0 { (seconds + 30) / 60 } else { (seconds - 30) / 60 }
}

#[cfg(test)]
mod tests;
