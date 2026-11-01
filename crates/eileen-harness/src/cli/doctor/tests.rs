use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use eileen_supervisor::{ChildClass, ChildSpec};

use super::{
    AUTH_STATUS_DEADLINE, AuthCheck, DoctorInputs, MsvcCheck, Probe, ProbeError, ProbeOutput,
    ProbeRunner, RunningCheck, SupervisedRunner, VersionCheck, doctor,
};
use crate::locate::tests::{Scratch, npm_layout, windows_env};
use crate::locate::{ClaudeVersion, LocateEnv, LocateError};

/// What a probe asked to run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Asked {
    program: PathBuf,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    env_clear: bool,
    piped: bool,
    class: ChildClass,
    deadline: Duration,
}

/// A runner that answers from a script and records every probe, starting nothing.
struct FakeRunner {
    answers: VecDeque<Result<ProbeOutput, ProbeError>>,
    asked: Vec<Asked>,
}

impl FakeRunner {
    fn new(answers: impl IntoIterator<Item = Result<ProbeOutput, ProbeError>>) -> Self {
        Self { answers: answers.into_iter().collect(), asked: Vec::new() }
    }

    fn args(&self, n: usize) -> Vec<String> {
        self.asked[n].args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect()
    }
}

impl ProbeRunner for FakeRunner {
    fn run(&mut self, probe: &Probe) -> Result<ProbeOutput, ProbeError> {
        let spec: &ChildSpec = &probe.spec;
        self.asked.push(Asked {
            program: spec.program.clone(),
            args: spec.args.clone(),
            env: spec.env.clone(),
            env_clear: spec.env_clear,
            piped: spec.piped,
            class: spec.class,
            deadline: probe.deadline,
        });
        self.answers
            .pop_front()
            .unwrap_or_else(|| Err(ProbeError::Spawn("no answer scripted".into())))
    }
}

/// A probe that exited with `code` and printed nothing.
fn exit(code: i32) -> ProbeOutput {
    printed(code, "")
}

/// A probe that exited with `code` and printed `stdout`.
fn printed(code: i32, stdout: &str) -> ProbeOutput {
    ProbeOutput { exit_code: Some(code), stdout: stdout.to_owned() }
}

fn version(text: &str) -> ClaudeVersion {
    ClaudeVersion::parse(text).unwrap()
}

/// Inputs whose `PATH` is the scratch folder's `npm`, with no vswhere and no running Eileen.
fn npm_inputs(scratch: &Scratch) -> DoctorInputs {
    DoctorInputs {
        locate: windows_env(&[&scratch.path().join("npm")]),
        host_vars: vec![("SystemRoot".into(), r"C:\Windows".into())],
        vswhere: None,
        running: None,
    }
}

#[test]
fn a_signed_out_cli_gets_the_login_suggestion_and_nothing_else_runs() {
    let scratch = Scratch::new();
    let (shim, exe) = npm_layout(&scratch, "2.1.304");
    let mut runner = FakeRunner::new([Ok(exit(super::SIGNED_OUT_EXIT))]);

    let report = doctor(&npm_inputs(&scratch), &mut runner).unwrap();

    assert_eq!(runner.asked.len(), 1, "only `claude auth status` ran: {:?}", runner.asked);
    assert_eq!(runner.asked[0].program, exe, "the real executable, not the shim");
    assert_eq!(runner.args(0), ["auth", "status"], "the sign-in check");
    assert_eq!(report.auth, AuthCheck::SignedOut, "the exit code says signed out");
    assert!(report.toolchains.is_none(), "no toolchain probe while signed out");
    assert!(!report.healthy(), "a signed-out CLI fails the check");
    let text = report.to_string();
    assert!(
        text.contains("Please run `claude auth login` yourself"),
        "the suggestion is printed: {text}"
    );
    assert!(text.contains(&shim.display().to_string()), "the npm shim is named: {text}");
    assert!(text.contains(&exe.display().to_string()), "and the executable behind it: {text}");
}

#[test]
fn a_version_under_the_floor_is_reported_below_it() {
    let scratch = Scratch::new();
    npm_layout(&scratch, "2.1.200");
    let mut runner = FakeRunner::new([Ok(exit(0))]);

    let report = doctor(&npm_inputs(&scratch), &mut runner).unwrap();

    assert_eq!(report.version, VersionCheck::BelowFloor(version("2.1.200")), "under 2.1.284");
    assert!(!report.healthy(), "a CLI under the floor fails the check");
    let text = report.to_string();
    assert!(text.contains("2.1.200, below the floor of 2.1.284"), "the floor is named: {text}");
}

#[test]
fn a_newer_version_than_the_tested_one_passes_with_a_note() {
    let scratch = Scratch::new();
    npm_layout(&scratch, "2.1.310");
    let mut runner = FakeRunner::new([Ok(exit(0))]);

    let report = doctor(&npm_inputs(&scratch), &mut runner).unwrap();

    assert_eq!(report.version, VersionCheck::NewerThanTested(version("2.1.310")), "past 2.1.304");
    assert!(report.healthy(), "a newer CLI is allowed");
    assert!(report.to_string().contains("newer than the tested 2.1.304"), "and said so");
}

#[test]
fn a_signed_in_cli_reports_the_toolchains_a_worker_sees() {
    let scratch = Scratch::new();
    npm_layout(&scratch, "2.1.304");
    let cargo = scratch.file("cargo/bin/cargo.exe", "MZ");
    let node = scratch.file("nodejs/node.exe", "MZ");
    let vswhere = scratch.file("Installer/vswhere.exe", "MZ");
    let inputs = DoctorInputs {
        locate: windows_env(&[
            &scratch.path().join("npm"),
            cargo.parent().unwrap(),
            node.parent().unwrap(),
        ]),
        vswhere: Some(vswhere.clone()),
        ..npm_inputs(&scratch)
    };
    let installation = printed(0, "C:\\VS\\2022\\BuildTools\r\n");
    let mut runner = FakeRunner::new([Ok(exit(0)), Ok(installation)]);

    let report = doctor(&inputs, &mut runner).unwrap();

    assert_eq!(report.version, VersionCheck::InRange(version("2.1.304")), "the tested release");
    assert_eq!(report.auth, AuthCheck::SignedIn, "exit code 0");
    assert!(report.healthy(), "nothing is wrong");
    let toolchains = report.toolchains.as_ref().unwrap();
    assert_eq!(toolchains.cargo, Some(cargo), "cargo on the worker's PATH");
    assert_eq!(toolchains.node, Some(node), "node on the worker's PATH");
    assert_eq!(
        toolchains.msvc,
        MsvcCheck::Installed(PathBuf::from("C:\\VS\\2022\\BuildTools")),
        "the installation vswhere names"
    );
    assert_eq!(runner.asked.len(), 2, "auth status, then vswhere");
    assert_eq!(runner.asked[1].program, vswhere, "vswhere from the Installer folder");
    assert!(runner.asked[1].piped, "its answer is read from stdout");
    assert!(
        runner.args(1).contains(&"Microsoft.VisualStudio.Component.VC.Tools.x86.x64".to_owned()),
        "it asks for the C++ build tools: {:?}",
        runner.args(1)
    );
    assert_eq!(runner.asked[1].class, ChildClass::Tool, "a short tool run");
}

#[test]
fn a_missing_cli_is_a_typed_error_and_nothing_runs() {
    let scratch = Scratch::new();
    let inputs = DoctorInputs { locate: windows_env(&[scratch.path()]), ..npm_inputs(&scratch) };
    let mut runner = FakeRunner::new([]);

    let found = doctor(&inputs, &mut runner);

    assert_eq!(
        found,
        Err(LocateError::NotFound { searched: vec![scratch.path().to_path_buf()] }),
        "no claude anywhere"
    );
    assert!(runner.asked.is_empty(), "nothing was started");
}

#[test]
fn auth_status_runs_in_the_worker_environment_with_a_deadline() {
    let scratch = Scratch::new();
    npm_layout(&scratch, "2.1.304");
    let inputs = DoctorInputs {
        host_vars: vec![
            ("SystemRoot".into(), r"C:\Windows".into()),
            ("ANTHROPIC_API_KEY".into(), "sk-test".into()),
            ("CLAUDE_CODE_ENTRYPOINT".into(), "claude-desktop".into()),
            ("GH_TOKEN".into(), "ghp-test".into()),
        ],
        ..npm_inputs(&scratch)
    };
    let mut runner = FakeRunner::new([Ok(exit(0))]);

    doctor(&inputs, &mut runner).unwrap();

    let asked = &runner.asked[0];
    assert!(asked.env_clear, "it starts from an empty environment");
    assert_eq!(
        asked.env,
        vec![(OsString::from("SystemRoot"), OsString::from(r"C:\Windows"))],
        "only what survives the worker denylist"
    );
    assert!(!asked.piped, "its output, which names the account, is never read");
    assert_eq!(asked.class, ChildClass::ClaudeSession, "below normal, like every Claude process");
    assert_eq!(asked.deadline, AUTH_STATUS_DEADLINE, "the wait has a deadline");
}

#[test]
fn an_auth_status_that_never_answers_is_reported_and_nothing_else_runs() {
    let scratch = Scratch::new();
    npm_layout(&scratch, "2.1.304");
    let mut runner = FakeRunner::new([Err(ProbeError::DeadlinePassed(AUTH_STATUS_DEADLINE))]);

    let report = doctor(&npm_inputs(&scratch), &mut runner).unwrap();

    assert_eq!(
        report.auth,
        AuthCheck::NoAnswer(ProbeError::DeadlinePassed(AUTH_STATUS_DEADLINE)),
        "the deadline passed"
    );
    assert!(!report.healthy(), "an unknown sign-in fails the check");
    assert_eq!(runner.asked.len(), 1, "nothing else ran");
    assert!(report.to_string().contains("no answer within 20 s"), "and said so");
}

#[test]
fn an_unknown_layout_version_is_asked_of_the_cli_once_signed_in() {
    let scratch = Scratch::new();
    scratch.file("home/.local/bin/claude.exe", "MZ");
    let inputs = DoctorInputs {
        locate: LocateEnv { home: Some(scratch.path().join("home")), ..windows_env(&[]) },
        ..npm_inputs(&scratch)
    };
    let mut runner = FakeRunner::new([Ok(exit(0)), Ok(printed(0, "2.1.304 (Claude Code)\n"))]);

    let report = doctor(&inputs, &mut runner).unwrap();

    assert_eq!(runner.args(1), ["--version"], "the CLI is asked");
    assert_eq!(report.version, VersionCheck::InRange(version("2.1.304")), "and its answer used");
    assert!(report.to_string().contains("the native installer"), "the layout is named");
}

#[test]
fn running_sessions_on_another_version_are_named() {
    let scratch = Scratch::new();
    npm_layout(&scratch, "2.1.304");
    let inputs = DoctorInputs {
        running: Some(vec![version("2.1.300"), version("2.1.304"), version("2.1.300")]),
        ..npm_inputs(&scratch)
    };
    let mut runner = FakeRunner::new([Ok(exit(0))]);

    let report = doctor(&inputs, &mut runner).unwrap();

    assert_eq!(report.running, RunningCheck::Differ(vec![version("2.1.300")]), "each once");
    assert!(
        report.to_string().contains("eileen claude update-window"),
        "the way to restart them is named"
    );
}

#[test]
fn the_supervised_runner_reports_a_program_that_cannot_start() {
    let scratch = Scratch::new();
    let runtime = eileen_rt::runtime::build("doctor-test", Some(1), 1).unwrap();
    let mut runner = SupervisedRunner::new(&runtime);
    let probe = Probe {
        spec: ChildSpec::new("claude.doctor", scratch.path().join("missing.exe")),
        deadline: Duration::from_secs(5),
    };

    let answer = runner.run(&probe);

    assert!(matches!(answer, Err(ProbeError::Spawn(_))), "a spawn error: {answer:?}");
}
