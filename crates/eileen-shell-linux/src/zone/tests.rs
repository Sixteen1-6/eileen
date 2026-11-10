use eileen_contracts::ids::WallTime;

use super::{ZoneError, ZoneOffsets, in_minutes, minutes, offsets_of, window, write_tzif};

/// The fixture `zone` of `tests/fixtures/zoneinfo/`: Ubuntu 24.04's tzdata 2026c, whose files
/// are fat, or under `slim/` the same zone as `zic -b slim` writes it from that tzdata's source.
macro_rules! zone {
    ($zone:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/zoneinfo/", $zone))
            .as_slice()
    };
}

/// A zone's name, its file, the year read, and the offsets expected: the initial one and the
/// changes as (Unix time, minutes).
type Case = (&'static str, &'static [u8], u16, i32, &'static [(i64, i32)]);

/// What glibc 2.39 gives for each fixture: `localtime`'s `tm_gmtoff` just before January 1 of
/// the year, 00:00 UTC, and at each change through the end of the next year, found second by
/// second, in minutes rounded as [`minutes`] rounds.
const GLIBC: &[Case] = &[
    ("UTC", zone!("Etc/UTC"), 2026, 0, &[]),
    (
        "New York",
        zone!("America/New_York"),
        2026,
        -300,
        &[
            (1_772_953_200, -240),
            (1_793_512_800, -300),
            (1_805_007_600, -240),
            (1_825_567_200, -300),
        ],
    ),
    (
        "New York, the fat file's end",
        zone!("America/New_York"),
        2037,
        -300,
        &[
            (2_120_108_400, -240),
            (2_140_668_000, -300),
            (2_152_162_800, -240),
            (2_172_722_400, -300),
        ],
    ),
    (
        "New York, past the fat file's end",
        zone!("America/New_York"),
        2040,
        -300,
        &[
            (2_215_062_000, -240),
            (2_235_621_600, -300),
            (2_246_511_600, -240),
            (2_267_071_200, -300),
        ],
    ),
    ("New York, local mean time", zone!("America/New_York"), 1882, -296, &[(-2_717_650_800, -300)]),
    ("New York, slim", zone!("slim/America/New_York"), 1882, -296, &[(-2_717_650_800, -300)]),
    (
        "London",
        zone!("Europe/London"),
        2026,
        0,
        &[(1_774_746_000, 60), (1_792_890_000, 0), (1_806_195_600, 60), (1_824_944_400, 0)],
    ),
    ("London, British Standard Time", zone!("Europe/London"), 1970, 60, &[(57_722_400, 0)]),
    (
        "Lord Howe, a half-hour change",
        zone!("Australia/Lord_Howe"),
        2026,
        660,
        &[(1_775_314_800, 630), (1_791_041_400, 660), (1_806_764_400, 630), (1_822_491_000, 660)],
    ),
    ("Kolkata", zone!("Asia/Kolkata"), 2026, 330, &[]),
    ("Kolkata, Madras time", zone!("Asia/Kolkata"), 1900, 321, &[]),
    (
        "Kolkata, the war",
        zone!("Asia/Kolkata"),
        1941,
        330,
        &[(-891_581_400, 390), (-872_058_600, 330), (-862_637_400, 390)],
    ),
    (
        "Chatham, 45 minutes",
        zone!("Pacific/Chatham"),
        2026,
        825,
        &[(1_775_311_200, 765), (1_790_431_200, 825), (1_806_760_800, 765), (1_821_880_800, 825)],
    ),
    (
        "Santiago, south of the equator",
        zone!("America/Santiago"),
        2026,
        -180,
        &[
            (1_775_358_000, -240),
            (1_788_667_200, -180),
            (1_806_807_600, -240),
            (1_820_116_800, -180),
        ],
    ),
    (
        "Santiago, from the footer",
        zone!("America/Santiago"),
        2040,
        -180,
        &[
            (2_217_466_800, -240),
            (2_230_171_200, -180),
            (2_248_916_400, -240),
            (2_262_225_600, -180),
        ],
    ),
    (
        "Nuuk, negative times",
        zone!("slim/America/Nuuk"),
        2026,
        -120,
        &[(1_774_746_000, -60), (1_792_890_000, -120), (1_806_195_600, -60), (1_824_944_400, -120)],
    ),
];

/// The fixtures with a fat and a slim file.
const PAIRS: &[(&[u8], &[u8])] = &[
    (zone!("America/New_York"), zone!("slim/America/New_York")),
    (zone!("America/Nuuk"), zone!("slim/America/Nuuk")),
    (zone!("America/Santiago"), zone!("slim/America/Santiago")),
    (zone!("Australia/Lord_Howe"), zone!("slim/Australia/Lord_Howe")),
    (zone!("Pacific/Chatham"), zone!("slim/Pacific/Chatham")),
];

fn offsets(initial_min: i32, changes: &[(i64, i32)]) -> ZoneOffsets {
    let changes = changes.iter().map(|(at, offset)| (WallTime(at * 1_000_000), *offset)).collect();
    ZoneOffsets { initial_min, changes }
}

#[test]
fn each_zone_gives_what_glibc_gives() {
    for (name, file, year, initial, changes) in GLIBC {
        assert_eq!(offsets_of(file, *year), Ok(offsets(*initial, changes)), "{name} from {year}");
    }
}

#[test]
fn a_slim_file_and_a_fat_one_agree_every_year() {
    for (fat, slim) in PAIRS {
        for year in 1_850..=2_100 {
            assert_eq!(offsets_of(fat, year), offsets_of(slim, year), "from {year}");
        }
    }
}

#[test]
fn consecutive_years_agree_where_their_two_years_overlap() {
    let micros = |at: i64| WallTime(at * 1_000_000);
    let pairs = PAIRS.iter().flat_map(|(fat, slim)| [*fat, *slim]);
    for file in pairs.chain(GLIBC.iter().map(|case| case.1)) {
        for year in 1_850..=2_100 {
            let (this, next) =
                (offsets_of(file, year).unwrap(), offsets_of(file, year + 1).unwrap());
            let (start, until) = (micros(window(year + 1).0), micros(window(year).1));
            let at_start = this
                .changes
                .iter()
                .take_while(|(at, _)| *at < start)
                .last()
                .map_or(this.initial_min, |c| c.1);
            assert_eq!(at_start, next.initial_min, "the offset as {} starts", year + 1);
            let this_within: Vec<_> = this.changes.iter().filter(|(at, _)| *at >= start).collect();
            let next_within: Vec<_> = next.changes.iter().filter(|(at, _)| *at < until).collect();
            assert_eq!(this_within, next_within, "the changes in {}", year + 1);
        }
    }
}

#[test]
fn offsets_round_to_the_nearest_minute_a_half_away_from_zero() {
    let cases = [
        (0, 0),
        (29, 0),
        (30, 1),
        (-29, 0),
        (-30, -1),
        (19_800, 330),
        (19_270, 321),
        (-17_762, -296),
        (-2_670, -45),
        (2_670, 45),
        (93_599, 1_560),
        (-89_999, -1_500),
    ];
    for (seconds, expected) in cases {
        assert_eq!(minutes(seconds), expected, "{seconds} s");
    }
}

#[test]
fn a_change_rounding_makes_no_change_and_a_repeated_instant_are_left_out() {
    let kept = in_minutes(-17_762, vec![(10, -17_760), (20, 3_600), (20, 7_200), (40, 7_170)]);
    assert_eq!(kept, offsets(-296, &[(20, 120)]), "-4:56:02 and -4:56:00 are one minute's offset");
    let kept = in_minutes(0, vec![(10, 60), (10, 0), (20, 3_600)]);
    assert_eq!(kept, offsets(0, &[(20, 60)]), "a change undone at its own instant");
}

#[test]
fn daylight_saving_time_all_year_is_one_offset() {
    let file = write_tzif(b'3', &[], &[(-3 * 3_600, false)], b"XXX3EDT4,0/0,J365/23");
    assert_eq!(offsets_of(&file, 2026), Ok(offsets(-240, &[])), "always EDT");
}

#[test]
fn a_rule_alone_gives_every_year() {
    let file = write_tzif(b'2', &[], &[(-5 * 3_600, false)], b"EST5EDT,M3.2.0,M11.1.0");
    let new_york = offsets_of(zone!("America/New_York"), 2026);
    assert_eq!(offsets_of(&file, 2026), new_york, "a footer with no transitions");
}

#[test]
fn a_file_without_a_rule_keeps_its_last_offset() {
    let file = write_tzif(b'2', &[(0, 1), (100, 0)], &[(3_600, false), (7_200, true)], b"");
    assert_eq!(offsets_of(&file, 2026), Ok(offsets(60, &[])), "the last type's offset");
    assert_eq!(offsets_of(&file, 1969), Ok(offsets(60, &[(0, 120), (100, 60)])), "1970's changes");
}

#[test]
fn a_file_that_is_not_tzif_is_malformed() {
    let error = offsets_of(b"EST5EDT\n", 2026).unwrap_err();
    assert_eq!(error, ZoneError::Malformed { reason: "the file ends inside a header" }, "text");
    assert_eq!(
        error.to_string(),
        "/etc/localtime is not a time zone file: the file ends inside a header",
        "reads well"
    );
}
