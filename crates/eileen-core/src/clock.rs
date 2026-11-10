//! The fast path's "what time is it" (BUILD.md section 2.4): the local time of the turn, from
//! its wall-clock instant and the PC's offsets from UTC, which the runtime read from the time
//! zone's rules at start ([`LocalTime`], section 3.3.3). The core reads no clock of its own.

use eileen_contracts::ids::WallTime;

/// Microseconds in a minute.
const MICROS_PER_MINUTE: i64 = 60_000_000;
/// Minutes in a day.
const MINUTES_PER_DAY: i64 = 1_440;

/// The PC's local time (section 3.3.3), which the fast path's "what time is it" speaks. The
/// runtime fills it at start from the time zone's rules for the current year and the next, so
/// a daylight-saving change while Eileen runs moves the hour with no restart; a change of time
/// zone takes effect at the next start. When the runtime cannot read the zone's rules it logs a
/// warning and uses UTC, which is also the default: `initial_offset_min` 0 and no changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalTime {
    /// Minutes east of UTC before the first change, or always with none.
    pub initial_offset_min: i32,
    /// The changes of offset. The runtime sorts them by `from`, but the core does not rely on
    /// the order.
    pub changes: Vec<LocalOffset>,
}

/// One change of the PC's offset from UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalOffset {
    /// When the offset takes effect.
    pub from: WallTime,
    /// Minutes east of UTC from `from` on.
    pub offset_min: i32,
}

impl LocalTime {
    /// An offset that never changes, `offset_min` minutes east of UTC.
    #[must_use]
    pub const fn fixed(offset_min: i32) -> Self {
        Self { initial_offset_min: offset_min, changes: Vec::new() }
    }

    /// The offset in force at `at`, in minutes east of UTC: the change with the latest `from`
    /// at or before `at`, whatever the order of the list, or the initial offset before the
    /// first change.
    #[must_use]
    pub fn offset_at(&self, at: WallTime) -> i32 {
        let in_force = self.changes.iter().filter(|change| change.from <= at);
        in_force
            .max_by_key(|change| change.from)
            .map_or(self.initial_offset_min, |change| change.offset_min)
    }
}

/// The spoken answer for `at` in `local`'s time: "It's 3:05 pm."
pub(crate) fn spoken_time(at: WallTime, local: &LocalTime) -> String {
    let offset_min = local.offset_at(at);
    let local = at.0.saturating_add(i64::from(offset_min).saturating_mul(MICROS_PER_MINUTE));
    let minute_of_day = local.div_euclid(MICROS_PER_MINUTE).rem_euclid(MINUTES_PER_DAY);
    let (hour, minute) = (minute_of_day / 60, minute_of_day % 60);
    let (hour, half) = match hour {
        0 => (12, "am"),
        1..=11 => (hour, "am"),
        12 => (12, "pm"),
        _ => (hour - 12, "pm"),
    };
    format!("It's {hour}:{minute:02} {half}.")
}

#[cfg(test)]
mod tests {
    use eileen_contracts::ids::WallTime;

    use super::{LocalOffset, LocalTime, spoken_time};

    /// 2026-01-01T00:00:00Z in microseconds.
    const MIDNIGHT_UTC: i64 = 1_767_225_600_000_000;
    const MINUTE: i64 = 60_000_000;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    /// 2026-03-08T07:00:00Z: 2 am Eastern Standard Time becomes 3 am daylight time.
    const DST_BEGINS: i64 = MIDNIGHT_UTC + 66 * DAY + 7 * HOUR;
    /// 2026-11-01T06:00:00Z: 2 am Eastern Daylight Time becomes 1 am standard time.
    const DST_ENDS: i64 = MIDNIGHT_UTC + 304 * DAY + 6 * HOUR;

    /// New York's rules for 2026, as the runtime would read them, in the given order.
    fn new_york(changes: [(i64, i32); 2]) -> LocalTime {
        let changes =
            changes.map(|(from, offset_min)| LocalOffset { from: WallTime(from), offset_min });
        LocalTime { initial_offset_min: -300, changes: changes.to_vec() }
    }

    #[test]
    fn the_time_is_spoken_in_twelve_hour_form() {
        let utc = LocalTime::default();
        let at = |minutes: i64| spoken_time(WallTime(MIDNIGHT_UTC + minutes * MINUTE), &utc);
        assert_eq!(at(0), "It's 12:00 am.", "midnight");
        assert_eq!(at(905), "It's 3:05 pm.", "15:05");
        assert_eq!(at(720), "It's 12:00 pm.", "noon");
        assert_eq!(at(59), "It's 12:59 am.", "00:59");
    }

    #[test]
    fn the_local_offset_moves_the_hour_across_midnight() {
        assert_eq!(
            spoken_time(WallTime(MIDNIGHT_UTC + 30 * MINUTE), &LocalTime::fixed(-360)),
            "It's 6:30 pm.",
            "UTC-6 is the evening before"
        );
        assert_eq!(
            spoken_time(WallTime(MIDNIGHT_UTC), &LocalTime::fixed(330)),
            "It's 5:30 am.",
            "UTC+5:30"
        );
    }

    #[test]
    fn the_hour_moves_at_a_daylight_saving_change() {
        let sorted = new_york([(DST_BEGINS, -240), (DST_ENDS, -300)]);
        let unsorted = new_york([(DST_ENDS, -300), (DST_BEGINS, -240)]);
        for local in [&sorted, &unsorted] {
            let at = |micros: i64| spoken_time(WallTime(micros), local);
            assert_eq!(at(MIDNIGHT_UTC + 17 * HOUR), "It's 12:00 pm.", "January, standard time");
            assert_eq!(at(DST_BEGINS - MINUTE), "It's 1:59 am.", "the minute before spring");
            assert_eq!(at(DST_BEGINS), "It's 3:00 am.", "the hour skipped in spring");
            assert_eq!(at(DST_ENDS - 10 * MINUTE), "It's 1:50 am.", "daylight time, November");
            assert_eq!(at(DST_ENDS), "It's 1:00 am.", "the hour repeated in the fall");
            assert_eq!(at(DST_ENDS + 10 * MINUTE), "It's 1:10 am.", "standard time again");
            assert_eq!(local.offset_at(WallTime(i64::MIN)), -300, "before every change");
        }
    }
}
