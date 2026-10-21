//! Monotonic time, wall time and calendar dates.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::ParseError;

/// Monotonic nanoseconds since the core started. Only the clock port creates it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct MonoTime(pub u64);

impl JsonSchema for MonoTime {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("MonoTime")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "format": "uint64",
            "minimum": 0,
            "description": "Monotonic nanoseconds since the core started",
        })
    }
}

/// Microseconds since the Unix epoch, UTC.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct WallTime(pub i64);

impl WallTime {
    /// Microseconds in one UTC day.
    const MICROS_PER_DAY: i64 = 86_400_000_000;

    /// The UTC calendar date this instant falls on.
    #[must_use]
    pub fn date(self) -> Date {
        Date::from_days_since_epoch(self.0.div_euclid(Self::MICROS_PER_DAY))
    }
}

impl JsonSchema for WallTime {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("WallTime")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "format": "int64",
            "description": "Microseconds since the Unix epoch, UTC",
        })
    }
}

/// A calendar date in UTC, serialized `YYYY-MM-DD` and validated on parse. Years run from 1 to
/// 9999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    /// The year, 1 to 9999.
    pub y: u16,
    /// The month, 1 to 12.
    pub m: u8,
    /// The day of the month, 1 to its length.
    pub d: u8,
}

impl Date {
    /// A date, if it exists on the calendar.
    pub const fn new(y: u16, m: u8, d: u8) -> Result<Self, ParseError> {
        let date = Self { y, m, d };
        if date.is_valid() {
            Ok(date)
        } else {
            Err(ParseError::Invalid { what: "Date", rule: "a calendar date from 0001-01-01" })
        }
    }

    /// True when the fields name a real calendar day in years 1 to 9999.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.y >= 1 && self.y <= 9999 && self.m >= 1 && self.m <= 12 && self.d >= 1 && {
            self.d <= days_in_month(self.y, self.m)
        }
    }

    /// Days from `self` to `later`: positive when `later` is after `self`, zero on the same day.
    #[must_use]
    pub fn days_until(self, later: Self) -> i64 {
        later.days_since_epoch() - self.days_since_epoch()
    }

    /// Days since 1970-01-01 (negative before it), by the proleptic Gregorian calendar.
    #[must_use]
    pub fn days_since_epoch(self) -> i64 {
        // Howard Hinnant's days_from_civil, with March as the first month of the year.
        let month = i64::from(self.m);
        let y = i64::from(self.y) - i64::from(month <= 2);
        let era = y.div_euclid(400);
        let year_of_era = y - era * 400;
        let shifted_month = (month + 9) % 12;
        let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(self.d) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The date `days` after 1970-01-01, clamped to years 1 to 9999.
    #[must_use]
    pub fn from_days_since_epoch(days: i64) -> Self {
        const FIRST: i64 = -719_162; // 0001-01-01
        const LAST: i64 = 2_932_896; // 9999-12-31
        let z = days.clamp(FIRST, LAST) + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * day_of_year + 2) / 153;
        let d = day_of_year - (153 * shifted_month + 2) / 5 + 1;
        let m = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
        let y = year_of_era + era * 400 + i64::from(m <= 2);
        Self {
            y: u16::try_from(y).unwrap_or(1),
            m: u8::try_from(m).unwrap_or(1),
            d: u8::try_from(d).unwrap_or(1),
        }
    }
}

/// The number of days in a month of a year, by the Gregorian leap rule.
const fn days_in_month(y: u16, m: u8) -> u8 {
    match m {
        2 if y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400)) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

impl FromStr for Date {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        const INVALID: ParseError =
            ParseError::Invalid { what: "Date", rule: "YYYY-MM-DD, a calendar date" };
        let bytes = text.as_bytes();
        let [y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2] = bytes else { return Err(INVALID) };
        if ![y1, y2, y3, y4, m1, m2, d1, d2].iter().all(|b| b.is_ascii_digit()) {
            return Err(INVALID);
        }
        let n = |b: u8| b - b'0';
        let y = u16::from(n(*y1)) * 1000
            + u16::from(n(*y2)) * 100
            + u16::from(n(*y3)) * 10
            + u16::from(n(*y4));
        Self::new(y, n(*m1) * 10 + n(*m2), n(*d1) * 10 + n(*d2)).map_err(|_| INVALID)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Cow::<'de, str>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Date {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Date")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$",
            "description": "A calendar date in UTC, YYYY-MM-DD",
        })
    }
}
