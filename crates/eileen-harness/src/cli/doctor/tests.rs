use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eileen_contracts::jobs::JobKind;
use eileen_supervisor::{ChildClass, ChildSpec};

use super::{
    AUTH_STATUS_DEADLINE, AuthCheck, BLOCKING_THREADS, DoctorInputs, MsvcCheck, PathSource, Probe,
    ProbeError, ProbeOutput, ProbeRunner, RunningCheck, SupervisedRunner, VERSION_DEADLINE,
    VSWHERE_DEADLINE, VersionCheck, doctor,
};
use crate::locate::tests::{Scratch, npm_layout};
use crate::locate::{ClaudeVersion, LocateError};
use crate::session::{HostEnv, worker_env};

/// What a probe asked to run.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Asked {
    program: PathBuf,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    env_clear: bool,
    piped: bool,
    raw_stderr: bool,
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
            raw_stderr: spec.raw_stderr,
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

/// `dirs` joined into a `PATH` value by this OS's rule, which the doctor splits by.
fn path_of(dirs: &[&Path]) -> String {
    std::env::join_paths(dirs).unwrap().into_string().unwrap()
}

/// A Windows host whose `PATH` is `dirs`, with `SystemRoot` and nothing read from the registry.
fn host(dirs: &[&Path]) -> HostEnv {
    HostEnv {
        vars: vec![("SystemRoot".into(), r"C:\Windows".into()), ("PATH".into(), path_of(dirs))],
        names_ignore_case: true,
        ..HostEnv::default()
    }
}

/// Inputs whose `PATH` is the scratch folder's `npm`, with no vswhere and no running Eileen.
fn npm_inputs(scratch: &Scratch) -> DoctorInputs {
    DoctorInputs {
        host: host(&[&scratch.path().join("npm")]),
        windows: true,
        vswhere: None,
        running: None,
    }
}

/// The environment a coding worker gets from `host`, as a spawn sets it.
fn worker_vars(host: &HostEnv) -> Vec<(OsString, OsString)> {
    worker_env(JobKind::Coding, host)
        .vars()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn a_signed_out_cli_gets_the_login_suggestion_and_nothing_else_runs() {
    let scratch = Scratch::new();
    let (shim, exe) = npm_layout(&scratch, "2.1.304");
    let mut runner = FakeRunner::new([Ok(exit(1))]);

    let report = doctor(&npm_inputs(&scratch), &mut runner).unwrap();

    assert_eq!(runner.asked.len(), 1, "only `claude auth status` ran: {:?}", runner.asked);
    assert_eq!(runner.asked[0].program, exe, "the real executable, not the shim");
    assert_eq!(runner.args(0), ["auth", "status"], "the sign-in check");
    assert_eq!(report.auth, AuthCheck::SignedOut { exit_code: 1 }, "a non-zero exit code");
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
fn any_non_zero_exit_of_auth_status_gets_the_login_suggestion() {
    let scratch = Scratch::new();
    npm_layout(&scratch, "2.1.304");
    let mut runner = FakeRunner::new([Ok(exit(2))]);

    let report = doctor(&npm_inputs(&scratch), &mut runner).unwrap();

    assert_eq!(report.auth, AuthCheck::SignedOut { exit_code: 2 }, "only 0 is signed in");
    assert_eq!(runner.asked.len(), 1, "nothing else ran");
    let text = report.to_string();
    assert!(text.contains("exited with code 2"), "the code is named: {text}");
    assert!(text.contains("Please run `claude auth login` yourself"), "and the suggestion: {text}");
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
        host: host(&[&scratch.path().join("npm"), cargo.parent().unwrap(), node.parent().unwrap()]),
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
    assert!(runner.asked[1].env_clear, "vswhere starts from an empty environment");
    assert_eq!(runner.asked[1].env, worker_vars(&inputs.host), "the worker's, as for the CLI");
    assert!(runner.asked[1].raw_stderr, "its standard error is drained, not logged");
    assert_eq!(runner.asked[1].deadline, VSWHERE_DEADLINE, "the wait has a deadline");
}

#[test]
fn the_toolchains_are_looked_for_on_the_path_a_worker_gets() {
    let scratch = Scratch::new();
    npm_layout(&scratch, "2.1.304");
    let shell_cargo = scratch.file("shell/cargo.exe", "MZ");
    let worker_cargo = scratch.file("home/.cargo/bin/cargo.exe", "MZ");
    let mut host = host(&[&scratch.path().join("npm"), shell_cargo.parent().unwrap()]);
    host.registry_path = Some(path_of(&[&scratch.path().join("npm")]));
    host.toolchain_dirs = vec![worker_cargo.parent().unwrap().display().to_string()];
    let inputs = DoctorInputs { host, ..npm_inputs(&scratch) };
    let mut runner = FakeRunner::new([Ok(exit(0))]);

    let report = doctor(&inputs, &mut runner).unwrap();

    let toolchains = report.toolchains.as_ref().unwrap();
    assert_eq!(
        toolchains.cargo,
        Some(worker_cargo),
        "a coding worker's PATH is the registry's with the toolchain folders in front, not this \
         shell's"
    );
    assert_eq!(report.path, PathSource::Registry, "and the report says where its PATH came from");
    assert!(report.to_string().contains("the registry's system and user PATH"), "in words");
}

#[test]
fn a_path_from_the_shell_is_named_as_such() {
    let scratch = Scratch::new();
    npm_layout(&scratch, "2.1.304");
    let mut runner = FakeRunner::new([Ok(exit(0))]);

    let report = doctor(&npm_inputs(&scratch), &mut runner).unwrap();

    assert_eq!(report.path, PathSource::Shell, "no registry PATH was read");
    assert!(report.to_string().contains("PATH:        this shell's"), "and the report says so");
}

#[test]
fn a_missing_cli_is_a_typed_error_and_nothing_runs() {
    let scratch = Scratch::new();
    let inputs = DoctorInputs { host: host(&[scratch.path()]), ..npm_inputs(&scratch) };
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
    let mut host = host(&[&scratch.path().join("npm")]);
    host.vars.extend([
        ("ANTHROPIC_API_KEY".into(), "sk-test".into()),
        ("CLAUDE_CODE_ENTRYPOINT".into(), "claude-desktop".into()),
        ("GH_TOKEN".into(), "ghp-test".into()),
    ]);
    let inputs = DoctorInputs { host, ..npm_inputs(&scratch) };
    let mut runner = FakeRunner::new([Ok(exit(0))]);

    doctor(&inputs, &mut runner).unwrap();

    let asked = &runner.asked[0];
    assert!(asked.env_clear, "it starts from an empty environment");
    assert_eq!(asked.env, worker_vars(&inputs.host), "exactly a coding worker's environment");
    for denied in ["ANTHROPIC_API_KEY", "CLAUDE_CODE_ENTRYPOINT", "GH_TOKEN"] {
        assert!(
            asked.env.iter().all(|(name, _)| name != denied),
            "{denied} is removed by the worker denylist"
        );
    }
    assert!(!asked.piped, "its output, which names the account, is never read");
    assert!(asked.raw_stderr, "nor its standard error forwarded into the logs");
    assert_eq!(asked.class, ChildClass::Tool, "a short tool run, not a session's lower priority");
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
    let mut host = host(&[]);
    host.vars.push(("USERPROFILE".into(), scratch.path().join("home").display().to_string()));
    let inputs = DoctorInputs { host, ..npm_inputs(&scratch) };
    let mut runner = FakeRunner::new([Ok(exit(0)), Ok(printed(0, "2.1.316 (Claude Code)\n"))]);

    let report = doctor(&inputs, &mut runner).unwrap();

    assert_eq!(runner.args(1), ["--version"], "the CLI is asked");
    let asked = &runner.asked[1];
    assert!(asked.piped && asked.raw_stderr, "its version is read and its stderr drained");
    assert!(asked.env_clear, "in the worker environment");
    assert_eq!(asked.class, ChildClass::Tool, "a short tool run");
    assert_eq!(asked.deadline, VERSION_DEADLINE, "with a deadline");
    assert_eq!(
        report.version,
        VersionCheck::NewerThanTested(version("2.1.316")),
        "and its answer used"
    );
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
    let runtime = eileen_rt::runtime::build("doctor-test", Some(1), BLOCKING_THREADS).unwrap();
    let mut runner = SupervisedRunner::new(&runtime);
    let probe = Probe {
        spec: ChildSpec::new("claude.doctor", scratch.path().join("missing.exe")),
        deadline: Duration::from_secs(5),
    };

    let answer = runner.run(&probe);

    assert!(matches!(answer, Err(ProbeError::Spawn(_))), "a spawn error: {answer:?}");
}

/// Set in the environment of the child [`probe_child_entry`] runs as.
const CHILD_SWITCH: &str = "EILEEN_DOCTOR_PROBE_CHILD";

/// A chatty probe: this test binary run again by
/// `the_supervised_runner_reads_a_real_child_to_its_exit`, with [`CHILD_SWITCH`] set. It writes
/// many times a pipe's buffer to standard error before it writes over 4 KB to standard output,
/// then fails, which ends the test binary with code 101. Without the switch it returns at once.
#[test]
fn probe_child_entry() {
    if std::env::var_os(CHILD_SWITCH).is_none() {
        return;
    }
    std::io::stderr().write_all(&vec![b'e'; 256 * 1024]).unwrap();
    std::io::stdout().write_all(&vec![b'o'; 16 * 1024]).unwrap();
    std::io::stdout().flush().unwrap();
    panic!("the probe child ends with a non-zero exit code");
}

#[test]
fn the_supervised_runner_reads_a_real_child_to_its_exit() {
    let runtime = eileen_rt::runtime::build("doctor-test", Some(1), BLOCKING_THREADS).unwrap();
    let mut runner = SupervisedRunner::new(&runtime);
    let mut spec = ChildSpec::new("claude.doctor", std::env::current_exe().unwrap())
        .args(["cli::doctor::tests::probe_child_entry", "--exact", "--nocapture", "--quiet"])
        .envs([(CHILD_SWITCH, "1")])
        .class(ChildClass::Tool);
    spec.raw_stderr = true;
    // Far above what the child needs, so only a stall reaches it.
    let probe = Probe { spec, deadline: Duration::from_secs(60) };

    let answer = runner.run(&probe).unwrap();

    assert_eq!(answer.exit_code, Some(101), "a failed test binary's exit code is read");
    assert_eq!(answer.stdout.len(), 4096, "standard output is kept up to 4 KB, the rest dropped");
    assert!(answer.stdout.contains('o'), "and holds what the child wrote: {:?}", answer.stdout);
}
