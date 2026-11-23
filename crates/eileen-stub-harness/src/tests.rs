//! The player on in-memory stdio, the script loader and the choice of script.

use std::ffi::OsString;
use std::io::{self, BufRead, Cursor, Read};
use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::Value;

use crate::play::{self, Input, Request};
use crate::{Ending, InputError, RunError, SCRIPT_ARG, ScriptError, load, script_path};

/// Plays `script` with `inputs` already read and stdin closed after them, and gives its stdout
/// lines and how it ended.
fn played(script: &str, inputs: Vec<Input>) -> (Vec<String>, Result<Ending, RunError>) {
    let script = load(script).unwrap();
    let (queue, input) = mpsc::sync_channel(inputs.len().max(1));
    for one in inputs {
        queue.send(one).unwrap();
    }
    drop(queue);
    let mut out = Vec::new();
    let ending = play::play(&script, &input, &mut out);
    let text = String::from_utf8(out).unwrap();
    (text.lines().map(str::to_owned).collect(), ending)
}

fn interrupt(id: &str) -> Input {
    Input::Control { request_id: id.to_owned(), request: Request::Interrupt }
}

fn json(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

const INIT: &str =
    r#"{"kind":"event","json":{"type":"system","subtype":"init","session_id":"s1","model":"m"}}"#;

const AWAIT: &str = r#"{"kind":"await_user"}"#;

/// An init that lists `eileen` as connected.
const SERVED_INIT: &str = r#"{"kind":"event","json":{"type":"system","subtype":"init","session_id":"s1","mcp_servers":[{"name":"eileen","status":"connected"}]}}"#;

#[test]
fn the_argument_names_the_script_before_the_environment() {
    let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
    let env = Some(OsString::from("from-env.ndjson"));
    assert_eq!(
        script_path(args(&["-p", SCRIPT_ARG, "a.ndjson", "--verbose"]), env.clone()),
        Some(PathBuf::from("a.ndjson")),
        "the argument wins"
    );
    assert_eq!(
        script_path(args(&["--stub-script=b.ndjson"]), None),
        Some(PathBuf::from("b.ndjson")),
        "the joined form"
    );
    assert_eq!(
        script_path(args(&["-p", "--output-format", "stream-json"]), env),
        Some(PathBuf::from("from-env.ndjson")),
        "claude's argv alone leaves the environment to name it"
    );
    assert_eq!(script_path(args(&[SCRIPT_ARG]), None), None, "the flag without a path");
    assert_eq!(script_path(args(&[]), Some(OsString::new())), None, "an empty variable");
}

#[test]
fn a_script_loads_with_its_line_numbers_and_skips_blank_lines() {
    let script = load(&format!("{AWAIT}\n\n  \n{{\"kind\":\"delay\",\"ms\":1}}\n")).unwrap();
    let numbers: Vec<usize> = script.lines().iter().map(|(line, _)| *line).collect();
    assert_eq!(numbers, [1, 4], "each line keeps its number in the file");
}

#[test]
fn a_line_that_is_not_a_stub_script_line_names_its_number() {
    let error = load(&format!("{AWAIT}\n{{\"kind\":\"teleport\"}}\n")).unwrap_err();
    assert!(matches!(error, ScriptError::Parse { line: 2, .. }), "line 2: {error}");
    let error = load("not json").unwrap_err();
    assert!(matches!(error, ScriptError::Parse { line: 1, .. }), "line 1: {error}");
}

#[test]
fn a_crash_code_outside_an_exit_code_is_refused_at_load() {
    for code in [-1, 256] {
        let error = load(&format!("{{\"kind\":\"crash\",\"code\":{code}}}")).unwrap_err();
        assert!(
            matches!(error, ScriptError::CrashCode { line: 1, code: c } if c == code),
            "{code}: {error}"
        );
    }
}

#[test]
fn a_line_break_inside_one_stdout_line_is_refused_at_load() {
    for bytes in [r"a\nb", r"a\rb", r"a\r\nb"] {
        let script = format!(r#"{AWAIT}{}{{"kind":"malformed","bytes":"{bytes}"}}"#, '\n');
        let error = load(&script).unwrap_err();
        assert!(matches!(error, ScriptError::LineBreak { line: 2 }), "{bytes}: {error}");
    }
    let event = concat!(r#"{"kind":"event","json":{"type":"#, "\r", r#""system"}}"#);
    let error = load(event).unwrap_err();
    assert!(matches!(error, ScriptError::LineBreak { line: 1 }), "a CR between tokens: {error}");
}

#[test]
fn an_event_is_written_byte_for_byte_as_the_script_holds_it() {
    // Keys out of alphabetical order, a space after a colon, and numbers whose text a
    // re-serialised value would change.
    let json = concat!(
        r#"{"type":"system","subtype":"init","apiKeySource":"none","session_id": "s1","#,
        r#""total_cost_usd":0.30000000000000004,"duration_ms":1.0e2,"usage":{"z":1,"a":2}}"#,
    );
    let (lines, ending) = played(&format!(r#"{{"kind":"event","json":{json}}}"#), Vec::new());
    assert_eq!(lines, [json], "the script's own bytes");
    assert!(matches!(ending, Ok(Ending::Played)), "{ending:?}");
}

#[test]
fn the_json_text_is_found_wherever_and_however_the_line_writes_it() {
    let cases = [
        // `json` before `kind`, with spaces around the tokens.
        (r#"{ "json" : {"type":"x"} , "kind" : "event" }"#, r#"{"type":"x"}"#),
        // The key written with an escape, after a member whose string holds braces and quotes.
        (
            r#"{"kind":"event","note":"}\"{[","json":{"a":"]}\"","b":[1,{"c":null}]}}"#,
            r#"{"a":"]}\"","b":[1,{"c":null}]}"#,
        ),
        // Scalars and an array.
        (r#"{"kind":"event","json":-1.5e3}"#, "-1.5e3"),
        (r#"{"kind":"event","json":true}"#, "true"),
        (r#"{"kind":"event","json":"a\"b"}"#, r#""a\"b""#),
        (r#"{"kind":"event","json":[[],{}]}"#, "[[],{}]"),
    ];
    for (line, json) in cases {
        let (lines, _) = played(line, Vec::new());
        assert_eq!(lines, [json], "the json text of {line}");
    }
}

#[test]
fn events_and_malformed_bytes_are_written_one_per_line() {
    let malformed = r#"{"kind":"malformed","bytes":"{\"ty"}"#;
    let (lines, ending) = played(&format!("{INIT}\n{malformed}"), Vec::new());
    assert_eq!(json(&lines[0])["session_id"], "s1", "the event, as JSON");
    assert_eq!(lines[1], r#"{"ty"#, "the bytes, unchanged");
    assert!(matches!(ending, Ok(Ending::Played)), "{ending:?}");
}

#[test]
fn a_narration_is_an_assistant_text_with_each_line_prefixed() {
    let narration = r#"{"kind":"narration","line":"one\ntwo"}"#;
    let (lines, _) = played(&format!("{INIT}\n{narration}"), Vec::new());
    let event = json(&lines[1]);
    assert_eq!(event["type"], "assistant", "an assistant message");
    assert_eq!(event["session_id"], "s1", "in the session of the last init");
    assert_eq!(event["message"]["content"][0]["text"], "> one\n> two", "each line prefixed");
}

#[test]
fn await_user_answers_an_interrupt_and_holds_until_a_user_line() {
    let (lines, ending) = played(&format!("{AWAIT}\n{INIT}"), vec![interrupt("i1"), Input::User]);
    assert_eq!(lines.len(), 2, "the reply, then the event: {lines:?}");
    let reply = json(&lines[0]);
    assert_eq!(reply["type"], "control_response", "a control reply first");
    assert_eq!(reply["response"]["subtype"], "success", "a success");
    assert_eq!(reply["response"]["request_id"], "i1", "for that request");
    assert_eq!(json(&lines[1])["type"], "system", "then the turn");
    assert!(matches!(ending, Ok(Ending::Played)), "{ending:?}");
}

#[test]
fn another_control_request_gets_an_error_reply() {
    let request = Input::Control { request_id: "r1".to_owned(), request: Request::Other };
    let (lines, _) = played(AWAIT, vec![request, Input::User]);
    let reply = json(&lines[0]);
    assert_eq!(reply["response"]["subtype"], "error", "not a success");
    assert_eq!(reply["response"]["request_id"], "r1", "for that request");
}

#[test]
fn initialize_and_mcp_status_are_answered_before_the_first_user_line() {
    let initialize = Input::Control { request_id: "r1".to_owned(), request: Request::Initialize };
    let status = Input::Control { request_id: "r2".to_owned(), request: Request::McpStatus };
    let (lines, _) =
        played(&format!("{AWAIT}\n{SERVED_INIT}"), vec![initialize, status, Input::User]);
    assert_eq!(lines.len(), 3, "two replies, then the init: {lines:?}");
    let first = json(&lines[0]);
    assert_eq!(first["response"]["subtype"], "success", "initialize answered");
    assert_eq!(first["response"]["request_id"], "r1", "for that request");
    let second = json(&lines[1]);
    assert_eq!(second["response"]["request_id"], "r2", "mcp_status answered");
    assert_eq!(
        second["response"]["response"]["mcpServers"],
        serde_json::json!([{ "name": "eileen", "status": "connected" }]),
        "the servers of the script's first init, before it plays"
    );
    assert_eq!(json(&lines[2])["type"], "system", "then the init, after the user line");
}

#[test]
fn stdin_closing_at_await_user_ends_the_run_quietly() {
    let (lines, ending) = played(&format!("{AWAIT}\n{INIT}"), Vec::new());
    assert!(lines.is_empty(), "nothing played: {lines:?}");
    assert!(matches!(ending, Ok(Ending::InputClosed)), "{ending:?}");
}

#[test]
fn a_crash_line_ends_the_run_with_its_code_and_plays_nothing_after() {
    let crash = r#"{"kind":"crash","code":3}"#;
    let (lines, ending) = played(&format!("{INIT}\n{crash}\n{INIT}"), Vec::new());
    assert_eq!(lines.len(), 1, "only the line before the crash: {lines:?}");
    assert!(matches!(ending, Ok(Ending::Crash(3))), "{ending:?}");
}

#[test]
fn a_permission_request_is_not_played_and_nothing_after_it_is() {
    let script = concat!(
        r#"{"kind":"narration","line":"before"}"#,
        "\n",
        r#"{"kind":"permission_request","tool_name":"Bash","tool_input":{"command":"x"}}"#,
        "\n",
        r#"{"kind":"narration","line":"after"}"#,
    );
    let (lines, ending) = played(script, Vec::new());
    assert_eq!(lines.len(), 1, "only the line before it: {lines:?}");
    assert!(matches!(ending, Err(RunError::NotPlayed { line: 2 })), "{ending:?}");
}

#[test]
fn a_user_line_after_the_script_asks_for_a_turn_it_does_not_have() {
    let (_, ending) = played(INIT, vec![Input::User]);
    assert!(matches!(ending, Err(RunError::ScriptEnded)), "{ending:?}");
}

#[test]
fn a_broken_stdin_line_ends_the_run() {
    let (_, ending) = played(AWAIT, vec![Input::Broken(InputError::NotJson)]);
    assert!(matches!(ending, Err(RunError::Input(InputError::NotJson))), "{ending:?}");
}

#[test]
fn stdin_lines_are_classified_as_the_protocol() {
    let read = |text: &str| {
        let (queue, input) = mpsc::sync_channel(8);
        play::read_input(Cursor::new(text.as_bytes().to_vec()), &queue);
        drop(queue);
        input.iter().collect::<Vec<_>>()
    };
    let lines = concat!(
        r#"{"type":"user","message":{}}"#,
        "\r\n\n",
        r#"{"type":"control_request","request_id":"a","request":{"subtype":"interrupt"}}"#,
        "\n",
        r#"{"type":"control_cancel_request","request_id":"a"}"#,
        "\n",
        r#"{"type":"control_request","request":{"subtype":"interrupt"}}"#,
        "\n",
        r#"{"type":"user"}"#,
        "\n",
    );
    assert_eq!(
        read(lines),
        [Input::User, interrupt("a"), Input::Other, Input::Broken(InputError::BadRequestId),],
        "blank lines skipped, and reading stops at the first broken line"
    );
    assert_eq!(read("not json\n"), [Input::Broken(InputError::NotJson)], "not JSON");
    assert_eq!(read("{\"type\":1}\n"), [Input::Broken(InputError::NoType)], "no string type");
}

/// A reader whose every read fails.
struct Failing;

impl Read for Failing {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

impl BufRead for Failing {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn consume(&mut self, _: usize) {}
}

#[test]
fn a_failed_read_of_stdin_keeps_its_kind() {
    let (queue, input) = mpsc::sync_channel(1);
    play::read_input(Failing, &queue);
    drop(queue);
    let read: Vec<Input> = input.iter().collect();
    assert_eq!(read, [Input::Broken(InputError::Read(io::ErrorKind::BrokenPipe))], "the kind");
}
