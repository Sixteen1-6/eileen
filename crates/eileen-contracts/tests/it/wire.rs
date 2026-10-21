//! The wire forms BUILD.md writes out, checked against the types: the dispatcher's rebuilt
//! tool arguments (section 3.3.9), the HUD's lines (3.3.6), a speech command frame (3.3.7), an
//! enveloped input (3.3.3) and a recording line (3.3.16).

use eileen_contracts::core_io::{Input, UserInput};
use eileen_contracts::harness::{HookDecision, HookReply};
use eileen_contracts::hud::{HudInput, HudInputLine, MAX_LINE_BYTES, PROTOCOL_VERSION};
use eileen_contracts::ids::{Envelope, MonoTime, Personal, TraceCtx, TurnId, WallTime};
use eileen_contracts::record::{HookAnswerRecord, RecordLine};
use eileen_contracts::speech::{SpeechCmd, SpeechCommandFrame, VoiceEvent};
use eileen_contracts::tools::apps::{AppsMedia, AppsResolve, MediaAction, ResolveOp};
use eileen_contracts::tools::desktop::{DesktopWindow, WindowAction};
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
