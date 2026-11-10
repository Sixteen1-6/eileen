//! A `TZif` footer's rule (RFC 9636 section 3.3): a POSIX `TZ` string (Base Definitions, section
//! 8.3), which gives local time after the file's last transition. A slim file, as `zic -b slim`
//! writes, stops its transitions where the rules become regular, so for every later year this
//! rule is all there is.
//!
//! - The grammar is POSIX's, `std offset [dst [offset] [,start[/time],end[/time]]]`, with RFC 9636
//!   section 3.3.2's extension: a change's time may be signed and run from -167 to 167 hours
//!   (`America/Nuuk`'s `M3.5.0/-1`, `Asia/Jerusalem`'s `M3.4.4/26`). The extension is read in
//!   every version, since a version 2 file that used it could mean nothing else.
//! - A day is `Jn` (1 to 365, February 29 never counted), `n` (0 to 365, counted) or `Mm.w.d`
//!   (weekday `d`, 0 for Sunday, of week `w` of month `m`, week 5 being the last).
//! - With a daylight saving name and no changes, the changes are `M3.2.0,M11.1.0`, as glibc and
//!   tzcode take them; a change with no time is at 02:00.
//! - Each year has its own pair of changes, the start in standard time and the end in daylight
//!   saving time. A year whose two changes fall at the same instant has no daylight saving time,
//!   as glibc reads it. One whose daylight saving time lasts a whole year or more keeps it all
//!   year, as section 3.3.1 and tzcode say; glibc 2.39 shows standard time in the hours of
//!   January 1 before the year's start, which no zone of tzdata 2026c's has.

/// Seconds in a day.
const DAY: i64 = 86_400;
/// Seconds in an hour.
const HOUR: i32 = 3_600;
/// The most hours an offset may have (POSIX).
const OFFSET_HOURS: i32 = 24;
/// The most hours a change's time may have (RFC 9636 section 3.3.2).
const TIME_HOURS: i32 = 167;
/// A change's time when the rule gives none: 02:00.
const DEFAULT_TIME: i32 = 2 * HOUR;
/// The changes when the rule has a daylight saving name and none: the second Sunday of March
/// and the first of November.
const DEFAULT_CHANGES: (Change, Change) = (
    Change { day: Day::Weekday { month: 3, week: 2, weekday: 0 }, time: DEFAULT_TIME },
    Change { day: Day::Weekday { month: 11, week: 1, weekday: 0 }, time: DEFAULT_TIME },
);

/// A footer's rule, its offsets in seconds east of UTC (POSIX writes them west of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rule {
    /// Standard time's offset.
    std: i32,
    /// Daylight saving time, when the rule has it.
    dst: Option<Dst>,
}

/// A rule's daylight saving time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dst {
    /// Its offset, seconds east of UTC.
    offset: i32,
    /// When it starts each year, in standard time.
    start: Change,
    /// When it ends each year, in daylight saving time.
    end: Change,
}

/// When in a year a change happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Change {
    /// The day.
    day: Day,
    /// Seconds after that day's local midnight, from -167 to 167 hours.
    time: i32,
}

/// A day of a year, in a rule's three forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Day {
    /// `Jn`: day `n` of the year, 1 to 365, February 29 never counted.
    NoLeap(u16),
    /// `n`: day `n` of the year from 0, 0 to 365, February 29 counted.
    FromZero(u16),
    /// `Mm.w.d`: weekday `d` (0 is Sunday) of week `w` (1 to 5, 5 the last) of month `m`.
    Weekday {
        /// The month, 1 to 12.
        month: u8,
        /// The week, 1 to 5.
        week: u8,
        /// The weekday, 0 (Sunday) to 6.
        weekday: u8,
    },
}

impl Rule {
    /// Reads a footer's `TZ` string.
    pub(super) fn parse(text: &[u8]) -> Result<Self, &'static str> {
        let mut at = Cursor(text);
        at.name()?;
        let std = -at.hms(OFFSET_HOURS)?;
        if at.0.is_empty() {
            return Ok(Self { std, dst: None });
        }
        at.name()?;
        let offset = if matches!(at.peek(), Some(b'0'..=b'9' | b'+' | b'-')) {
            -at.hms(OFFSET_HOURS)?
        } else {
            std + HOUR
        };
        let (start, end) = if at.0.is_empty() {
            DEFAULT_CHANGES
        } else {
            at.expect(b',')?;
            let start = at.change()?;
            at.expect(b',')?;
            (start, at.change()?)
        };
        if !at.0.is_empty() {
            return Err("the footer's rule has text after its end");
        }
        Ok(Self { std, dst: Some(Dst { offset, start, end }) })
    }

    /// The offset in force at `at`, seconds since the epoch.
    pub(super) fn offset_at(&self, at: i64) -> i32 {
        let year = year_of(at);
        self.events(year - 2, year + 1)
            .into_iter()
            .take_while(|(when, _)| *when <= at)
            .last()
            .map_or(self.std, |(_, offset)| offset)
    }

    /// Each change of offset at or after `from` and before `until`, in order, as (seconds since
    /// the epoch, the offset from then on).
    pub(super) fn changes(&self, from: i64, until: i64) -> Vec<(i64, i32)> {
        let mut events = self.events(year_of(from) - 1, year_of(until) + 1);
        events.retain(|(at, _)| from <= *at && *at < until);
        events
    }

    /// The changes the rule makes in the years `first` to `last`, in order. A change's time can
    /// move it by up to a week into the year before or after, so callers ask for a year more on
    /// each side than they need.
    fn events(&self, first: i64, last: i64) -> Vec<(i64, i32)> {
        let Some(dst) = self.dst else { return Vec::new() };
        let mut events = Vec::new();
        for year in first..=last {
            let (Some(start), Some(end)) =
                (dst.start.instant(year, self.std), dst.end.instant(year, dst.offset))
            else {
                continue;
            };
            if start == end {
                continue;
            }
            events.push((start, dst.offset));
            if end < start || end - start < year_length(year) * DAY {
                events.push((end, self.std));
            }
        }
        // At one instant the end of one year's daylight saving time comes before the start of the
        // next year's, so daylight saving time that ends where it starts again goes on.
        events.sort_by_key(|&(at, offset)| (at, offset != self.std));
        events
    }
}

impl Change {
    /// When the change happens in `year`, seconds since the epoch, on a clock `offset` seconds
    /// east of UTC; `None` past what the arithmetic holds.
    fn instant(self, year: i64, offset: i32) -> Option<i64> {
        let day = match self.day {
            Day::NoLeap(n) => {
                let after_february = is_leap(year) && n >= 60;
                days_from_civil(year, 1, 1) + i64::from(n) - 1 + i64::from(after_february)
            }
            Day::FromZero(n) => days_from_civil(year, 1, 1) + i64::from(n),
            Day::Weekday { month, week, weekday } => weekday_in(year, month, week, weekday),
        };
        day.checked_mul(DAY)?.checked_add(i64::from(self.time))?.checked_sub(i64::from(offset))
    }
}

/// The day, counted from 1970-01-01, of weekday `weekday` in week `week` of `month` in `year`:
/// week 1 holds the month's first such weekday, and week 5 is the month's last.
fn weekday_in(year: i64, month: u8, week: u8, weekday: u8) -> i64 {
    let first = days_from_civil(year, month, 1);
    // 1970-01-01 was a Thursday, weekday 4.
    let first_weekday = (first + 4).rem_euclid(7);
    let day =
        first + (i64::from(weekday) - first_weekday).rem_euclid(7) + 7 * (i64::from(week) - 1);
    if day >= first + month_length(year, month) { day - 7 } else { day }
}

/// Days from 1970-01-01 to the given date of the proleptic Gregorian calendar, negative before
/// it: Howard Hinnant's `days_from_civil`, as `eileen_contracts::ids::Date` counts, for any year.
fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let month = i64::from(month);
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The proleptic Gregorian year that second `at` since the epoch falls in, in UTC.
pub(super) fn year_of(at: i64) -> i64 {
    let days = at.div_euclid(DAY) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // The count starts in March, so January and February belong to the next year.
    year_of_era + era * 400 + i64::from(day_of_year >= 306)
}

/// The first second of `year`, in UTC.
pub(super) fn start_of(year: i64) -> i64 {
    days_from_civil(year, 1, 1) * DAY
}

/// Whether `year` has February 29.
const fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// The days of `year`.
const fn year_length(year: i64) -> i64 {
    if is_leap(year) { 366 } else { 365 }
}

/// The days of `month` in `year`.
const fn month_length(year: i64, month: u8) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// What is left of a `TZ` string to read.
struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    /// The next byte.
    const fn peek(&self) -> Option<u8> {
        self.0.first().copied()
    }

    /// Takes `byte` when it comes next.
    const fn eat(&mut self, byte: u8) -> bool {
        match self.0.split_first() {
            Some((&first, rest)) if first == byte => {
                self.0 = rest;
                true
            }
            _ => false,
        }
    }

    /// Takes `byte`, which must come next.
    const fn expect(&mut self, byte: u8) -> Result<(), &'static str> {
        if self.eat(byte) { Ok(()) } else { Err("the footer's rule lacks a separator") }
    }

    /// Takes the bytes up to the first that `keep` refuses; how many it took.
    fn skip_while(&mut self, keep: impl Fn(u8) -> bool) -> usize {
        let taken = self.0.iter().position(|byte| !keep(*byte)).unwrap_or(self.0.len());
        self.0 = self.0.get(taken..).unwrap_or_default();
        taken
    }

    /// A zone name: three or more letters, or three or more letters, digits, `+` and `-` in
    /// angle brackets. Only its place matters.
    fn name(&mut self) -> Result<(), &'static str> {
        let length = if self.eat(b'<') {
            let length =
                self.skip_while(|byte| byte.is_ascii_alphanumeric() || b"+-".contains(&byte));
            self.expect(b'>')?;
            length
        } else {
            self.skip_while(|byte| byte.is_ascii_alphabetic())
        };
        if length < 3 { Err("the footer's rule lacks a zone name") } else { Ok(()) }
    }

    /// One or more digits whose value is at most `max`.
    fn number(&mut self, max: i32) -> Result<i32, &'static str> {
        let digits = self.0.iter().take_while(|byte| byte.is_ascii_digit()).count();
        if digits == 0 {
            return Err("the footer's rule lacks a number");
        }
        let mut value: i32 = 0;
        for byte in self.0.iter().take(digits) {
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(i32::from(byte - b'0')))
                .filter(|value| *value <= max)
                .ok_or("a number in the footer's rule is out of range")?;
        }
        self.0 = self.0.get(digits..).unwrap_or_default();
        Ok(value)
    }

    /// A number from `min` to `max`.
    fn number_in(&mut self, min: i32, max: i32) -> Result<i32, &'static str> {
        let value = self.number(max)?;
        if value < min { Err("a number in the footer's rule is out of range") } else { Ok(value) }
    }

    /// `[+|-]hh[:mm[:ss]]` in seconds, with at most `max_hours` hours.
    fn hms(&mut self, max_hours: i32) -> Result<i32, &'static str> {
        let negative = self.eat(b'-');
        if !negative {
            self.eat(b'+');
        }
        let mut seconds = self.number(max_hours)? * HOUR;
        if self.eat(b':') {
            seconds += self.number(59)? * 60;
            if self.eat(b':') {
                seconds += self.number(59)?;
            }
        }
        Ok(if negative { -seconds } else { seconds })
    }

    /// A change: its day, then `/` and its time when it has one.
    fn change(&mut self) -> Result<Change, &'static str> {
        let day = if self.eat(b'J') {
            Day::NoLeap(small(self.number_in(1, 365)?))
        } else if self.eat(b'M') {
            let month = self.number_in(1, 12)?;
            self.expect(b'.')?;
            let week = self.number_in(1, 5)?;
            self.expect(b'.')?;
            let weekday = self.number_in(0, 6)?;
            Day::Weekday { month: tiny(month), week: tiny(week), weekday: tiny(weekday) }
        } else {
            Day::FromZero(small(self.number(365)?))
        };
        let time = if self.eat(b'/') { self.hms(TIME_HOURS)? } else { DEFAULT_TIME };
        Ok(Change { day, time })
    }
}

/// A number of at most 365 as a day of the year.
fn small(value: i32) -> u16 {
    u16::try_from(value).unwrap_or_default()
}

/// A number of at most 12 as a month, week or weekday.
fn tiny(value: i32) -> u8 {
    u8::try_from(value).unwrap_or_default()
}

#[cfg(test)]
mod tests;
