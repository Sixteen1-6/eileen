//! The fast path's "what time is it" (BUILD.md section 2.4): the local time of the turn, from
//! its wall-clock instant and the local offset the runtime read from the PC. The core reads no
//! clock of its own.

use eileen_contracts::ids::WallTime;

/// Microseconds in a minute.
const MICROS_PER_MINUTE: i64 = 60_000_000;
/// Minutes in a day.
const MINUTES_PER_DAY: i64 = 1_440;

/// The spoken answer for `at`, `offset_min` minutes east of UTC: "It's 3:05 pm."
pub(crate) fn spoken_time(at: WallTime, offset_min: i32) -> String {
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

    use super::spoken_time;

    /// 2026-01-01T00:00:00Z in microseconds.
    const MIDNIGHT_UTC: i64 = 1_767_225_600_000_000;
    const MINUTE: i64 = 60_000_000;

    #[test]
    fn the_time_is_spoken_in_twelve_hour_form() {
        assert_eq!(spoken_time(WallTime(MIDNIGHT_UTC), 0), "It's 12:00 am.", "midnight");
        assert_eq!(spoken_time(WallTime(MIDNIGHT_UTC + 905 * MINUTE), 0), "It's 3:05 pm.", "15:05");
        assert_eq!(spoken_time(WallTime(MIDNIGHT_UTC + 720 * MINUTE), 0), "It's 12:00 pm.", "noon");
        assert_eq!(spoken_time(WallTime(MIDNIGHT_UTC + 59 * MINUTE), 0), "It's 12:59 am.", "00:59");
    }

    #[test]
    fn the_local_offset_moves_the_hour_across_midnight() {
        assert_eq!(
            spoken_time(WallTime(MIDNIGHT_UTC + 30 * MINUTE), -360),
            "It's 6:30 pm.",
            "UTC-6 is the evening before"
        );
        assert_eq!(spoken_time(WallTime(MIDNIGHT_UTC), 330), "It's 5:30 am.", "UTC+5:30");
    }
}
