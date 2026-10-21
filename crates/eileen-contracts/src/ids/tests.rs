use proptest::prelude::*;
use ulid::Ulid;

use super::{
    CallId, ConnectorId, Date, InstallId, JobId, MachineHash, ParseError, Personal, ScenarioId,
    SkillId, ToolName, TraceCtx, TurnId, Untrusted, WallTime,
};

#[test]
fn ids_print_and_parse_with_their_prefix() {
    let id = JobId::from_ulid(Ulid::from_string("01K8ZQ3V2X4Y5Z6A7B8C9D0E1F").unwrap());
    assert_eq!(id.to_string(), "job_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F", "display");
    assert_eq!(format!("{id:?}"), "job_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F", "debug equals display");
    assert_eq!("job_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse::<JobId>(), Ok(id), "parse");
    assert_eq!(serde_json::to_string(&id).unwrap(), "\"job_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F\"", "json");
}

#[test]
fn ids_refuse_a_missing_or_wrong_prefix() {
    let bare = "01K8ZQ3V2X4Y5Z6A7B8C9D0E1F";
    assert_eq!(bare.parse::<JobId>(), Err(ParseError::MissingPrefix { expected: "job" }), "bare");
    assert_eq!(
        format!("turn_{bare}").parse::<JobId>(),
        Err(ParseError::MissingPrefix { expected: "job" }),
        "another type's prefix"
    );
    assert_eq!(
        format!("jobs_{bare}").parse::<JobId>(),
        Err(ParseError::MissingPrefix { expected: "job" }),
        "a longer prefix"
    );
    assert_eq!(
        format!("job{bare}").parse::<JobId>(),
        Err(ParseError::MissingPrefix { expected: "job" }),
        "no underscore"
    );
    assert!(serde_json::from_str::<JobId>(&format!("\"{bare}\"")).is_err(), "json without prefix");
}

#[test]
fn ids_refuse_non_canonical_ulids() {
    for text in [
        "job_01k8zq3v2x4y5z6a7b8c9d0e1f", // lower case
        "job_01K8ZQ3V2X4Y5Z6A7B8C9D0E1",  // 25 characters
        "job_81K8ZQ3V2X4Y5Z6A7B8C9D0E1F", // overflows 128 bits
        "job_01K8ZQ3V2X4Y5Z6A7B8C9D0E1U", // U is not in the alphabet
    ] {
        assert_eq!(text.parse::<JobId>(), Err(ParseError::BadUlid), "{text}");
    }
}

proptest! {
    #[test]
    fn id_text_round_trips(hi in any::<u64>(), lo in any::<u64>()) {
        let id = CallId::from_ulid(Ulid::from((hi, lo)));
        prop_assert_eq!(id.to_string().parse::<CallId>(), Ok(id));
        let json = serde_json::to_string(&id).unwrap();
        prop_assert_eq!(serde_json::from_str::<CallId>(&json).unwrap(), id);
    }

    #[test]
    fn date_text_round_trips(days in -719_162_i64..=2_932_896) {
        let date = Date::from_days_since_epoch(days);
        prop_assert!(date.is_valid());
        prop_assert_eq!(date.days_since_epoch(), days);
        prop_assert_eq!(date.to_string().parse::<Date>(), Ok(date));
    }

    #[test]
    fn install_id_text_round_trips(bytes in any::<[u8; 16]>()) {
        let id = InstallId::from_bytes(bytes);
        prop_assert_eq!(id.to_string().parse::<InstallId>(), Ok(id));
    }
}

#[test]
fn validated_strings_follow_their_rules() {
    assert!(SkillId::new("resolve-goto-15").is_ok(), "skill id");
    assert!(SkillId::new("").is_err(), "empty skill id");
    assert!(SkillId::new("a".repeat(65)).is_err(), "65 characters");
    assert!(SkillId::new("Resolve").is_err(), "upper case");
    assert!(ScenarioId::new("resolve-goto-timecode").is_ok(), "scenario id");
    assert!(ScenarioId::new("resolve goto").is_err(), "space");
    assert!(ToolName::new("apps_resolve").is_ok(), "tool name");
    assert!(ToolName::new("ab").is_err(), "too short");
    assert!(ToolName::new("_apps").is_err(), "leading underscore");
    assert!(ToolName::new(format!("a{}", "b".repeat(62))).is_ok(), "63 characters");
    assert!(ToolName::new(format!("a{}", "b".repeat(63))).is_err(), "64 characters");
    assert!(ConnectorId::new("pc.desktop").is_ok(), "connector id");
    assert!(ConnectorId::new("phone").is_ok(), "one segment");
    assert!(ConnectorId::new("pc..apps").is_err(), "empty segment");
    assert!(ConnectorId::new("PC.apps").is_err(), "upper case");
    assert!(MachineHash::new("ab".repeat(32)).is_ok(), "machine hash");
    assert!(MachineHash::new("AB".repeat(32)).is_err(), "upper-case hex");
    assert!(serde_json::from_str::<ToolName>("\"Apps\"").is_err(), "validated on deserialize");
}

#[test]
fn machine_hash_renders_a_digest_as_hex() {
    let mut digest = [0_u8; 32];
    digest[0] = 0xab;
    digest[31] = 0x01;
    let hash = MachineHash::from_digest(digest);
    assert_eq!(hash.as_str().len(), 64, "length");
    assert!(hash.as_str().starts_with("ab00"), "first byte");
    assert!(hash.as_str().ends_with("01"), "last byte");
}

#[test]
fn install_id_never_shows_in_debug() {
    let id = InstallId::from_bytes([7; 16]);
    assert_eq!(format!("{id:?}"), "InstallId([hidden])", "debug");
    assert_eq!(id.to_string(), "07".repeat(16), "display is the stored form");
}

#[test]
fn dates_parse_only_real_days() {
    assert_eq!("2026-10-21".parse(), Ok(Date { y: 2026, m: 10, d: 21 }), "ordinary day");
    assert!("2024-02-29".parse::<Date>().is_ok(), "leap day");
    for text in ["2026-02-29", "1900-02-29", "2026-13-01", "2026-00-10", "0000-01-01", "26-10-21"] {
        assert!(text.parse::<Date>().is_err(), "{text}");
    }
    assert!("2000-02-29".parse::<Date>().is_ok(), "400-year leap day");
}

#[test]
fn days_until_counts_calendar_days() {
    let since = Date { y: 2026, m: 10, d: 21 };
    assert_eq!(since.days_until(Date { y: 2026, m: 11, d: 20 }), 30, "30 days");
    assert_eq!(since.days_until(Date { y: 2027, m: 1, d: 19 }), 90, "90 days");
    assert_eq!(since.days_until(since), 0, "same day");
    assert_eq!(Date { y: 2026, m: 10, d: 22 }.days_until(since), -1, "backwards");
    assert_eq!(Date { y: 1970, m: 1, d: 1 }.days_since_epoch(), 0, "epoch");
}

#[test]
fn wall_time_gives_the_utc_date() {
    assert_eq!(WallTime(0).date(), Date { y: 1970, m: 1, d: 1 }, "epoch");
    assert_eq!(WallTime(-1).date(), Date { y: 1969, m: 12, d: 31 }, "one microsecond before");
    // 2026-10-21T23:59:59.999999Z
    assert_eq!(WallTime(1_792_627_199_999_999).date(), Date { y: 2026, m: 10, d: 21 }, "late");
}

#[test]
fn traceparent_uses_the_turn_then_the_job() {
    let turn = TurnId::from_parts(1, 2);
    let job = JobId::from_parts(3, 4);
    let ctx = TraceCtx { turn: Some(turn), job: Some(job), call: None };
    let expected = format!("00-{:032x}-{:016x}-01", turn.ulid().0, 0xabc_u64);
    assert_eq!(ctx.traceparent(0xabc), Some(expected), "turn first");
    let ctx = TraceCtx { turn: None, job: Some(job), call: None };
    assert_eq!(
        ctx.traceparent(1),
        Some(format!("00-{:032x}-0000000000000001-01", job.ulid().0)),
        "job when there is no turn"
    );
    assert_eq!(TraceCtx::default().traceparent(1), None, "neither");
    assert_eq!(TraceCtx { turn: Some(turn), job: None, call: None }.traceparent(0), None, "span 0");
}

#[test]
fn wrappers_serialize_as_the_bare_value() {
    let text = Untrusted::new("Save changes?".to_owned());
    assert_eq!(serde_json::to_string(&text).unwrap(), "\"Save changes?\"", "untrusted");
    let path = Personal::new("C:\\Users\\someone\\notes.txt".to_owned());
    assert_eq!(
        serde_json::to_string(&path).unwrap(),
        "\"C:\\\\Users\\\\someone\\\\notes.txt\"",
        "personal"
    );
    assert_eq!(format!("{path:?}"), "Personal([hidden])", "personal debug hides the text");
    assert_eq!(format!("{text:?}"), "Untrusted(\"Save changes?\")", "untrusted debug shows it");
}
