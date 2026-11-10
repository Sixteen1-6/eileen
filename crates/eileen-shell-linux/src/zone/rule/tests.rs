use eileen_contracts::ids::Date;
use proptest::prelude::*;

use super::{Change, DEFAULT_CHANGES, Day, Dst, HOUR, Rule, start_of, year_of};

/// The first second of a UTC date.
fn midnight(y: u16, m: u8, d: u8) -> i64 {
    Date::new(y, m, d).unwrap().days_since_epoch() * 86_400
}

fn parse(text: &str) -> Result<Rule, &'static str> {
    Rule::parse(text.as_bytes())
}

fn weekday(month: u8, week: u8, weekday: u8, time: i32) -> Change {
    Change { day: Day::Weekday { month, week, weekday }, time }
}

#[test]
fn each_form_of_the_rule_reads() {
    let us = Dst { offset: -4 * HOUR, start: DEFAULT_CHANGES.0, end: DEFAULT_CHANGES.1 };
    let cases = [
        ("UTC0", Rule { std: 0, dst: None }),
        ("IST-5:30", Rule { std: 19_800, dst: None }),
        ("<+0545>-5:45", Rule { std: 20_700, dst: None }),
        ("<-03>+3", Rule { std: -3 * HOUR, dst: None }),
        ("LMT-0:53:28", Rule { std: 3_208, dst: None }),
        ("EST5EDT", Rule { std: -5 * HOUR, dst: Some(us) }),
        ("EST5EDT,M3.2.0,M11.1.0", Rule { std: -5 * HOUR, dst: Some(us) }),
        ("EST5EDT4,M3.2.0/2,M11.1.0/02:00:00", Rule { std: -5 * HOUR, dst: Some(us) }),
        (
            "<+1245>-12:45<+1345>,M9.5.0/2:45,M4.1.0/3:45",
            Rule {
                std: 45_900,
                dst: Some(Dst {
                    offset: 49_500,
                    start: weekday(9, 5, 0, 9_900),
                    end: weekday(4, 1, 0, 13_500),
                }),
            },
        ),
        (
            "<-02>2<-01>,M3.5.0/-1,M10.5.0/0",
            Rule {
                std: -2 * HOUR,
                dst: Some(Dst {
                    offset: -HOUR,
                    start: weekday(3, 5, 0, -HOUR),
                    end: weekday(10, 5, 0, 0),
                }),
            },
        ),
        (
            "AAA-1BBB,J60/-167:59:59,300/+167",
            Rule {
                std: HOUR,
                dst: Some(Dst {
                    offset: 2 * HOUR,
                    start: Change { day: Day::NoLeap(60), time: -(167 * HOUR + 3_599) },
                    end: Change { day: Day::FromZero(300), time: 167 * HOUR },
                }),
            },
        ),
    ];
    for (text, rule) in cases {
        assert_eq!(parse(text), Ok(rule), "{text}");
    }
}

#[test]
fn a_malformed_rule_is_refused() {
    let cases = [
        "",
        "EST",
        "ES5",
        "<+05",
        "<+5>-5",
        ":America/New_York",
        "EST5EDT,M3.2.0",
        "EST5EDT,M3.2.0,",
        "EST5EDT;M3.2.0,M11.1.0",
        "EST5EDT,M13.1.0,M11.1.0",
        "EST5EDT,M0.1.0,M11.1.0",
        "EST5EDT,M3.6.0,M11.1.0",
        "EST5EDT,M3.0.0,M11.1.0",
        "EST5EDT,M3.2.7,M11.1.0",
        "EST5EDT,M3.2,M11.1.0",
        "EST5EDT,J0,J365",
        "EST5EDT,J366,J1",
        "EST5EDT,366,1",
        "EST5EDT,M3.2.0/168,M11.1.0",
        "EST5EDT,M3.2.0/2:60,M11.1.0",
        "EST5EDT,M3.2.0,M11.1.0x",
        "EST5EDT,M3.2.0,M11.1.0\n",
        "EST25",
        "EST5:",
        "EST99999999999999999999",
        "EST5EDT,M3.2.0/2:00:60,M11.1.0",
    ];
    for text in cases {
        assert!(parse(text).is_err(), "{text:?} read as {:?}", parse(text));
    }
}

#[test]
fn new_york_changes_on_the_second_sunday_of_march_and_the_first_of_november() {
    let rule = parse("EST5EDT,M3.2.0,M11.1.0").unwrap();
    // glibc 2.39 for America/New_York: 2026-03-08 07:00 UTC and 2026-11-01 06:00 UTC.
    let changes = rule.changes(midnight(2026, 1, 1), midnight(2027, 1, 1));
    assert_eq!(changes, [(1_772_953_200, -4 * HOUR), (1_793_512_800, -5 * HOUR)], "2026");
    assert_eq!(rule.offset_at(1_772_953_199), -5 * HOUR, "a second before the change");
    assert_eq!(rule.offset_at(1_772_953_200), -4 * HOUR, "at the change");
}

#[test]
fn a_southern_rule_starts_in_spring_and_ends_in_autumn() {
    // America/Santiago's rule, whose changes come at 24:00 on a Saturday (glibc 2.39).
    let rule = parse("<-04>4<-03>,M9.1.6/24,M4.1.6/24").unwrap();
    assert_eq!(rule.offset_at(midnight(2026, 1, 1)), -3 * HOUR, "summer at New Year");
    let changes = rule.changes(midnight(2026, 1, 1), midnight(2027, 1, 1));
    assert_eq!(changes, [(1_775_358_000, -4 * HOUR), (1_788_667_200, -3 * HOUR)], "2026");
}

#[test]
fn negative_and_extended_times_move_a_change_to_another_day() {
    // America/Nuuk changes at -1:00 on the last Sunday of March, 22:00 standard time on the
    // Saturday; Asia/Jerusalem at 26:00 on the fourth Thursday, 02:00 on the Friday (glibc 2.39).
    let nuuk = parse("<-02>2<-01>,M3.5.0/-1,M10.5.0/0").unwrap();
    let changes = nuuk.changes(midnight(2040, 1, 1), midnight(2041, 1, 1));
    assert_eq!(changes, [(2_216_250_000, -HOUR), (2_234_998_800, -2 * HOUR)], "Nuuk");
    let jerusalem = parse("IST-2IDT,M3.4.4/26,M10.5.0").unwrap();
    let changes = jerusalem.changes(midnight(2040, 1, 1), midnight(2041, 1, 1));
    assert_eq!(changes, [(2_216_073_600, 3 * HOUR), (2_234_991_600, 2 * HOUR)], "Jerusalem");
}

#[test]
fn jn_never_counts_february_29_and_n_does() {
    let julian = parse("AAA0BBB,J60/0,J305/0").unwrap();
    let zero = parse("AAA0BBB,59/0,304/0").unwrap();
    for (year, february_29) in [(2027, false), (2028, true)] {
        let from = midnight(year, 1, 1);
        let until = midnight(year + 1, 1, 1);
        let (Some(&(julian_start, _)), Some(&(zero_start, _))) =
            (julian.changes(from, until).first(), zero.changes(from, until).first())
        else {
            panic!("no changes in {year}");
        };
        assert_eq!(julian_start, midnight(year, 3, 1), "J60 is March 1 in {year}");
        let expected = if february_29 { midnight(year, 2, 29) } else { midnight(year, 3, 1) };
        assert_eq!(zero_start, expected, "59 counts February 29 in {year}");
    }
}

#[test]
fn week_five_is_the_last_such_weekday_of_the_month() {
    let rule = parse("AAA0BBB,M3.5.0/0,M10.5.0/0").unwrap();
    // March 2026 has five Sundays, the last on the 29th; October 2026 four, the last on the 25th,
    // whose midnight in daylight saving time is 23:00 UTC the day before.
    let changes = rule.changes(midnight(2026, 1, 1), midnight(2027, 1, 1));
    let end = midnight(2026, 10, 25) - 3_600;
    assert_eq!(changes, [(midnight(2026, 3, 29), HOUR), (end, 0)], "2026");
}

#[test]
fn changes_that_coincide_leave_standard_time_all_year() {
    // The start at 02:00 standard time and the end at 03:00 daylight time are one instant:
    // glibc 2.39 keeps standard time all year.
    let rule = parse("AAA3BBB2,J100/2,J100/3").unwrap();
    assert!(rule.changes(midnight(2026, 1, 1), midnight(2028, 1, 1)).is_empty(), "no change");
    assert_eq!(rule.offset_at(midnight(2026, 4, 10) + 6 * 3_600), -3 * HOUR, "standard time");
}

#[test]
fn daylight_saving_time_all_year_has_only_its_own_offset() {
    // RFC 9636 section 3.3.1's example: daylight saving time from 00:00 on January 1 to 24:00
    // less the hour on December 31, every year.
    let rule = parse("XXX3EDT4,0/0,J365/23").unwrap();
    let changes = rule.changes(midnight(2026, 1, 1), midnight(2028, 1, 1));
    assert!(changes.iter().all(|(_, offset)| *offset == -4 * HOUR), "{changes:?}");
    for at in [midnight(2026, 1, 1), midnight(2026, 1, 1) + 3 * 3_600, midnight(2026, 7, 1)] {
        assert_eq!(rule.offset_at(at), -4 * HOUR, "at {at}");
    }
}

#[test]
fn a_year_and_its_first_second_agree() {
    for year in [-1, 0, 1, 1582, 1900, 1970, 2000, 2026, 2100, 9999, 65_537] {
        let start = start_of(year);
        assert_eq!(year_of(start), year, "the first second of {year}");
        assert_eq!(year_of(start - 1), year - 1, "the second before {year}");
    }
}

/// A rule's day in the `TZ` string's notation.
fn day_text(day: Day) -> String {
    match day {
        Day::NoLeap(n) => format!("J{n}"),
        Day::FromZero(n) => n.to_string(),
        Day::Weekday { month, week, weekday } => format!("M{month}.{week}.{weekday}"),
    }
}

/// Seconds as `[-]h[:mm[:ss]]`.
fn hms(seconds: i32) -> String {
    let sign = if seconds < 0 { "-" } else { "" };
    let seconds = seconds.unsigned_abs();
    match (seconds / 60 % 60, seconds % 60) {
        (0, 0) => format!("{sign}{}", seconds / 3_600),
        (m, 0) => format!("{sign}{}:{m:02}", seconds / 3_600),
        (m, s) => format!("{sign}{}:{m:02}:{s:02}", seconds / 3_600),
    }
}

/// `rule` as a footer writes it.
fn written(rule: &Rule) -> String {
    let std = format!("<+AA>{}", hms(-rule.std));
    let Some(dst) = rule.dst else { return std };
    format!(
        "{std}BBB{},{}/{},{}/{}",
        hms(-dst.offset),
        day_text(dst.start.day),
        hms(dst.start.time),
        day_text(dst.end.day),
        hms(dst.end.time),
    )
}

fn day() -> impl Strategy<Value = Day> {
    prop_oneof![
        (1_u16..=365).prop_map(Day::NoLeap),
        (0_u16..=365).prop_map(Day::FromZero),
        (1_u8..=12, 1_u8..=5, 0_u8..=6).prop_map(|(month, week, weekday)| Day::Weekday {
            month,
            week,
            weekday
        }),
    ]
}

fn change() -> impl Strategy<Value = Change> {
    (day(), -(167 * HOUR + 3_599)..=167 * HOUR + 3_599).prop_map(|(day, time)| Change { day, time })
}

/// Offsets as POSIX writes them, up to 24:59:59 either side.
fn offset() -> impl Strategy<Value = i32> {
    -(24 * HOUR + 3_599)..=24 * HOUR + 3_599
}

fn rule() -> impl Strategy<Value = Rule> {
    let dst =
        (offset(), change(), change()).prop_map(|(offset, start, end)| Dst { offset, start, end });
    (offset(), prop::option::of(dst)).prop_map(|(std, dst)| Rule { std, dst })
}

proptest! {
    #[test]
    fn a_written_rule_reads_back(rule in rule()) {
        prop_assert_eq!(parse(&written(&rule)), Ok(rule), "{}", written(&rule));
    }

    #[test]
    fn each_change_falls_on_the_day_its_rule_names(change in change(), year in 1_u16..=9_997, offset in offset()) {
        let at = change.instant(i64::from(year), offset).unwrap();
        let local_midnight = at + i64::from(offset) - i64::from(change.time);
        prop_assert_eq!(local_midnight.rem_euclid(86_400), 0, "a midnight");
        let date = Date::from_days_since_epoch(local_midnight.div_euclid(86_400));
        let start = Date::new(year, 1, 1).unwrap();
        let day_of_year = start.days_until(date);
        match change.day {
            Day::NoLeap(n) => {
                let leap = Date::new(year, 2, 29).is_ok();
                let counted = day_of_year + 1 - i64::from(leap && day_of_year >= 59);
                prop_assert_eq!(counted, i64::from(n), "J{} is {}", n, date);
            }
            Day::FromZero(n) => prop_assert_eq!(day_of_year, i64::from(n), "{} is {}", n, date),
            Day::Weekday { month, week, weekday } => {
                prop_assert_eq!(date.m, month, "the month of {}", date);
                // 1970-01-01, day 0, was a Thursday.
                let days = date.days_since_epoch();
                prop_assert_eq!((days + 4).rem_euclid(7), i64::from(weekday), "the weekday of {}", date);
                let later = Date::from_days_since_epoch(days + 7);
                if week == 5 {
                    prop_assert!(later.m != month, "{} is the last in its month", date);
                } else {
                    prop_assert_eq!((date.d - 1) / 7 + 1, week, "the week of {}", date);
                }
            }
        }
    }

    #[test]
    fn the_offset_at_an_instant_is_the_last_change_before_it(
        rule in rule(),
        year in 1_900_i64..=2_200,
        into in 0_i64..2 * 365 * 86_400,
    ) {
        let from = start_of(year);
        let until = start_of(year + 2);
        let at = from + into;
        let changes = rule.changes(from, until);
        prop_assert!(changes.windows(2).all(|pair| pair[0].0 <= pair[1].0), "in order");
        let expected = changes
            .iter()
            .take_while(|(when, _)| *when <= at)
            .last()
            .map_or_else(|| rule.offset_at(from - 1), |(_, offset)| *offset);
        prop_assert_eq!(rule.offset_at(at), expected, "at {}", at);
    }
}
