//! Unit tests of the machine-wide files: the root, the line formats, percentiles, the `auto`
//! paging limit, the slot probe, the baseline's keys, and the hourly summaries with the usual
//! memory they give.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{self, ErrorKind};
use std::path::PathBuf;

use super::{
    Baseline, HOURS_FILE, HOURS_TRIM_BYTES, HourSummary, LimitSource, PagingWindow, Sample, Source,
    Store, USUAL_MIN_HOURS, USUAL_MIN_SAMPLES, Usual, auto_paging_limit, machine_root_from,
    p75_plus_25, percentile, plus_25, usual_of,
};
use crate::clock::ManualClock;
use crate::slots::{Holder, Slot, try_take_slot};

/// The machine root a process with exactly these variables would use.
fn root_with(vars: &[(&'static str, &'static str)]) -> io::Result<PathBuf> {
    let vars = vars.to_vec();
    machine_root_from(move |key| {
        vars.iter().find(|(name, _)| *name == key).map(|(_, value)| OsString::from(*value))
    })
}

#[test]
fn the_machine_root_follows_each_platforms_rule() {
    assert_eq!(
        root_with(&[("LOCALAPPDATA", "local"), ("XDG_STATE_HOME", "state"), ("HOME", "home")])
            .unwrap(),
        PathBuf::from("local").join("eileen"),
        "LOCALAPPDATA wins on every OS"
    );
    let state = root_with(&[("LOCALAPPDATA", ""), ("XDG_STATE_HOME", "state"), ("HOME", "h")]);
    let home = root_with(&[("XDG_DATA_HOME", "data"), ("HOME", "home")]);
    let nothing = root_with(&[("XDG_DATA_HOME", "data")]);
    if cfg!(windows) {
        for (what, found) in [("XDG_STATE_HOME", &state), ("HOME", &home), ("nothing", &nothing)] {
            assert_eq!(
                found.as_ref().map_err(io::Error::kind),
                Err(ErrorKind::NotFound),
                "without LOCALAPPDATA there is no root on Windows, whatever {what} says"
            );
        }
    } else {
        assert_eq!(
            state.unwrap(),
            PathBuf::from("state").join("eileen"),
            "an empty LOCALAPPDATA counts as unset, and XDG_STATE_HOME comes next"
        );
        assert_eq!(
            home.unwrap(),
            PathBuf::from("home").join(".local").join("state").join("eileen"),
            "then the state folder under HOME, never the data folder"
        );
        assert_eq!(
            nothing.as_ref().map_err(io::Error::kind),
            Err(ErrorKind::NotFound),
            "nothing set"
        );
    }
}

#[test]
fn lines_round_trip_in_the_documented_format() {
    let sample = Sample {
        at_ms: 1_761_000_000_000,
        avail_mb: 1_300,
        headroom_mb: 5_500,
        paging_kb_s: 400,
        slot_held: true,
        src: Source::Runtime,
    };
    let line = sample.to_line();
    assert_eq!(
        line,
        r#"{"at_ms": 1761000000000, "avail_mb": 1300, "headroom_mb": 5500, "paging_kb_s": 400, "slot_held": true, "src": "runtime"}"#,
        "sample line",
    );
    assert_eq!(Sample::from_line(&line), Some(sample), "round trip");
    let window =
        PagingWindow { at_ms: 30_000, paging_kb_s: 9, slot_held: false, src: Source::Runtime };
    assert_eq!(PagingWindow::from_line(&window.to_line()), Some(window), "window round trip");
}

#[test]
fn percentiles_use_the_nearest_rank() {
    let values: Vec<u32> = (1..=100).collect();
    assert_eq!(percentile(&values, 75), Some(75), "p75 of 1..=100");
    assert_eq!(percentile(&values, 10), Some(10), "p10");
    assert_eq!(percentile(&[7], 95), Some(7), "one value");
    assert_eq!(percentile(&[], 50), None, "none");
    assert_eq!(plus_25(4_000), 5_000, "plus 25 percent");
    assert_eq!(p75_plus_25(&values), Some(94), "75 plus 25 percent, rounded");
}

#[test]
fn auto_limit_prefers_a_day_of_windows_then_the_baseline_then_the_default() {
    let window =
        |kb| PagingWindow { at_ms: 0, paging_kb_s: kb, slot_held: false, src: Source::Runtime };
    let baseline = Baseline::from_toml(
        "measured_from = \"2026-10-22\"\nmeasured_to = \"2026-10-22\"\nsamples = 1\n\
         slot_held_excluded = 0\nslot0_paging_kb_s = 90000\nrecheck_paging_kb_s = 1\nheadroom_floor_mb = 1024\n",
    )
    .unwrap();
    let short: Vec<_> = (0..100).map(|_| window(8_000)).collect();
    assert_eq!(auto_paging_limit(&short, None).source, LimitSource::Default, "default");
    assert_eq!(auto_paging_limit(&short, None).kb_s, 5_000, "default value");
    let from_baseline = auto_paging_limit(&short, Some(&baseline));
    assert_eq!(
        (from_baseline.kb_s, from_baseline.source),
        (40_000, LimitSource::Baseline),
        "clamped"
    );
    let mut day: Vec<_> = (0..2_880).map(|_| window(8_000)).collect();
    day.push(PagingWindow { slot_held: true, ..window(1_000_000) });
    let learned = auto_paging_limit(&day, Some(&baseline));
    assert_eq!((learned.kb_s, learned.source), (10_000, LimitSource::Windows), "a day of windows");
    day.remove(0);
    assert_eq!(
        auto_paging_limit(&day, None).source,
        LimitSource::Default,
        "2,879 free windows and a slot-held one do not cover a day",
    );
}

#[test]
fn the_slot_probe_sees_a_held_and_a_free_slot() {
    let dir = std::env::temp_dir()
        .join(format!("eileen-buildguard-samples-{}-probe", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::at(&dir);
    assert!(!store.slot_held(), "no lock files: free");
    let holder = Holder {
        pid: std::process::id(),
        worktree: "wt".to_owned(),
        command: "cargo xtask test".to_owned(),
        started_ms: 0,
    };
    let held = try_take_slot(&dir, Slot::One, &holder).unwrap().unwrap();
    assert!(store.slot_held(), "slot 1 held");
    drop(held);
    assert!(!store.slot_held(), "released: free again");
    let _ignored = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_baseline_needs_every_key() {
    assert!(Baseline::from_toml("samples = 1\n").is_none(), "keys missing");
    assert!(Baseline::from_toml("not toml").is_none(), "malformed");
}

/// 2026-10-26T00:00:00Z.
const NOW: u64 = 1_792_972_800_000;
const HOUR_MS: u64 = 3_600_000;

/// A full hour ending `hours_ago` hours before [`NOW`].
fn hour(hours_ago: u64, avail_p50_mb: u32, plan_items_mb: u32) -> HourSummary {
    HourSummary {
        at_ms: NOW - hours_ago * HOUR_MS,
        avail_p10_mb: avail_p50_mb / 2,
        avail_p50_mb,
        avail_p90_mb: avail_p50_mb * 2,
        headroom_p10_mb: 5_000 + avail_p50_mb,
        paging_p75_kb_s: 2_000,
        plan_items_mb,
        slot_held_pct: 0,
        samples: 720,
        src: Source::Runtime,
    }
}

fn root(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("eileen-buildguard-hours-{}-{name}", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn an_hour_line_round_trips_in_the_documented_format() {
    let summary = HourSummary { slot_held_pct: 25, ..hour(0, 1_800, 300) };
    let line = summary.to_line();
    assert_eq!(
        line,
        r#"{"at_ms": 1792972800000, "avail_p10_mb": 900, "avail_p50_mb": 1800, "avail_p90_mb": 3600, "headroom_p10_mb": 6800, "paging_p75_kb_s": 2000, "plan_items_mb": 300, "slot_held_pct": 25, "samples": 720, "src": "runtime"}"#,
        "hour line"
    );
    assert_eq!(HourSummary::from_line(&line), Some(summary), "round trip");
    let too_big = line.replace("\"slot_held_pct\": 25", "\"slot_held_pct\": 256");
    assert_eq!(HourSummary::from_line(&too_big), None, "a share past a u8 is refused");
}

#[test]
fn hour_lines_past_30_days_are_dropped() {
    let dir = root("trim");
    let store = Store::at(&dir);
    let path = dir.join(HOURS_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Hours from 40 days back, enough of them to take the file past 2 MB, then one from
    // 29 days back that is kept.
    let old_from = 40 * 24;
    let mut text = String::new();
    let mut hours_ago = old_from;
    while u64::try_from(text.len()).unwrap() <= HOURS_TRIM_BYTES {
        let line = hour(hours_ago, 1_000, 0).to_line();
        writeln!(text, "{line}").unwrap();
        // Keep every old line older than 30 days: cycle within days 31 to 40.
        hours_ago = if hours_ago == 31 * 24 { old_from } else { hours_ago - 1 };
    }
    let kept = hour(29 * 24, 1_000, 0);
    writeln!(text, "{}", kept.to_line()).unwrap();
    std::fs::write(&path, text).unwrap();
    let clock = ManualClock::new(NOW);
    assert!(store.append_hour(&hour(0, 1_000, 0), &clock).unwrap(), "the new hour is written");
    let left: Vec<u64> = store.hours_at(NOW, 60).unwrap().into_iter().map(|h| h.at_ms).collect();
    assert_eq!(left, [kept.at_ms, NOW], "only the hours of the last 30 days are left");
    let _ignored = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_line_for_the_same_hour_is_not_written() {
    let dir = root("one-hour");
    let store = Store::at(&dir);
    let clock = ManualClock::new(NOW);
    assert!(store.append_hour(&hour(1, 900, 0), &clock).unwrap(), "first");
    let same = HourSummary { at_ms: NOW - HOUR_MS + 1_000, ..hour(1, 950, 0) };
    assert!(!store.append_hour(&same, &clock).unwrap(), "within the hour: refused");
    assert!(store.append_hour(&hour(0, 1_000, 0), &clock).unwrap(), "the next hour");
    assert_eq!(store.hours_at(NOW, 1).unwrap().len(), 2, "two lines");
    let _ignored = std::fs::remove_dir_all(&dir);
}

/// A fixture week: 168 full hours whose medians run 400, 500, ... in a cycle of eight, with
/// the plan's items at 300, 200 or 400.
fn fixture_week() -> Vec<HourSummary> {
    (0..168)
        .map(|i| {
            let step = u32::try_from(i % 8).unwrap();
            let items = [300, 200, 400][usize::try_from(i % 3).unwrap()];
            hour(i, 400 + step * 100, items)
        })
        .collect()
}

#[test]
fn usual_is_the_25th_percentile_of_hourly_medians_with_plan_items_added_back() {
    let usual = usual_of(&fixture_week()).unwrap();
    // 21 hours each at 400, 500, ..., 1100: the 42nd of 168 sorted medians is 500.
    assert_eq!(usual.available_mb, 500, "the 25th percentile of the hourly medians");
    assert_eq!(usual.plan_items_mb, 300, "the median of the plan's items");
    assert_eq!(usual.headroom_p10_mb, 5_500, "the usual 10th percentile of headroom");
    assert_eq!(usual.hours, 168, "every hour qualified");
    assert_eq!(usual.free_mb(), 800, "with the plan's items added back");
}

#[test]
fn usual_leaves_out_hours_under_360_samples() {
    let mut week = fixture_week();
    // Forty short hours with very little free: counted, they would set the 25th percentile.
    week.extend(
        (0..40).map(|i| HourSummary { samples: USUAL_MIN_SAMPLES - 1, ..hour(200 + i, 50, 0) }),
    );
    let usual = usual_of(&week).unwrap();
    assert_eq!(usual.hours, 168, "the short hours are left out");
    assert_eq!(usual.available_mb, 500, "and do not move the figure");
    let mut exactly = fixture_week();
    exactly.extend(
        (0..100).map(|i| HourSummary { samples: USUAL_MIN_SAMPLES, ..hour(200 + i, 50, 0) }),
    );
    assert_eq!(usual_of(&exactly).unwrap().available_mb, 50, "360 samples count");
}

#[test]
fn usual_is_none_with_fewer_than_24_qualifying_hours() {
    let day: Vec<HourSummary> = (0..24).map(|i| hour(i, 1_000, 100)).collect();
    assert_eq!(
        usual_of(&day),
        Some(Usual { hours: 24, available_mb: 1_000, plan_items_mb: 100, headroom_p10_mb: 6_000 }),
        "24 hours are enough"
    );
    let short_of_a_day = &day[1..];
    assert_eq!(short_of_a_day.len(), USUAL_MIN_HOURS - 1, "23 hours");
    assert_eq!(usual_of(short_of_a_day), None, "23 are not");
    let mut padded = short_of_a_day.to_vec();
    padded.push(HourSummary { samples: 10, ..hour(30, 1_000, 100) });
    assert_eq!(usual_of(&padded), None, "and a short hour does not make up the day");
}
