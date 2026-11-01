//! The parser and both mappings against the lines CLI 2.1.304 wrote in WP3's approval contract
//! test (copies under `tests/fixtures/wp3-approval/`), and against the documented shapes those
//! fixtures leave out.

use std::sync::Arc;

use eileen_contracts::harness::{BrainEvent, BrainSlot, WorkerEvent};
use eileen_contracts::ids::{TurnId, Untrusted, WallTime};
use eileen_contracts::jobs::{Cost, PlanUsage, Route};
use eileen_trace::Redactor;
use serde_json::{Value, json};

use super::{
    Block, BrainAt, Denial, DenialKind, HookOutcome, Line, MAX_LINE_BYTES, MAX_TOOL_INPUT_BYTES,
    ToolResult, brain_events, parse, worker_events,
};

const EVENTS: &str = include_str!("../../tests/fixtures/wp3-approval/stream-events.json");
const PT_ALLOW: &str = include_str!("../../tests/fixtures/wp3-approval/stream-pt-allow.json");
const STEER: &str = include_str!("../../tests/fixtures/wp3-approval/stream-steer.json");
const STDIO_ALLOW: &str = include_str!("../../tests/fixtures/wp3-approval/stream-stdio-allow.json");

/// The pid the worker mapping is given.
const PID: u32 = 4242;

/// One recorded event, written as the line the CLI writes.
fn parse_value(event: &Value) -> Line {
    parse(event.to_string().as_bytes(), &Redactor::new())
}

/// A recorded run's events, in order, without the runner's own entries.
fn run(fixture: &str) -> Vec<Value> {
    let entries: Vec<Value> = serde_json::from_str(fixture).unwrap();
    entries.into_iter().filter_map(|entry| entry.get("event").cloned()).collect()
}

/// `stream-events.json`.
fn events() -> Value {
    serde_json::from_str(EVENTS).unwrap()
}

/// The `index`th `tool_result` block recorded for a case, inside the `user` line the runs show.
fn tool_result_line_at(case: &str, index: usize) -> Line {
    let recorded = events();
    let block = &recorded["tool_results"][case][index];
    assert!(block.is_object(), "stream-events.json records a tool_result for {case}");
    parse_value(&json!({ "type": "user", "message": { "role": "user", "content": [block] } }))
}

/// The first `tool_result` block recorded for a case, as a line.
fn tool_result_line(case: &str) -> Line {
    tool_result_line_at(case, 0)
}

/// The one tool result a line carries.
fn only_result(line: &Line) -> ToolResult {
    let Line::ToolResults(results) = line else { panic!("expected tool results, got {line:?}") };
    assert_eq!(results.len(), 1, "one tool result: {results:?}");
    results[0].clone()
}

/// A short name for a line of the pt-allow run.
fn kind(line: &Line) -> &'static str {
    match line {
        Line::Init(_) => "init",
        Line::Assistant(_) => "assistant",
        Line::ToolResults(_) => "tool results",
        Line::TextDelta(_) => "text delta",
        Line::Partial => "partial",
        Line::RateLimit(_) => "rate limit",
        Line::HookStarted(hook) => {
            if hook.event == "PreToolUse" {
                "guard started"
            } else {
                "hook started"
            }
        }
        Line::HookResponse(hook, outcome) => {
            if *outcome == HookOutcome::Success {
                if hook.event == "PreToolUse" { "guard answered" } else { "hook answered" }
            } else {
                "hook failed"
            }
        }
        Line::PermissionDenied(_) => "permission denied",
        Line::Result(_) => "result",
        Line::Unknown { .. } => "unknown",
        Line::Malformed { .. } => "malformed",
    }
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().unwrap().iter().map(|item| item.as_str().unwrap().to_owned()).collect()
}

fn untrusted(text: &str) -> Untrusted<String> {
    Untrusted::new(text.to_owned())
}

#[test]
fn the_pt_allow_run_parses_to_its_event_sequence() {
    let lines: Vec<Line> = run(PT_ALLOW).iter().map(parse_value).collect();
    let kinds: Vec<&str> = lines.iter().map(kind).collect();
    let expected = [
        "init",
        "assistant",
        "guard started",
        "guard answered",
        "hook started",
        "hook answered",
        "tool results",
        "assistant",
        "hook started",
        "hook answered",
        "result",
    ];
    assert_eq!(kinds, expected, "the run's lines, in order");
    let Line::Init(init) = &lines[0] else { panic!("the run starts with init") };
    assert!(init.connected("eileen"), "the approve server is connected");
    assert!(!init.connected("other"), "no other server is");
    assert!(init.offers("interrupt_receipt_v1"), "the interrupt is offered");
    assert_eq!(init.permission_mode.as_deref(), Some("dontAsk"), "the pinned mode");
    assert_eq!(init.cli_version.as_deref(), Some("2.1.304"), "the recorded CLI");
    assert!(init.memory_path.is_none(), "auto memory was off for the run");
    assert!(lines.iter().all(|line| line.denials().is_empty()), "an allowed ask is no denial");
}

#[test]
fn the_pt_allow_run_maps_to_worker_events() {
    let events = run(PT_ALLOW);
    let mapped: Vec<WorkerEvent> =
        events.iter().flat_map(|event| worker_events(parse_value(event), PID)).collect();
    let id = "toolu_01RoL4SGHmKKGragXFts7H5p";
    let answer = "1. `touch tier2-asked.txt` - ran (completed with no output).";
    let expected = vec![
        WorkerEvent::Init {
            session_id: "a3b57c79-73b3-485d-8511-c5ae9b1cdac5".to_owned(),
            pid: PID,
            model: "claude-haiku-5-5".to_owned(),
            tools: vec!["Bash".to_owned()],
            mcp_servers: vec!["eileen:connected".to_owned()],
            plugins: Vec::new(),
            capabilities: strings(&events[0]["capabilities"]),
        },
        WorkerEvent::ToolUse {
            tool_use_id: id.to_owned(),
            name: "Bash".to_owned(),
            input: Untrusted::new(json!({
                "command": "touch tier2-asked.txt",
                "description": "Create empty file tier2-asked.txt",
            })),
        },
        WorkerEvent::ToolResult { tool_use_id: id.to_owned(), ok: true, observed: None },
        WorkerEvent::Text { delta: untrusted(answer) },
        WorkerEvent::TurnDone {
            cost: Cost {
                duration_ms: 16_622,
                api_ms: 2_621,
                model_turns: 2,
                tokens_in: 4 + 1_130,
                tokens_out: 118,
                tokens_cache_read: 9_066,
            },
            is_error: false,
            summary: untrusted(answer),
        },
    ];
    assert_eq!(mapped, expected, "the worker events of the pt-allow run");
    let Line::Result(result) = parse_value(&events[10]) else { panic!("the run ends with result") };
    assert_eq!(result.context_tokens, 2 + 130 + 5_033 + 27, "the last request's tokens");
}

#[test]
fn the_guard_deny_is_a_denial_in_the_tool_result_and_the_result() {
    let result = only_result(&tool_result_line("guard-deny"));
    assert!(result.is_error, "the call was refused");
    assert_eq!(result.denial, Some(DenialKind::Guard), "the guard refused it");
    let id = "toolu_01BUAWis45bYchXjZgxUTqVZ";
    let listed = parse_value(&events()["results"]["guard-deny"][0]).denials();
    let expected = Denial {
        tool_use_id: Some(id.to_owned()),
        tool_name: Some("Bash".to_owned()),
        kind: DenialKind::Listed,
    };
    assert_eq!(listed, [expected], "the result lists the same call");
    let from_result = tool_result_line("guard-deny").denials();
    assert_eq!(from_result[0].tool_use_id.as_deref(), Some(id), "the same id, counted once");
}

#[test]
fn failed_approve_calls_count_as_denials() {
    for case in ["pt-iserror", "pt-exit", "pt-hang-mcptimeout", "pt-missing-server"] {
        let line = tool_result_line(case);
        assert_eq!(only_result(&line).denial, Some(DenialKind::ApproveFailed), "{case}");
        assert_eq!(line.denials().len(), 1, "{case} counts as one denial");
    }
    let result = parse_value(&events()["results"]["pt-iserror"][0]);
    assert!(result.denials().is_empty(), "the CLI's result leaves the failed approve call out");
}

#[test]
fn every_failed_guard_blocks_as_a_guard_denial() {
    let cases = [
        "guard-dead-block",
        "guard-hang-block",
        "guard-garbage-block",
        "guard-500-block",
        "guard-baddecision-block",
        "steer",
    ];
    for case in cases {
        assert_eq!(only_result(&tool_result_line(case)).denial, Some(DenialKind::Guard), "{case}");
    }
}

#[test]
fn route_and_mode_denials_are_told_apart_from_tool_errors() {
    let denial_of = |case: &str| only_result(&tool_result_line(case)).denial;
    let invalid = denial_of("pt-garbage");
    assert_eq!(invalid, Some(DenialKind::ApproveInvalid), "a malformed approve reply");
    assert_eq!(denial_of("pr-unlisted-dontAsk"), Some(DenialKind::Mode), "the mode's refusal");
    let declined = only_result(&tool_result_line("pt-deny"));
    assert!(declined.is_error, "an approve deny is an error");
    assert_eq!(declined.denial, None, "its text is Eileen's own; the result lists it");
    let ran = tool_result_line_at("steer", 1);
    assert_eq!(only_result(&ran).denial, None, "a call that ran");
    assert!(!only_result(&ran).is_error, "and succeeded");
}

#[test]
fn permission_denied_events_name_their_reason() {
    let recorded = events();
    let Line::PermissionDenied(asked) = parse_value(&recorded["permission_denied_hook_ask"][0])
    else {
        panic!("a permission_denied event")
    };
    assert_eq!(asked.kind, DenialKind::Guard, "an ask that found no route");
    assert_eq!(asked.tool_name.as_deref(), Some("Bash"), "the tool");
    assert_eq!(asked.tool_use_id.as_deref(), Some("toolu_01AFdFK9efL5UDFfPT2PJYM1"), "the call");
    let Line::PermissionDenied(mode) = parse_value(&recorded["permission_denied_mode"][0]) else {
        panic!("a permission_denied event")
    };
    assert_eq!(mode.kind, DenialKind::Mode, "the mode refused it");
    let failed = parse_value(&recorded["hook_response_failed_block"][0]);
    assert!(matches!(failed, Line::HookResponse(_, HookOutcome::Error)), "{failed:?}");
}

#[test]
fn the_steer_run_s_second_turn_starts_with_init_again() {
    let lines: Vec<Line> = run(STEER).iter().map(parse_value).collect();
    let inits = lines.iter().filter(|line| matches!(line, Line::Init(_))).count();
    assert_eq!(inits, 2, "the CLI writes init at the start of each turn");
    let results: Vec<&Line> = lines.iter().filter(|line| matches!(line, Line::Result(_))).collect();
    assert_eq!(results.len(), 2, "one result per turn");
    assert_eq!(results[0].denials().len(), 1, "the first turn's call waited for the user");
    assert!(results[1].denials().is_empty(), "the steered retry ran");
    let unknown = Line::Unknown { kind: "system/thinking_tokens".to_owned() };
    assert!(lines.contains(&unknown), "a system subtype the parser keeps nothing of");
    assert!(lines.iter().all(|line| !matches!(line, Line::Malformed { .. })), "every line parses");
}

#[test]
fn an_unknown_type_is_unknown() {
    let line = parse(br#"{"type":"surprise","detail":1}"#, &Redactor::new());
    assert_eq!(line, Line::Unknown { kind: "surprise".to_owned() }, "an unknown type");
    assert!(worker_events(line, PID).is_empty(), "it gives no worker event");
    let long = format!(r#"{{"type":"{}"}}"#, "x".repeat(500));
    let Line::Unknown { kind } = parse(long.as_bytes(), &Redactor::new()) else {
        panic!("a long unknown type is still unknown")
    };
    assert_eq!(kind.len(), super::MAX_KIND_BYTES, "its name is capped");
}

#[test]
fn an_over_long_line_is_malformed_before_parsing() {
    let shell = r#"{"type":"assistant","message":{"content":[{"type":"text","text":""}]}}"#;
    let fill = MAX_LINE_BYTES - shell.len();
    let at_cap = shell.replace(r#""text":"""#, &format!(r#""text":"{}""#, "a".repeat(fill)));
    assert_eq!(at_cap.len(), MAX_LINE_BYTES, "a line exactly at the cap");
    assert!(matches!(parse(at_cap.as_bytes(), &Redactor::new()), Line::Assistant(_)), "parses");
    let over = format!("{at_cap} ");
    let line = parse(over.as_bytes(), &Redactor::new());
    let line_len = u32::try_from(over.len()).unwrap();
    assert!(matches!(line, Line::Malformed { line_len: len, .. } if len == line_len), "{line:?}");
    assert_eq!(worker_events(line, PID), [WorkerEvent::Malformed { line_len }], "one event");
}

#[test]
fn invalid_lines_are_malformed_with_their_length() {
    let invalid: [&[u8]; 6] = [
        b"not json",
        b"[1,2]",
        br#"{"type":3}"#,
        b"{\"type\":\"\xff\"}",
        br#"{"type":"system","subtype":"init","model":"m"}"#,
        br#"{"type":"user","message":{"content":[{"type":"tool_result","is_error":true}]}}"#,
    ];
    for line in invalid {
        let line_len = u32::try_from(line.len()).unwrap();
        let parsed = parse(line, &Redactor::new());
        assert!(
            matches!(parsed, Line::Malformed { line_len: len, .. } if len == line_len),
            "{parsed:?}"
        );
    }
}

#[test]
fn a_skill_call_becomes_skill_invoked() {
    let line = br#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Skill","input":{"skill":"eileen-learned:resolve-goto-offset","args":"15:30"}}]}}"#;
    let events = worker_events(parse(line, &Redactor::new()), PID);
    let expected = WorkerEvent::SkillInvoked {
        name: "eileen-learned:resolve-goto-offset".to_owned(),
        args: untrusted("15:30"),
    };
    assert_eq!(events, [expected], "the Skill call, and no tool use beside it");
    let unnamed = br#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_2","name":"Skill","input":{}}]}}"#;
    let events = worker_events(parse(unnamed, &Redactor::new()), PID);
    assert!(matches!(&events[..], [WorkerEvent::ToolUse { .. }]), "no skill named: {events:?}");
}

#[test]
fn narration_lines_are_split_from_text() {
    let text =
        "Starting.\n> Opening Resolve\nThe timeline is open.\r\n> \n>not narration\r\n>  Done  ";
    let line = json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": text }] } });
    let Line::Assistant(blocks) = parse_value(&line) else { panic!("an assistant line") };
    let expected = [
        Block::Text(untrusted("Starting.")),
        Block::Narration(untrusted("Opening Resolve")),
        Block::Text(untrusted("The timeline is open.\n>not narration")),
        Block::Narration(untrusted("Done")),
    ];
    assert_eq!(
        blocks, expected,
        "text and narration, in order; an empty narration line is dropped"
    );
    let events = worker_events(Line::Assistant(blocks), PID);
    assert!(
        matches!(events[1], WorkerEvent::Narration { .. }),
        "narration reaches the worker events"
    );
}

#[test]
fn a_tool_input_is_redacted_and_capped() {
    let redactor = Arc::new(Redactor::new());
    let _token = redactor.add_literal("hook-token-5f2a9c").unwrap();
    let secret =
        json!({ "command": "echo ghp_abcdefghijklmnopqrstuvwxyz0123456789 hook-token-5f2a9c" });
    let line = json!({ "type": "assistant", "message": { "content": [
        { "type": "tool_use", "id": "toolu_1", "name": "Bash", "input": secret },
        { "type": "tool_use", "id": "toolu_2", "name": "Write", "input": { "content": "x".repeat(5_000) } },
    ] } });
    let Line::Assistant(blocks) = parse(line.to_string().as_bytes(), &redactor) else {
        panic!("an assistant line")
    };
    let [Block::ToolUse(redacted), Block::ToolUse(long)] = &blocks[..] else {
        panic!("{blocks:?}")
    };
    let command = redacted.input.get()["command"].as_str().unwrap();
    assert!(!command.contains("ghp_"), "the token is gone: {command}");
    assert!(!command.contains("hook-token-5f2a9c"), "the session's literal is gone: {command}");
    assert!(command.contains("[redacted:"), "and marked: {command}");
    let Value::String(cut) = long.input.get() else {
        panic!("an input over the cap is cut to text")
    };
    assert!(cut.len() <= MAX_TOOL_INPUT_BYTES, "{} bytes", cut.len());
    assert!(cut.starts_with(r#"{"content":"xxx"#), "it keeps the start: {cut}");
}

#[test]
fn an_auth_failure_in_the_result_becomes_auth_failed() {
    let failed = |text: &str, status: Option<u16>| {
        json!({ "type": "result", "subtype": "success", "is_error": true, "num_turns": 0,
                "duration_ms": 120, "duration_api_ms": 0, "result": text, "api_error_status": status })
    };
    for (text, status) in [
        ("Failed to authenticate: OAuth session expired and could not be refreshed", None),
        ("Not logged in \u{b7} Please run /login", None),
        ("API Error: something went wrong", Some(401)),
    ] {
        let events = worker_events(parse_value(&failed(text, status)), PID);
        let [WorkerEvent::AuthFailed { message }, WorkerEvent::TurnDone { is_error: true, .. }] =
            &events[..]
        else {
            panic!("{text}: {events:?}")
        };
        assert_eq!(message.get(), text, "the vendor's message");
    }
    let answer = json!({ "type": "result", "subtype": "success", "is_error": false, "num_turns": 1,
                         "result": "Failed to authenticate with the registry, so I stopped." });
    let events = worker_events(parse_value(&answer), PID);
    assert!(
        matches!(&events[..], [WorkerEvent::TurnDone { .. }]),
        "an answer is no failure: {events:?}"
    );
}

#[test]
fn rate_limit_events_become_plan_usage() {
    let event = |info: Value| json!({ "type": "rate_limit_event", "rate_limit_info": info, "uuid": "u", "session_id": "s" });
    let five_hour = event(json!({ "status": "allowed_warning", "resetsAt": 1_761_055_200,
                                  "rateLimitType": "five_hour", "utilization": 0.83 }));
    let expected = PlanUsage {
        five_hour_pct: Some(83),
        seven_day_pct: None,
        resets_at: Some(WallTime(1_761_055_200_000_000)),
    };
    assert_eq!(parse_value(&five_hour), Line::RateLimit(expected), "the five-hour window");
    let rejected = event(json!({ "status": "rejected", "rateLimitType": "seven_day" }));
    let Line::RateLimit(usage) = parse_value(&rejected) else { panic!("a rate-limit line") };
    assert_eq!(usage.seven_day_pct, Some(100), "a rejected window is full");
    assert_eq!(
        worker_events(Line::RateLimit(usage), PID),
        [WorkerEvent::RateLimit(usage)],
        "worker"
    );
    let at = BrainAt { which: BrainSlot::Primary, instance: 3, turn: None };
    assert_eq!(brain_events(Line::RateLimit(usage), at), [BrainEvent::RateLimit(usage)], "brain");
}

#[test]
fn partial_text_reaches_the_brain_and_not_the_worker() {
    let delta = json!({ "type": "stream_event", "session_id": "s", "uuid": "u",
        "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Sure" } } });
    let line = parse_value(&delta);
    assert_eq!(line, Line::TextDelta(untrusted("Sure")), "a text delta");
    assert!(worker_events(line.clone(), PID).is_empty(), "the worker waits for the whole message");
    let turn = TurnId::from_parts(1, 2);
    let at = BrainAt { which: BrainSlot::Spare, instance: 9, turn: Some(turn) };
    let spoken = BrainEvent::TextDelta { instance: 9, turn, text: untrusted("Sure") };
    assert_eq!(brain_events(line.clone(), at), [spoken], "the brain speaks it");
    assert!(brain_events(line, BrainAt { turn: None, ..at }).is_empty(), "not without a turn");
    let start = json!({ "type": "stream_event", "event": { "type": "message_start" } });
    assert_eq!(parse_value(&start), Line::Partial, "other partial messages");
    let events = run(PT_ALLOW);
    let model = "claude-haiku-5-5".to_owned();
    let ready =
        BrainEvent::Ready { which: BrainSlot::Spare, instance: 9, route: Route::Claude, model };
    assert_eq!(brain_events(parse_value(&events[0]), at), [ready], "init makes the brain ready");
    let done = brain_events(parse_value(&events[10]), at);
    let [BrainEvent::TurnDone { context_tokens, is_error: false, .. }] = &done[..] else {
        panic!("a result ends the brain's turn: {done:?}")
    };
    assert_eq!(*context_tokens, 5_192, "the context after the turn");
}

#[test]
fn every_line_of_the_stdio_run_parses() {
    let lines: Vec<Line> = run(STDIO_ALLOW).iter().map(parse_value).collect();
    let kinds: Vec<&str> = lines.iter().map(kind).collect();
    assert!(!kinds.contains(&"malformed"), "every line parses: {kinds:?}");
    // The control protocol's parser is the control_message half, in its own pull request; until
    // it lands, the CLI's can_use_tool request is a line this parser does not know.
    let control = Line::Unknown { kind: "control_request".to_owned() };
    assert_eq!(lines.iter().filter(|line| **line == control).count(), 1, "the one control line");
    assert_eq!(kinds.iter().filter(|name| **name == "result").count(), 1, "one turn");
}

#[test]
fn the_subagent_constant_names_both_spellings() {
    // system/init lists the tool as Task, and the model calls it Agent (section 2.5 of the
    // approval contract report), so both are disallowed by name.
    assert_eq!(crate::SUBAGENT_TOOLS, ["Task", "Agent"], "both spellings");
}
