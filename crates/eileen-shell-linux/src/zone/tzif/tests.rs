use proptest::prelude::*;

use super::{Zone, write, write_block};
use crate::zone::rule::{Rule, start_of};

/// `America/New_York` from Ubuntu 24.04's tzdata 2026c, version 2, fat: transitions to 2037.
const NEW_YORK: &[u8] = include_bytes!("../../../tests/fixtures/zoneinfo/America/New_York");
/// `America/Nuuk` from the same tzdata, version 3 for its footer's negative times.
const NUUK: &[u8] = include_bytes!("../../../tests/fixtures/zoneinfo/America/Nuuk");
/// `Etc/UTC`: one type, no transitions, `UTC0`.
const UTC: &[u8] = include_bytes!("../../../tests/fixtures/zoneinfo/Etc/UTC");

/// A version 1 file: one header and its 32-bit block.
fn version_1(transitions: &[(i64, u8)], types: &[(i32, bool)]) -> Vec<u8> {
    let mut file = Vec::new();
    write_block(&mut file, 0, transitions, types, 4);
    file
}

#[test]
fn the_files_of_each_version_read() {
    let utc = Zone::parse(UTC).unwrap();
    assert_eq!((utc.transitions.len(), utc.first), (0, 0), "UTC");
    assert_eq!(utc.footer, Some(Rule::parse(b"UTC0").unwrap()), "UTC's footer");

    let new_york = Zone::parse(NEW_YORK).unwrap();
    assert_eq!(new_york.first, -17_762, "local mean time, -4:56:02");
    assert_eq!(new_york.footer, Some(Rule::parse(b"EST5EDT,M3.2.0,M11.1.0").unwrap()), "footer");
    let last = new_york.transitions.last().unwrap();
    assert_eq!(*last, (2_140_668_000, -18_000), "fat: the last change is in November 2037");

    let nuuk = Zone::parse(NUUK).unwrap();
    assert_eq!(
        nuuk.footer,
        Some(Rule::parse(b"<-02>2<-01>,M3.5.0/-1,M10.5.0/0").unwrap()),
        "version 3's footer"
    );

    let old =
        Zone::parse(&version_1(&[(-100, 1), (100, 0)], &[(3_600, false), (7_200, true)])).unwrap();
    assert_eq!(old.transitions, [(-100, 7_200), (100, 3_600)], "version 1's transitions");
    assert_eq!((old.first, old.footer), (3_600, None), "version 1 has no footer");
}

#[test]
fn a_later_file_reads_from_its_second_block_whatever_its_first_says() {
    let types = [(3_600, false), (7_200, true)];
    let transitions = [(-5_000_000_000, 1), (100, 0), (5_000_000_000, 1)];
    let full = write(b'2', &transitions, &types, b"");
    // RFC 9636 section 4's placeholder first block: no transitions, one type, one designation.
    let mut placeholder = Vec::new();
    write_block(&mut placeholder, b'2', &[], &[(0, false)], 4);
    let mut second = Vec::new();
    write_block(&mut second, b'2', &transitions, &types, 8);
    placeholder.extend_from_slice(&second);
    placeholder.extend_from_slice(b"\n\n");
    let zone = Zone::parse(&full).unwrap();
    assert_eq!(Zone::parse(&placeholder).unwrap(), zone, "a placeholder first block");
    assert_eq!(
        zone.transitions,
        [(-5_000_000_000, 7_200), (100, 3_600), (5_000_000_000, 7_200)],
        "64-bit times"
    );
    for version in *b"345" {
        let later = write(version, &transitions, &types, b"");
        assert_eq!(Zone::parse(&later).unwrap(), zone, "version {}", char::from(version));
    }
}

/// A version 2 file with leap-second records, the same in both blocks, written by hand: `write`
/// writes none. `transitions` as (leap time, type), `types` as seconds east of UTC, `leaps` as
/// (occurrence, correction).
fn with_leaps(transitions: &[(i64, u8)], types: &[i32], leaps: &[(i64, i32)]) -> Vec<u8> {
    let mut file = Vec::new();
    for time_size in [4_usize, 8] {
        file.extend_from_slice(b"TZif2");
        file.extend_from_slice(&[0; 15]);
        for count in [0, 0, leaps.len(), transitions.len(), types.len(), 4] {
            file.extend_from_slice(&u32::try_from(count).unwrap().to_be_bytes());
        }
        for (time, _) in transitions {
            file.extend_from_slice(time.to_be_bytes().get(8 - time_size..).unwrap());
        }
        file.extend(transitions.iter().map(|(_, index)| *index));
        for offset in types {
            file.extend_from_slice(&offset.to_be_bytes());
            file.extend_from_slice(&[0, 0]);
        }
        file.extend_from_slice(b"ZZZ\0");
        for (at, correction) in leaps {
            file.extend_from_slice(at.to_be_bytes().get(8 - time_size..).unwrap());
            file.extend_from_slice(&correction.to_be_bytes());
        }
    }
    file.extend_from_slice(b"\n\n");
    file
}

#[test]
fn leap_seconds_move_transitions_back_to_unix_time() {
    // Two leap records (1972-07-01 and 1973-01-01, in leap time) and transitions before, at and
    // after them.
    let leaps = [(78_796_800, 1), (94_694_401, 2)];
    let transitions = [(1_000, 0), (78_796_801, 0), (94_694_500, 0)];
    let zone = Zone::parse(&with_leaps(&transitions, &[3_600], &leaps)).unwrap();
    let unix: Vec<i64> = zone.transitions.iter().map(|(at, _)| *at).collect();
    assert_eq!(unix, [1_000, 78_796_800, 94_694_498], "each less the correction in force");
    // A leap second between two transitions a second apart brings them to one instant.
    let together = [(78_796_799, 0), (78_796_800, 0)];
    let zone = Zone::parse(&with_leaps(&together, &[3_600], &leaps)).unwrap();
    let unix: Vec<i64> = zone.transitions.iter().map(|(at, _)| *at).collect();
    assert_eq!(unix, [78_796_799, 78_796_799], "one instant");
}

/// A valid file to break: two types, three transitions and a footer.
fn sound() -> Vec<u8> {
    write(b'3', &[(-100, 1), (100, 0), (200, 1)], &[(3_600, false), (7_200, true)], b"AAA-1")
}

/// The offset of the second header in [`sound`]'s bytes.
fn second_header() -> usize {
    44 + 3 * 5 + 2 * 6 + 4
}

#[test]
fn a_malformed_file_is_refused() {
    let second = second_header();
    let block = second + 44;
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut set = |name, at: usize, bytes: &[u8]| {
        let mut file = sound();
        file.splice(at..at + bytes.len(), bytes.iter().copied());
        cases.push((name, file));
    };
    set("no magic", 0, b"TZiF");
    set("version 1 written as a digit", 4, b"1");
    set("an unknown version", 4, b"A");
    set("the headers disagree", second + 4, b"2");
    set("a huge count", second + 32, &[0xff; 4]);
    set("no types", second + 36, &[0, 0, 0, 0]);
    set("no designations", second + 40, &[0, 0, 0, 0]);
    set("indicators that do not match the types", second + 20, &[0, 0, 0, 1]);
    set("equal transitions", block + 8, &(-100_i64).to_be_bytes());
    set("transitions out of order", block + 16, &(50_i64).to_be_bytes());
    set("a missing type", block + 24, &[2]);
    set("an offset over a day", block + 27, &93_600_i32.to_be_bytes());
    set("an offset of -2^31", block + 27, &i32::MIN.to_be_bytes());
    set("a daylight flag of 2", block + 31, &[2]);
    set("a designation past the end", block + 32, &[4]);
    set("designations without a NUL", block + 42, b"Z");
    set("no footer", block + 43, b"X");
    set("a footer that is not a rule", block + 44, b"1");
    let mut unclosed = sound();
    unclosed.pop();
    cases.push(("an unclosed footer", unclosed));
    cases.push(("empty", Vec::new()));
    cases.push(("a header alone", sound()[..44].to_vec()));
    let (transitions, types) = ([(50, 1), (150, 0)], [3_600, 7_200]);
    // A first correction may be any (a version 4 file can start its table late), but one that
    // takes 1000 s off the second transition moves it before the first.
    cases.push(("a correction that reorders", with_leaps(&transitions, &types, &[(100, 1_000)])));
    let apart = [(100, 1), (200, 3)];
    cases.push(("corrections two apart", with_leaps(&transitions, &types, &apart)));
    for (name, file) in cases {
        assert!(Zone::parse(&file).is_err(), "{name}: {:?}", Zone::parse(&file));
    }
    assert!(Zone::parse(&sound()).is_ok(), "the file the cases break reads");
    let late = with_leaps(&transitions, &types, &[(10, 27), (100, 28), (200, 27)]);
    assert!(Zone::parse(&late).is_ok(), "a table that starts late, with a negative leap second");
}

#[test]
fn every_truncation_of_a_file_is_refused() {
    for file in [NEW_YORK, NUUK, UTC] {
        for length in 0..file.len() {
            let cut = &file[..length];
            assert!(Zone::parse(cut).is_err(), "{length} of {} bytes read", file.len());
        }
    }
}

#[test]
fn bytes_after_the_footer_are_not_read() {
    let mut longer = NEW_YORK.to_vec();
    longer.extend_from_slice(b"anything\n\0\xff");
    assert_eq!(Zone::parse(&longer), Zone::parse(NEW_YORK), "the same zone");
}

/// The offset in force at `at` by a table read the slow way: type 0's before the first
/// transition, then each transition's.
fn reference(transitions: &[(i64, i32)], first: i32, at: i64) -> i32 {
    transitions
        .iter()
        .take_while(|(when, _)| *when <= at)
        .last()
        .map_or(first, |(_, offset)| *offset)
}

/// A table: transitions as (Unix time, type) and types as (offset, daylight saving).
type Table = (Vec<(i64, u8)>, Vec<(i32, bool)>);

fn table() -> impl Strategy<Value = Table> {
    let types = prop::collection::vec((-89_999_i32..=93_599, any::<bool>()), 1..8);
    let times = prop::collection::btree_set(-(1_i64 << 40)..(1_i64 << 40), 0..40);
    (types, times, any::<u64>()).prop_map(|(types, times, seed)| {
        let count = u64::try_from(types.len()).unwrap();
        let transitions = times
            .into_iter()
            .enumerate()
            .map(|(i, at)| (at, u8::try_from((seed >> (i % 60)) % count).unwrap()))
            .collect();
        (transitions, types)
    })
}

/// Footers in each form a rule takes: none, a fixed offset, `Mm.w.d`, negative times and times
/// past 24 hours, `Jn` and `n` with the extreme times, and daylight saving time all year.
const FOOTERS: [&str; 7] = [
    "",
    "UTC0",
    "EST5EDT,M3.2.0,M11.1.0",
    "<-03>3<-02>,M3.5.0/-2,M10.5.0/-1",
    "IST-2IDT,M3.4.4/26,M10.5.0",
    "AAA-1BBB,J60/-167:59:59,300/+167",
    "XXX3EDT4,0/0,J365/23",
];

/// The offset in force at `at` by an initial offset and its changes, in order.
fn folded(initial: i32, changes: &[(i64, i32)], at: i64) -> i32 {
    changes.iter().take_while(|(when, _)| *when <= at).last().map_or(initial, |(_, offset)| *offset)
}

proptest! {
    #[test]
    fn a_written_table_and_rule_read_back(
        (transitions, types) in table(),
        footer in prop::sample::select(FOOTERS.to_vec()),
        year in 1_i64..=9_999,
        into in 0_i64..2 * 365 * 86_400,
    ) {
        let file = write(b'3', &transitions, &types, footer.as_bytes());
        let zone = Zone::parse(&file).unwrap();
        let rule = (!footer.is_empty()).then(|| Rule::parse(footer.as_bytes()).unwrap());
        prop_assert_eq!(zone.footer, rule, "the footer");
        // The table holds before its last transition, and the rule, when there is one, from it on,
        // at that transition too: a generated rule need not agree with the table there.
        let mut expected: Vec<(i64, i32)> =
            transitions.iter().map(|(at, index)| (*at, types[usize::from(*index)].0)).collect();
        if let (Some(rule), Some(last)) = (rule, expected.last_mut()) {
            last.1 = rule.offset_at(last.0);
        }
        prop_assert_eq!(&zone.transitions, &expected, "the transitions");
        prop_assert_eq!(zone.first, types[0].0, "type 0 before them");
        let last = expected.last().map(|(at, _)| *at);
        let truth = |at: i64| match rule {
            Some(rule) if last.is_none_or(|last| at >= last) => rule.offset_at(at),
            _ => reference(&expected, zone.first, at),
        };
        let (from, until) = (start_of(year), start_of(year + 2));
        let (initial, changes) = zone.offsets(from, until);
        prop_assert_eq!(initial, truth(from - 1), "just before {}", year);
        prop_assert!(changes.iter().all(|(at, _)| from <= *at && *at < until), "within the years");
        prop_assert!(changes.windows(2).all(|pair| pair[0].0 <= pair[1].0), "in order");
        let mut instants = vec![from, from + into, until - 1];
        instants.extend(changes.iter().flat_map(|(at, _)| [*at - 1, *at]));
        for at in instants.into_iter().filter(|at| from <= *at && *at < until) {
            prop_assert_eq!(folded(initial, &changes, at), truth(at), "at {}", at);
        }
    }

    #[test]
    fn any_bytes_read_without_a_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ignored = Zone::parse(&bytes);
        let mut file = sound();
        let at = usize::from(bytes.first().copied().unwrap_or_default()) % file.len();
        file.splice(at..file.len().min(at + bytes.len()), bytes.iter().copied());
        let _ignored = Zone::parse(&file);
    }
}
