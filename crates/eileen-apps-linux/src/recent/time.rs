//! A bookmark's times, read as `GLib`'s `g_date_time_new_from_iso8601` reads them
//! (<https://gitlab.gnome.org/GNOME/glib/-/blob/main/glib/gdatetime.c>): ISO 8601 calendar
//! dates and times. That is the extended form that `GLib` and KIO write,
//! `2026-10-21T16:40:11.902114Z`, the basic form, `20261021T164011Z`, or a mix of the two; `T`,
//! `t` or a space before the time; an optional fraction after `.` or `,`, kept to the
//! microsecond; a leap second as :59; and a required zone, `Z` or an offset such as `+02:00`,
//! `+0200` or `+02`, its hours up to 24.
//!
//! This reader differs from `GLib` in two ways: the ordinal and week dates that `GLib` also
//! reads (`2026-294`, `2026-W43-3`) do not parse, since neither `GLib` nor KIO writes them; and
//! a fraction too long for `GLib`'s 64-bit arithmetic parses, where `GLib` refuses it, since only
//! six digits count.

use eileen_contracts::ids::{Date, WallTime};

/// An ISO 8601 calendar date and time, such as `2026-10-21T16:40:11.902114Z` or
/// `20261021T164011Z`, in microseconds since the Unix epoch; the module doc gives the forms it
/// takes.
pub(super) fn parse_time(text: &str) -> Option<WallTime> {
    let mut at = Cursor(text.as_bytes());
    // The extended form separates the fields and the basic form does not. `GLib` tells the
    // date's form and the time's apart, so each may be either.
    let year = at.number(4)?;
    let extended = at.skip(b"-");
    let month = at.number(2)?;
    if extended {
        at.expect(b"-")?;
    }
    let day = at.number(2)?;
    at.expect(b"Tt ")?;
    let hour = at.number(2)?;
    let extended = at.skip(b":");
    let minute = at.number(2)?;
    if extended {
        at.expect(b":")?;
    }
    // GLib reads a leap second as :59.
    let second = match at.number(2)? {
        60 | 61 => 59,
        second => second,
    };
    let micros = if at.skip(b".,") { at.fraction()? } else { 0 };
    let offset = at.zone()?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let date =
        Date::new(u16::try_from(year).ok()?, u8::try_from(month).ok()?, u8::try_from(day).ok()?)
            .ok()?;
    let seconds =
        date.days_since_epoch() * 86_400 + i64::from(hour * 3_600 + minute * 60 + second) - offset;
    Some(WallTime(seconds * 1_000_000 + micros))
}

/// What is left of a time's text.
struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    /// The value of the next `count` bytes, which must be ASCII digits.
    fn number(&mut self, count: usize) -> Option<u32> {
        let (digits, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        digits.iter().try_fold(0, |value, digit| {
            digit.is_ascii_digit().then(|| value * 10 + u32::from(digit - b'0'))
        })
    }

    /// The next byte, which must be one of `bytes`.
    fn expect(&mut self, bytes: &[u8]) -> Option<u8> {
        let (&first, rest) = self.0.split_first()?;
        bytes.contains(&first).then(|| {
            self.0 = rest;
            first
        })
    }

    /// Whether the next byte is one of `bytes`, taking it if so.
    fn skip(&mut self, bytes: &[u8]) -> bool {
        self.expect(bytes).is_some()
    }

    /// The microseconds of a fraction's digits, of which there must be one or more. Digits
    /// past the sixth are dropped.
    fn fraction(&mut self) -> Option<i64> {
        let count = self.0.iter().take_while(|byte| byte.is_ascii_digit()).count();
        let (digits, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        (count > 0).then(|| {
            (0..6).fold(0, |micros, at| {
                micros * 10 + digits.get(at).map_or(0, |digit| i64::from(digit - b'0'))
            })
        })
    }

    /// The zone that ends the text, as seconds east of UTC: `Z`, or `+` or `-` and `hh:mm`,
    /// `hhmm` or `hh`, its hours up to 24 as `GLib` allows.
    fn zone(&mut self) -> Option<i64> {
        let east = match self.expect(b"Z+-")? {
            b'Z' => return self.0.is_empty().then_some(0),
            sign => sign == b'+',
        };
        let hours = self.number(2)?;
        let minutes = if self.0.is_empty() {
            0
        } else {
            self.skip(b":");
            self.number(2)?
        };
        if !self.0.is_empty() || hours > 24 || minutes > 59 {
            return None;
        }
        let offset = i64::from(hours * 3_600 + minutes * 60);
        Some(if east { offset } else { -offset })
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::ids::WallTime;

    use super::parse_time;

    // The times' values are those GLib 2.80's `g_date_time_new_from_iso8601` gives for the same
    // text, except in the test of where the two differ.
    #[test]
    fn times_read_as_glib_and_kio_write_them() {
        let at = |text: &str| parse_time(text).map(|time| time.0);
        let glib = Some(1_792_600_811_902_114);
        assert_eq!(at("1970-01-01T00:00:00Z"), Some(0), "the epoch");
        assert_eq!(at("2026-10-21T16:40:11.902114Z"), glib, "GLib's microseconds");
        assert_eq!(at("2026-10-21T16:40:11Z"), Some(1_792_600_811_000_000), "no fraction");
        assert_eq!(at("2026-10-21T16:40:11.902000Z"), Some(1_792_600_811_902_000), "KIO's");
        assert_eq!(at("2026-10-21T16:40:11.902Z"), Some(1_792_600_811_902_000), "milliseconds");
        assert_eq!(at("2026-10-21T16:40:11.90211499Z"), glib, "past microseconds, dropped");
        assert_eq!(at("2026-10-21T18:40:11.902114+02:00"), glib, "an offset east");
        assert_eq!(at("2026-10-21T11:10:11.902114-0530"), glib, "an offset west, no colon");
        assert_eq!(at("2026-10-21T17:40:11.902114+01"), glib, "an offset in hours");
        assert_eq!(at("2026-10-21 16:40:11,902114Z"), glib, "a space and a comma");
        assert_eq!(at("2026-10-21t16:40:11.902114Z"), glib, "a lowercase t");
        assert_eq!(at("1969-12-31T23:59:59.5Z"), Some(-500_000), "before the epoch");
        assert_eq!(at("2016-12-31T23:59:60Z"), Some(1_483_228_799_000_000), "a leap second");
        assert_eq!(at("2000-02-29T12:00:00Z"), Some(951_825_600_000_000), "a leap day");
        assert_eq!(at("9999-12-31T23:59:59.999999Z"), Some(253_402_300_799_999_999), "the last");
    }

    #[test]
    fn the_basic_form_and_offsets_of_24_hours_read_as_glib_reads_them() {
        let at = |text: &str| parse_time(text).map(|time| time.0);
        let second = Some(1_792_600_811_000_000);
        assert_eq!(at("20261021T164011Z"), second, "the basic form");
        assert_eq!(at("20261021T164011.902114Z"), Some(1_792_600_811_902_114), "a fraction");
        assert_eq!(at("20261021T164011,5Z"), Some(1_792_600_811_500_000), "after a comma");
        assert_eq!(at("20261021T184011+0200"), second, "an offset");
        assert_eq!(at("20261021T16:40:11Z"), second, "a basic date, an extended time");
        assert_eq!(at("2026-10-21T164011Z"), second, "an extended date, a basic time");
        assert_eq!(at("20261021 164011Z"), second, "a space");
        assert_eq!(at("20261021t164011Z"), second, "a lowercase t");
        assert_eq!(at("2026-10-22T16:40:11+2400"), second, "24 hours east");
        assert_eq!(at("2026-10-20T15:41:11-24:59"), second, "24 hours and 59 minutes west");
    }

    #[test]
    fn times_glib_refuses_do_not_parse() {
        for text in [
            "",
            "yesterday",
            "2026-10-21T16:40:11",
            "2026-10-21T16:40:11z",
            "2026-10-21T16:40:11.Z",
            "2026-10-21T16:40Z",
            "2026-10-21T24:00:00Z",
            "2026-10-21T16:60:00Z",
            "2026-10-21T16:40:62Z",
            "2026-02-29T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "0000-01-01T00:00:00Z",
            "2026-10-21T16:40:11+25:00",
            "2026-10-21T16:40:11+02:60",
            "2026-10-21T16:40:11+02:0",
            "2026-10-21T16:40:11Z ",
            "2026-10-21X16:40:11Z",
            "2026-1021T16:40:11Z",
            "202610-21T16:40:11Z",
            "2026-10-21T16:4011Z",
            "2026-10-21T1640:11Z",
            "20261021T1640Z",
            "20261021T164011",
            "20261021T246000Z",
            "20260229T000000Z",
        ] {
            assert_eq!(parse_time(text), None, "{text:?} does not parse");
        }
        assert!(
            parse_time("2026-10-21T16:40:11Z") > parse_time("2026-10-21T16:40:10.999999Z"),
            "order"
        );
        assert_eq!(parse_time("1970-01-01T00:00:01Z"), Some(WallTime(1_000_000)), "one second");
    }

    #[test]
    fn where_glib_differs_ordinal_and_week_dates_fail_and_long_fractions_parse() {
        for text in [
            "2026-294T16:40:11Z",
            "2026294T16:40:11Z",
            "2026-W43-3T16:40:11Z",
            "2026W433T16:40:11Z",
        ] {
            assert_eq!(
                parse_time(text),
                None,
                "{text:?}: GLib reads it, and neither it nor KIO writes it"
            );
        }
        assert_eq!(
            parse_time("2026-10-21T16:40:11.1111111111111111111Z"),
            Some(WallTime(1_792_600_811_111_111)),
            "19 digits, which GLib refuses, kept to six"
        );
    }
}
