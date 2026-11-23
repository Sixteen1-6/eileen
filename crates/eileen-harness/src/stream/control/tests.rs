use std::path::PathBuf;
use std::sync::Arc;

use eileen_contracts::ids::Untrusted;
use eileen_trace::Redactor;

use super::{
    INTERRUPT_CAPABILITY, MAX_OPEN_REQUESTS, MAX_REQUEST_ID_BYTES, Message, Reply, Requests,
    SendError, SettleError, interrupt, parse,
};
use crate::stream::{Init, Line, parse as parse_line};

/// The capabilities CLI 2.1.304's `system/init` listed in WP3's approval contract test.
const RECORDED_CAPABILITIES: [&str; 3] =
    ["interrupt_receipt_v1", "interrupt_cancel_queued_v1", "interrupt_send_now_v1"];

/// A `system/init` that lists `capabilities`.
fn init(capabilities: &[&str]) -> Init {
    Init {
        session_id: "session".to_owned(),
        model: "model".to_owned(),
        tools: Vec::new(),
        mcp_servers: Vec::new(),
        plugins: Vec::new(),
        capabilities: capabilities.iter().map(|name| (*name).to_owned()).collect(),
        permission_mode: None,
        memory_path: None,
        cli_version: None,
    }
}

/// The CLI's `control_response` line to `request_id`, `success` or, with `error`, `error`.
fn response_line(request_id: &str, error: Option<&str>) -> String {
    match error {
        None => format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{request_id}"}}}}"#
        ),
        Some(error) => format!(
            r#"{{"type":"control_response","response":{{"subtype":"error","request_id":"{request_id}","error":"{error}"}}}}"#
        ),
    }
}

/// Parses the CLI's `control_response` to `request_id` and settles it on `requests`.
fn settle(
    requests: &mut Requests,
    request_id: &str,
    error: Option<&str>,
) -> Result<Reply, SettleError> {
    let line = response_line(request_id, error);
    match parse(line.as_bytes(), &Redactor::new()) {
        Ok(Message::Response { request_id, error }) => requests.settle(&request_id, error),
        other => panic!("{line} parsed as {other:?}"),
    }
}

fn untrusted(text: &str) -> Untrusted<String> {
    Untrusted::new(text.to_owned())
}

/// A short form of a parsed seed: its message kind and id, or `malformed`.
fn summary(line: &Line) -> String {
    match line {
        Line::Control(Message::Interrupt { request_id }) => format!("interrupt {request_id}"),
        Line::Control(Message::CanUseTool(ask)) => {
            format!("can_use_tool {} {}", ask.request_id, ask.tool_name)
        }
        Line::Control(Message::Request { request_id, subtype }) => {
            format!("request {subtype} {request_id}")
        }
        Line::Control(Message::Response { request_id, error: None }) => {
            format!("success {request_id}")
        }
        Line::Control(Message::Response { request_id, error: Some(error) }) => {
            format!("error {request_id}: {}", error.get())
        }
        Line::Control(Message::Cancel { request_id }) => format!("cancel {request_id}"),
        Line::Malformed { .. } => "malformed".to_owned(),
        other @ (Line::Init(_)
        | Line::Assistant(_)
        | Line::ToolResults(_)
        | Line::TextDelta(_)
        | Line::Partial
        | Line::RateLimit(_)
        | Line::HookStarted(_)
        | Line::HookResponse(..)
        | Line::PermissionDenied(_)
        | Line::Result(_)
        | Line::Unknown { .. }) => panic!("a control seed parsed as {other:?}"),
    }
}

/// Every seed of `fuzz/corpus/control_message/` and what it parses to.
const SEEDS: [(&str, &str); 18] = [
    ("can-use-tool-allow", "can_use_tool 70bd7443-478d-4b77-8d60-d5c6ed0c44fe Bash"),
    ("can-use-tool-deny", "can_use_tool 59734163-8fdc-47d0-b37c-77a0447b7b05 Bash"),
    ("hand-cancel", "cancel req_1_0a1b2c3d"),
    ("hand-error-secret", "error req_1_0a1b2c3d: fuzz-secret-7c1e was refused"),
    ("hand-interrupt", "interrupt req_1_0a1b2c3d"),
    ("hand-no-request-id", "malformed"),
    ("hand-not-json", "malformed"),
    ("hand-other-subtype", "request mcp_status req_2_0a1b2c3d"),
    ("hand-ready-error", "error eileen_ready_1: not ready"),
    ("hand-ready-other-subtype", "malformed"),
    ("hand-ready-pending", "success eileen_ready_1"),
    ("hand-ready-status", "success eileen_ready_1"),
    ("hand-reason-secret", "can_use_tool req_3_0a1b2c3d Bash"),
    ("hand-response-eileen-1", "success eileen_1"),
    ("hand-response-error", "error req_1_0a1b2c3d: no turn to interrupt"),
    ("hand-response-success", "success req_1_0a1b2c3d"),
    ("response-allow", "success 70bd7443-478d-4b77-8d60-d5c6ed0c44fe"),
    ("response-deny", "success 59734163-8fdc-47d0-b37c-77a0447b7b05"),
];

#[test]
fn each_seed_parses_to_its_typed_form_or_is_malformed_with_its_length() {
    let corpus =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/control_message");
    let mut names: Vec<String> = std::fs::read_dir(&corpus)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let listed: Vec<&str> = SEEDS.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, listed, "the table names every seed");
    for (name, expected) in SEEDS {
        let data = std::fs::read(corpus.join(name)).unwrap();
        let line = parse_line(&data, &Redactor::new());
        assert_eq!(summary(&line), expected, "{name}");
        if let Line::Malformed { line_len, .. } = line {
            assert_eq!(usize::try_from(line_len).unwrap(), data.len(), "{name}: its length");
        }
    }
}

#[test]
fn an_interrupt_round_trips_with_its_request_id() {
    let mut requests = Requests::default();
    let line = requests.interrupt(&init(&RECORDED_CAPABILITIES)).unwrap();
    let sent = Message::Interrupt { request_id: "eileen_1".to_owned() };
    assert_eq!(parse(line.as_bytes(), &Redactor::new()), Ok(sent.clone()), "{line}");
    assert_eq!(parse_line(line.as_bytes(), &Redactor::new()), Line::Control(sent), "the stream");
    assert!(!line.contains('\n'), "one line: {line}");
    let second = requests.interrupt(&init(&RECORDED_CAPABILITIES)).unwrap();
    assert_eq!(
        parse(second.as_bytes(), &Redactor::new()),
        Ok(Message::Interrupt { request_id: "eileen_2".to_owned() }),
        "each request has its own id"
    );
    assert_eq!(requests.open(), 2, "both wait for their reply");
}

#[test]
fn the_interrupt_is_sent_only_when_init_offers_it() {
    let mut requests = Requests::default();
    assert_eq!(requests.interrupt(&init(&[])), Err(SendError::NotOffered), "no capabilities");
    let others = init(&["interrupt_cancel_queued_v1", "msg_lifecycle_v1"]);
    assert_eq!(requests.interrupt(&others), Err(SendError::NotOffered), "other capabilities");
    assert_eq!(requests.open(), 0, "nothing was sent");
    assert!(requests.interrupt(&init(&[INTERRUPT_CAPABILITY])).is_ok(), "offered");
}

#[test]
fn replies_are_matched_by_request_id() {
    let mut requests = Requests::default();
    let offered = init(&RECORDED_CAPABILITIES);
    requests.interrupt(&offered).unwrap();
    requests.interrupt(&offered).unwrap();
    let failed = settle(&mut requests, "eileen_2", Some("no turn to interrupt"));
    assert_eq!(
        failed,
        Ok(Reply {
            request_id: "eileen_2".to_owned(),
            error: Some(untrusted("no turn to interrupt"))
        }),
        "an error reply matches its request"
    );
    let done = settle(&mut requests, "eileen_1", None);
    assert_eq!(done, Ok(Reply { request_id: "eileen_1".to_owned(), error: None }), "success");
    assert_eq!(requests.open(), 0, "both settled");
    assert_eq!(
        settle(&mut requests, "eileen_1", None),
        Err(SettleError::NotOpen),
        "a second reply to one request"
    );
}

#[test]
fn a_reply_with_no_or_an_unknown_request_id_is_malformed_not_matched() {
    let redactor = Redactor::new();
    let mut requests = Requests::default();
    requests.interrupt(&init(&RECORDED_CAPABILITIES)).unwrap();
    let no_id = br#"{"type":"control_response","response":{"subtype":"success"}}"#;
    assert!(parse(no_id, &redactor).is_err(), "no request_id");
    assert!(
        matches!(parse_line(no_id, &redactor), Line::Malformed { line_len, .. } if usize::try_from(line_len).unwrap() == no_id.len()),
        "the stream reads it as malformed with its length"
    );
    let empty = br#"{"type":"control_response","response":{"subtype":"success","request_id":""}}"#;
    assert!(parse(empty, &redactor).is_err(), "an empty request_id");
    let long = "r".repeat(MAX_REQUEST_ID_BYTES + 1);
    assert!(parse(interrupt(&long).as_bytes(), &redactor).is_err(), "a request_id over the cap");
    assert_eq!(
        settle(&mut requests, "eileen_9", None),
        Err(SettleError::NotOpen),
        "an id Eileen never sent"
    );
    assert_eq!(
        settle(&mut requests, "70bd7443-478d-4b77-8d60-d5c6ed0c44fe", Some("unknown")),
        Err(SettleError::NotOpen),
        "the CLI's own id"
    );
    assert_eq!(requests.open(), 1, "the open request is still open");
}

#[test]
fn open_requests_are_bounded_and_can_be_abandoned() {
    let mut requests = Requests::default();
    let offered = init(&RECORDED_CAPABILITIES);
    for _ in 0..MAX_OPEN_REQUESTS {
        requests.interrupt(&offered).unwrap();
    }
    assert_eq!(requests.interrupt(&offered), Err(SendError::Full), "at the cap");
    assert!(requests.abandon("eileen_1"), "the driver's deadline closes the oldest");
    assert!(!requests.abandon("eileen_1"), "only once");
    assert_eq!(
        settle(&mut requests, "eileen_1", None),
        Err(SettleError::NotOpen),
        "a late reply matches nothing"
    );
    let line = requests.interrupt(&offered).unwrap();
    assert!(line.contains(r#""request_id":"eileen_5""#), "ids are never reused: {line}");
}

#[test]
fn unanswered_requests_closed_at_a_turns_end_leave_room_for_the_next_interrupt() {
    let mut requests = Requests::default();
    let offered = init(&RECORDED_CAPABILITIES);
    // A CLI that never answers: four interrupts fill the cap.
    for _ in 0..MAX_OPEN_REQUESTS {
        requests.interrupt(&offered).unwrap();
    }
    assert_eq!(requests.interrupt(&offered), Err(SendError::Full), "at the cap");
    assert_eq!(requests.abandon_all(), MAX_OPEN_REQUESTS, "the turn's end closes all four");
    assert_eq!(requests.abandon_all(), 0, "nothing left open");
    let line = requests.interrupt(&offered).unwrap();
    assert!(line.contains(r#""request_id":"eileen_5""#), "a fifth goes out, numbered on: {line}");
    assert_eq!(
        settle(&mut requests, "eileen_4", None),
        Err(SettleError::NotOpen),
        "a late reply to an abandoned one settles nothing"
    );
}

#[test]
fn other_control_lines_parse_to_their_kind() {
    let redactor = Redactor::new();
    let cancel = br#"{"type":"control_cancel_request","request_id":"r"}"#;
    assert_eq!(parse(cancel, &redactor), Ok(Message::Cancel { request_id: "r".to_owned() }));
    let other =
        br#"{"type":"control_request","request_id":"r","request":{"subtype":"mcp_status"}}"#;
    let parsed = parse(other, &redactor);
    assert!(
        matches!(parsed, Ok(Message::Request { ref subtype, .. }) if subtype == "mcp_status"),
        "{parsed:?}"
    );
    let unknown_reply =
        br#"{"type":"control_response","response":{"subtype":"pending","request_id":"r"}}"#;
    assert!(parse(unknown_reply, &redactor).is_err(), "neither success nor error");
    assert!(parse(br#"{"type":"user"}"#, &redactor).is_err(), "not a control line");
}

#[test]
fn reason_and_error_text_are_redacted() {
    let secret = "hook-token-5f2a9c";
    let redactor = Arc::new(Redactor::new());
    let _token = redactor.add_literal(secret).unwrap();
    let ask = format!(
        r#"{{"type":"control_request","request_id":"r","request":{{"subtype":"can_use_tool","tool_name":"Bash","input":{{}},"decision_reason":"the guard saw {secret}"}}}}"#
    );
    let Ok(Message::CanUseTool(ask)) = parse(ask.as_bytes(), &redactor) else {
        panic!("{ask} is not a can_use_tool request");
    };
    let reason = ask.reason.unwrap();
    assert!(!reason.get().contains(secret), "the reason: {}", reason.get());
    assert!(reason.get().starts_with("the guard saw "), "the rest stays: {}", reason.get());
    let reply = response_line("r", Some(&format!("{secret} was refused")));
    let Ok(Message::Response { error: Some(error), .. }) = parse(reply.as_bytes(), &redactor)
    else {
        panic!("{reply} is not an error reply");
    };
    assert!(!error.get().contains(secret), "the error: {}", error.get());
    assert!(error.get().ends_with(" was refused"), "the rest stays: {}", error.get());
}
