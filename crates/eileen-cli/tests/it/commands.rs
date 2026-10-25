//! Every command form of BUILD.md section 3.5 parses and reaches a crate that says it is not
//! built yet, and `eileen run` wires every component.

use clap::Parser as _;
use eileen_cli::{Cli, run, wiring};
use eileen_runtime::cli::RunArgs;

/// One invocation of every form section 3.5 lists, flags included.
const COMMANDS: &[&[&str]] = &[
    &["run"],
    &["run", "--data-dir", "d", "--audio-source", "wav:in.wav", "--audio-sink", "capture:out.wav"],
    &["run", "--stub-harness", "s.ndjson", "--replay-models", "models", "--muted", "--no-hud"],
    &["run", "--console", "--test-harness"],
    &["say", "what time is it"],
    &["bench", "brain", "--backend", "claude"],
    &["status"],
    &["jobs"],
    &["jobs", "list"],
    &["jobs", "tail", "job_1"],
    &["jobs", "cancel", "job_1"],
    &["about"],
    &["setup"],
    &["doctor"],
    &["doctor", "claude"],
    &["doctor", "audio"],
    &["doctor", "resolve"],
    &["doctor", "resolve", "--route", "mcp"],
    &["doctor", "resolve", "--route", "bridge"],
    &["doctor", "desktop"],
    &["doctor", "machine"],
    &["doctor", "machine", "--bench"],
    &["config"],
    &["config", "explain"],
    &["config", "edit"],
    &["config", "undo"],
    &["config", "profiles"],
    &["config", "export"],
    &["config", "import"],
    &["skills"],
    &["skills", "list"],
    &["skills", "show"],
    &["skills", "history"],
    &["skills", "rollback"],
    &["skills", "forget"],
    &["skills", "distill"],
    &["skills", "distill", "--job", "job_1", "--wait"],
    &["facts", "list"],
    &["facts", "forget"],
    &["apps", "proposals"],
    &["apps", "add-mcp", "resolve", "io.example/resolve"],
    &["notes"],
    &["notes", "list"],
    &["notes", "show"],
    &["notes", "forget"],
    &["notes", "keep"],
    &["notes", "drop"],
    &["voice", "list"],
    &["voice", "learn"],
    &["voice", "use", "nova", "--engine", "chatterbox-turbo"],
    &["voice", "delete"],
    &["voice", "render", "hello", "--voice", "nova", "--out", "hello.wav"],
    &["voice", "check-mark", "hello.wav"],
    &["scenario"],
    &["scenario", "save"],
    &["scenario", "list"],
    &["scenario", "show", "core/example"],
    &["scenario", "share", "core/example"],
    &["scenario", "submit", "core/example"],
    &["scenario", "run", "core/example"],
    &["scenario", "replay", "core/example"],
    &["scenario", "bless", "core/example"],
    &["scenario", "stale", "core/example", "--issue", "https://example.com/1", "--days", "7"],
    &["scenario", "check"],
    &["test", "--fleet", "--once", "--filter", "core", "--mode", "recorded"],
    &["test", "--fleet", "--nightly", "--mode", "live"],
    &["test", "--fleet", "--arm-tonight"],
    &["test", "--fleet", "--submit", "--push"],
    &["test", "--fleet", "--submit", "--file", "results.jsonl"],
    &["trace", "turn_1"],
    &["trace", "last"],
    &["logs"],
    &["incident"],
    &["incident", "list"],
    &["incident", "show"],
    &["incident", "capture"],
    &["incident", "pack"],
    &["record", "start"],
    &["record", "stop"],
    &["record", "list"],
    &["record", "trim"],
    &["replay", "rec_1"],
    &["replay", "rec_1", "--level", "core", "--mode", "strict"],
    &["replay", "rec_1", "--mode", "scenario"],
    &["debug", "stats"],
    &["debug", "tasks"],
    &["debug", "dump"],
    &["claude"],
    &["claude", "install"],
    &["claude", "uninstall"],
    &["claude", "update-window"],
    &["mcp-probe"],
    &["bench"],
];

#[test]
fn every_command_of_section_3_5_says_not_built_yet() {
    for args in COMMANDS {
        let line = args.join(" ");
        let cli = Cli::try_parse_from(std::iter::once("eileen").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("`eileen {line}` does not parse: {error}"));
        let Err(error) = run(cli) else { panic!("`eileen {line}` should not be built yet") };
        let message = error.to_string();
        assert!(message.contains("is not built yet"), "`eileen {line}` said: {message}");
    }
}

#[test]
fn an_unknown_command_is_refused() {
    assert!(Cli::try_parse_from(["eileen", "dance"]).is_err(), "no such command");
}

#[test]
fn eileen_run_wires_every_component() {
    let names = |args: &RunArgs| -> Vec<&'static str> {
        wiring::components(args).iter().map(|component| component.name()).collect()
    };
    let mut expected = vec!["harness", "audio", "learn"];
    if cfg!(windows) {
        expected.push("shell");
    }
    assert_eq!(names(&RunArgs::default()), expected, "the components of a normal run");

    let replay = RunArgs { replay_models: Some("models".into()), ..RunArgs::default() };
    expected[0] = "responder";
    assert_eq!(names(&replay), expected, "the responder stands in for the harness");
}
