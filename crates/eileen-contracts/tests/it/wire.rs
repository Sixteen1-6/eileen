//! The wire forms BUILD.md writes out, checked against the types: the dispatcher's rebuilt
//! tool arguments and the shim's pipe lines (section 3.3.9), the `approve` call and reply as
//! the real CLI sent and accepted them (2.5), the HUD's lines (3.3.6), a speech command frame
//! (3.3.7), an enveloped input (3.3.3) and a recording line (3.3.16).

use eileen_contracts::core_io::{Content, Input, ShimRole, UserInput};
use eileen_contracts::harness::{HookDecision, HookReply, SessionToken};
use eileen_contracts::hud::{HudInput, HudInputLine, MAX_LINE_BYTES, PROTOCOL_VERSION};
use eileen_contracts::ids::{Envelope, MonoTime, Personal, TraceCtx, TurnId, Untrusted, WallTime};
use eileen_contracts::record::{HookAnswerRecord, RecordLine};
use eileen_contracts::speech::{SpeechCmd, SpeechCommandFrame, VoiceEvent};
use eileen_contracts::tools::apps::{AppsMedia, AppsResolve, MediaAction, ResolveOp};
use eileen_contracts::tools::desktop::{DesktopWindow, WindowAction};
use eileen_contracts::tools::worker::{Approve, ApproveReply};
use eileen_contracts::tools::{
    CallLine, CallMeta, DISPATCH_VERSION, DispatcherLine, ResultLine, ShimHello, ShimLine, Welcome,
};
use proptest::prelude::*;
use serde_json::{Value, json};

fn round_trip<T>(value: &T, expected: &Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    assert_eq!(&serde_json::to_value(value).unwrap(), expected, "serialized form");
    assert_eq!(&serde_json::from_value::<T>(expected.clone()).unwrap(), value, "parsed back");
}

#[test]
fn tool_arguments_have_the_dispatcher_s_wire_form() {
    round_trip(
        &AppsMedia { action: MediaAction::Status, session: None },
        &json!({ "action": "status", "session": null }),
    );
    round_trip(
        &DesktopWindow { hwnd: 4523, action: WindowAction::Focus },
        &json!({ "hwnd": 4523, "action": "focus" }),
    );
    round_trip(
        &AppsResolve { op: ResolveOp::Goto { offset_s: 930.0 } },
        &json!({ "op": { "goto": { "offset_s": 930.0 } } }),
    );
}

#[test]
fn a_hud_input_line_has_the_documented_shape() {
    let job = "job_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap();
    let line =
        HudInputLine { v: PROTOCOL_VERSION, input: HudInput::CancelJob(job), physical: true };
    round_trip(
        &line,
        &json!({ "v": 1, "input": { "cancel_job": "job_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F" }, "physical": true }),
    );
    assert!(serde_json::to_vec(&line).unwrap().len() < MAX_LINE_BYTES, "within a line");
}

#[test]
fn a_speech_command_frame_carries_its_context_and_span() {
    let speak = "speak_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap();
    let frame =
        SpeechCommandFrame { ctx: TraceCtx::default(), span: 7, cmd: SpeechCmd::Cancel { speak } };
    round_trip(
        &frame,
        &json!({
            "ctx": { "turn": null, "job": null, "call": null },
            "span": 7,
            "cmd": { "cancel": { "speak": "speak_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F" } }
        }),
    );
}

#[test]
fn an_enveloped_input_round_trips() {
    let turn: TurnId = "turn_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap();
    let envelope = Envelope {
        seq: 41,
        at: WallTime(1_792_627_199_000_000),
        mono: MonoTime(12_000_000_000),
        ctx: TraceCtx { turn: Some(turn), job: None, call: None },
        body: Input::Voice(VoiceEvent::SpeechEnded { turn }),
    };
    let text = serde_json::to_string(&envelope).unwrap();
    assert_eq!(serde_json::from_str::<Envelope<Input>>(&text).unwrap(), envelope, "voice");
    let typed = Envelope {
        body: Input::User(UserInput::Typed { text: Personal::new("hi".to_owned()) }),
        ..envelope
    };
    assert_eq!(
        serde_json::to_value(&typed).unwrap()["body"],
        json!({ "user": { "typed": { "text": "hi" } } }),
        "externally tagged, snake_case"
    );
}

#[test]
fn a_hook_answer_line_names_the_input_it_answers() {
    let line = RecordLine::HookAnswer(HookAnswerRecord {
        input: 12,
        mono: MonoTime(5),
        reply: HookReply::PreToolUse {
            request: 3,
            decision: HookDecision::Ask { reason: "tier 2".to_owned() },
        },
    });
    round_trip(
        &line,
        &json!({
            "hook_answer": {
                "input": 12,
                "mono": 5,
                "reply": { "pre_tool_use": { "request": 3, "decision": { "ask": { "reason": "tier 2" } } } }
            }
        }),
    );
}

/// The four lines of section 3.3.9's shim protocol, as the plan writes them.
#[test]
fn the_shim_s_pipe_lines_have_the_plan_s_shape() {
    let worker = "ws_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap();
    for (role, text) in [
        (ShimRole::Brain, json!("brain")),
        (ShimRole::Worker(worker), json!({ "worker": "ws_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F" })),
        (ShimRole::Librarian, json!("librarian")),
        (ShimRole::Session, json!("session")),
        (ShimRole::Dev, json!("dev")),
    ] {
        let token = SessionToken::new("9f86d081884c7d65".to_owned());
        round_trip(
            &ShimLine::Hello(ShimHello { v: DISPATCH_VERSION, token, role }),
            &json!({ "hello": { "v": 1, "token": "9f86d081884c7d65", "role": text } }),
        );
    }
    round_trip(
        &DispatcherLine::Welcome(Welcome { tools: Vec::new() }),
        &json!({ "welcome": { "tools": [] } }),
    );
    let traceparent = "00-0198c6a3b2c94e5f8a7b6c5d4e3f2a1b-00f067aa0ba902b7-01";
    round_trip(
        &ShimLine::Call(CallLine {
            id: 7,
            tool: "jobs_status".parse().unwrap(),
            args: Untrusted::new(json!({ "job": null })),
            meta: CallMeta { traceparent: Some(traceparent.to_owned()) },
        }),
        &json!({ "call": { "id": 7, "tool": "jobs_status", "args": { "job": null }, "meta": { "traceparent": traceparent } } }),
    );
    round_trip(
        &DispatcherLine::Result(ResultLine {
            id: 7,
            content: vec![Content::Text(Untrusted::new("2 jobs running".to_owned()))],
            is_error: false,
        }),
        &json!({ "result": { "id": 7, "content": [{ "text": "2 jobs running" }], "is_error": false } }),
    );
    let bare: ShimLine = serde_json::from_value(
        json!({ "call": { "id": 8, "tool": "jobs_status", "args": {}, "meta": {} } }),
    )
    .unwrap();
    assert!(
        matches!(bare, ShimLine::Call(CallLine { meta: CallMeta { traceparent: None }, .. })),
        "a call with no traceparent"
    );
    let wrong_way = json!({ "welcome": { "tools": [] } });
    assert!(serde_json::from_value::<ShimLine>(wrong_way).is_err(), "a shim never sends a welcome");
}

/// The approve server's conversation with the real CLI (2.1.304) in WP3's contract test: the
/// `tools/call` it received and the `allow` and `deny` replies the CLI accepted.
const MCP_APPROVE: &str =
    include_str!("../../../../docs/research/fixtures/wp3-approval/mcp-approve.json");

/// The hook requests of the same test, among them the tier guard's `ask` for that call.
const HOOKS: &str = include_str!("../../../../docs/research/fixtures/wp3-approval/hooks.json");

/// The recorded messages of one conversation of `mcp-approve.json` in one direction.
fn recorded(conversation: &str, dir: &str) -> Vec<Value> {
    let fixture: Value = serde_json::from_str(MCP_APPROVE).unwrap();
    let lines = fixture[conversation].as_array().unwrap();
    lines.iter().filter(|line| line["dir"] == dir).map(|line| line["msg"].clone()).collect()
}

/// The text of the one block in the recorded reply to the `approve` call (JSON-RPC id 2).
fn recorded_reply(conversation: &str) -> String {
    let replies = recorded(conversation, "send");
    let reply = replies.iter().find(|msg| msg["id"] == 2).unwrap();
    let content = reply["result"]["content"].as_array().unwrap();
    assert_eq!(content.len(), 1, "{conversation}: one block");
    assert_eq!(content[0]["type"], "text", "{conversation}: a text block");
    content[0]["text"].as_str().unwrap().to_owned()
}

/// A recorded reply's JSON as serde_json writes it, compact (the recording's keys are already in
/// sorted order, which is also the order the reply writes them).
fn compact(text: &str) -> String {
    serde_json::to_string(&serde_json::from_str::<Value>(text).unwrap()).unwrap()
}

/// The body of the `PreToolUse` request the guard answered `ask`, whose input it stored.
fn guard_ask() -> Value {
    let hooks: Value = serde_json::from_str(HOOKS).unwrap();
    let ask = &hooks["pre_tool_use_ask"];
    assert_eq!(ask["reply"]["hookSpecificOutput"]["permissionDecision"], "ask", "the guard asked");
    ask["body"].clone()
}

/// The CLI's `tools/call` for `approve` parses as `Approve`, and it answers the guard's ask:
/// the same `tool_use_id` and an equal input. A changed input or another id does not.
#[test]
fn the_recorded_approve_call_parses_and_matches_the_guard_s_ask() {
    let calls = recorded("allow", "recv");
    let call = calls.iter().find(|msg| msg["method"] == "tools/call").unwrap();
    assert_eq!(call["params"]["name"], "approve", "the tool");
    let approve: Approve = serde_json::from_value(call["params"]["arguments"].clone()).unwrap();
    assert_eq!(
        approve,
        Approve {
            tool_name: "Bash".to_owned(),
            input: json!({
                "command": "touch tier2-asked.txt",
                "description": "Create empty file tier2-asked.txt"
            }),
            tool_use_id: "toolu_01RoL4SGHmKKGragXFts7H5p".to_owned(),
        },
        "the recorded arguments"
    );
    assert_eq!(
        call["params"]["_meta"]["claudecode/toolUseId"],
        approve.tool_use_id.as_str(),
        "the request's _meta names the same use"
    );
    let ask = guard_ask();
    let id = ask["tool_use_id"].as_str().unwrap();
    let stored = Untrusted::new(ask["tool_input"].clone());
    assert!(approve.matches_ask(id, &stored), "the call the CLI made answers the guard's ask");
    let mut changed = ask["tool_input"].clone();
    changed["command"] = json!("touch somewhere-else.txt");
    assert!(!approve.matches_ask(id, &Untrusted::new(changed)), "a different input");
    assert!(!approve.matches_ask("toolu_01OTHERASK", &stored), "another ask");
}

/// The reply, built from the input the guard stored, is the text the CLI accepted: one block
/// holding `{"behavior":"allow","updatedInput":...}` or `{"behavior":"deny","message":...}`.
#[test]
fn the_approve_reply_matches_the_recorded_allow_and_deny_lines() {
    let allow = ApproveReply::allow(Untrusted::new(guard_ask()["tool_input"].clone()));
    let deny = ApproveReply::deny("Eileen: the user declined this (test)");
    for (conversation, reply) in [("allow", &allow), ("deny_reply", &deny)] {
        let line = recorded_reply(conversation);
        assert_eq!(reply.text(), compact(&line), "{conversation}: the text");
        assert_eq!(
            reply.content(),
            Content::Text(Untrusted::new(compact(&line))),
            "{conversation}: the one text block"
        );
        let parsed: ApproveReply = serde_json::from_str(&line).unwrap();
        assert_eq!(&parsed, reply, "{conversation}: the recorded text parses back");
    }
    assert!(
        serde_json::from_value::<ApproveReply>(json!({ "behavior": "ask" })).is_err(),
        "no third shape"
    );
}

/// Any JSON value: escapes, every char, large and negative integers, floats and nesting.
fn json_value() -> impl Strategy<Value = Value> {
    let text = prop::collection::vec(any::<char>(), 0..8)
        .prop_map(|chars| chars.into_iter().collect::<String>());
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        any::<f64>()
            .prop_filter_map("finite", |x| serde_json::Number::from_f64(x).map(Value::Number)),
        text.clone().prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 32, 4, move |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::vec((text.clone(), inner), 0..4)
                .prop_map(|pairs| Value::Object(pairs.into_iter().collect())),
        ]
    })
}

proptest! {
    /// `allow` carries whatever input was stored, unchanged: its text is that value exactly as
    /// serde_json writes it on its own, inside the allow shape and nothing else.
    #[test]
    fn allow_carries_any_stored_value_unchanged(value in json_value()) {
        let input = serde_json::to_string(&value).unwrap();
        let text = ApproveReply::allow(Untrusted::new(value)).text();
        prop_assert_eq!(&text, &format!(r#"{{"behavior":"allow","updatedInput":{input}}}"#));
        let parsed = serde_json::from_str::<ApproveReply>(&text);
        prop_assert!(matches!(parsed, Ok(ApproveReply::Allow { .. })), "reads back as allow");
    }
}
