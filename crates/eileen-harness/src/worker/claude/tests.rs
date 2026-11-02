use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use eileen_contracts::harness::{
    BuildSlotTicket, JobBinding, SessionSpec, WorkerCmd, WorktreeSpec,
};
use eileen_contracts::ids::{JobId, Personal, SkillId, TraceCtx};
use eileen_contracts::jobs::JobKind;
use eileen_contracts::learn::{SkillRef, SnapshotManifest};
use eileen_trace::Redactor;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt as _, BufReader, duplex};

use super::{
    ClaudeWorker, Lines, Status, check_init, eileen_status, first_turn, reply_to, unsupported,
    user_line,
};
use crate::stream::{Init, Line, parse};
use crate::{DriverError, HarnessConfig, WorkerDriver as _};

/// The output of a future that answers without waiting, as every refusal before a spawn does.
fn at_once<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("a refusal answers at once"),
    }
}

/// The coding worker's spec without its build slot, which the driver does not hold yet.
fn coding_spec() -> SessionSpec {
    let mut spec = crate::session::tests::coding_worker();
    spec.build_slot = None;
    spec
}

fn binding() -> JobBinding {
    JobBinding {
        job: JobId::from_parts(1_700_000_000_000, 1),
        kind: JobKind::Coding,
        task: "Rename the build script".to_owned(),
        skill: None,
        inline_skill: None,
        facts: Vec::new(),
        trace: TraceCtx::default(),
    }
}

/// A `system/init` line with `eileen` at `status`, and `memory_paths` when `memory` is set.
fn init(status: &str, memory: Option<&str>) -> Init {
    let mut line = json!({
        "type": "system",
        "subtype": "init",
        "session_id": "a3b57c79-73b3-485d-8511-c5ae9b1cdac5",
        "model": "claude-haiku-5-5",
        "tools": ["Bash"],
        "mcp_servers": [{ "name": "eileen", "status": status }],
        "capabilities": ["interrupt_receipt_v1"],
    });
    if let (Some(path), Value::Object(fields)) = (memory, &mut line) {
        fields.insert("memory_paths".to_owned(), json!({ "auto": path }));
    }
    let parsed = parse(line.to_string().as_bytes(), &Redactor::new());
    let Line::Init(init) = &parsed else { panic!("not an init: {parsed:?}") };
    init.as_ref().clone()
}

#[test]
fn the_first_turn_is_the_task_alone_without_a_skill_or_facts() {
    assert_eq!(first_turn(&binding()), "Rename the build script", "the task as it is");
}

#[test]
fn the_first_turn_names_the_skill_carries_its_inline_text_and_the_facts_block() {
    let mut binding = binding();
    binding.skill = Some(SkillRef { id: SkillId::new("cargo-rename-bin").unwrap(), version: 3 });
    binding.inline_skill = Some("1. Edit Cargo.toml's [[bin]] name.".to_owned());
    binding.facts =
        vec!["The repository builds with cargo xtask".to_owned(), "Rust 1.95".to_owned()];
    assert_eq!(
        first_turn(&binding),
        "Rename the build script\n\n\
         Follow skill eileen-learned:cargo-rename-bin\n\n\
         1. Edit Cargo.toml's [[bin]] name.\n\n\
         Facts:\n\
         - The repository builds with cargo xtask\n\
         - Rust 1.95",
        "task, skill line, inline skill, facts"
    );
}

#[test]
fn a_user_line_has_the_shape_the_contract_test_wrote() {
    let line: Value = serde_json::from_str(&user_line("Write hi to hello.txt")).unwrap();
    assert_eq!(
        line,
        json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{ "type": "text", "text": "Write hi to hello.txt" }],
            },
            "parent_tool_use_id": null,
            "session_id": "",
        }),
        "worker-command.json's first_user_line"
    );
    assert!(!user_line("a\nb").contains('\n'), "one line, whatever the text holds");
}

#[test]
fn only_an_init_with_eileen_connected_and_no_memory_folder_may_be_bound() {
    assert_eq!(check_init(&init("connected", None)), Ok(()), "ready");
    for status in ["failed", "pending", "needs-auth"] {
        assert_eq!(
            check_init(&init(status, None)),
            Err(DriverError::EileenNotConnected),
            "eileen {status}"
        );
    }
    assert_eq!(
        check_init(&init("connected", Some("/data/eileen/memory/claude"))),
        Err(DriverError::MemoryPathReported),
        "any memory folder at all, while every session runs with memory off"
    );
}

#[test]
fn a_worker_without_a_located_cli_refuses_to_start() {
    let mut worker = ClaudeWorker::new(HarnessConfig::default(), Arc::new(Redactor::new()));
    let spec = coding_spec();
    assert_eq!(at_once(worker.start(spec)).err(), Some(DriverError::NoExecutable), "no CLI");
}

#[test]
fn a_session_that_was_never_started_takes_no_job_and_no_turn() {
    let cfg = HarnessConfig {
        claude_exe: PathBuf::from("/opt/claude/claude"),
        ..HarnessConfig::default()
    };
    let mut worker = ClaudeWorker::new(cfg, Arc::new(Redactor::new()));
    assert_eq!(at_once(worker.bind(binding())), Err(DriverError::NoSession), "no job");
    let prompt = WorkerCmd::Prompt { text: "go on".to_owned() };
    assert_eq!(at_once(worker.send(prompt)), Err(DriverError::NoSession), "no turn");
    assert_eq!(at_once(worker.send(WorkerCmd::Interrupt)), Err(DriverError::NoSession), "none");
    assert_eq!(at_once(worker.send(WorkerCmd::Cancel)), Ok(()), "cancel of nothing is done");
    assert_eq!(
        at_once(worker.send(WorkerCmd::Recycle)),
        Err(DriverError::Unsupported { command: "recycle" }),
        "the pool's commands come later"
    );
    assert!(at_once(worker.next_event()).is_none(), "no event");
}

#[test]
fn a_spec_asking_for_what_the_driver_does_not_carry_out_is_refused() {
    let refused = |change: fn(&mut SessionSpec)| {
        let mut spec = coding_spec();
        change(&mut spec);
        unsupported(&spec)
    };
    assert_eq!(refused(|_| {}), Ok(()), "the spec the core builds today");
    let expect_refused = |command| Err(DriverError::Unsupported { command });
    assert_eq!(
        refused(|spec| {
            spec.snapshot = Some(SnapshotManifest { session: spec.session, files: Vec::new() });
        }),
        expect_refused("a plugin snapshot check"),
        "the snapshot is not re-hashed or checked against init yet"
    );
    assert_eq!(
        refused(|spec| spec.snapshot_ref = Some("refs/eileen/1".to_owned())),
        expect_refused("a snapshot restore"),
        "no snapshot is restored yet"
    );
    assert_eq!(
        refused(|spec| {
            spec.worktree = Some(WorktreeSpec {
                repo: Personal::new("repo".to_owned()),
                path: Personal::new("wt".to_owned()),
                branch: "eileen/job".to_owned(),
                base: "main".to_owned(),
            });
        }),
        expect_refused("a worktree"),
        "no worktree is made yet, so the job would run in the run folder"
    );
    assert_eq!(
        refused(|spec| spec.build_slot = Some(BuildSlotTicket { slot: 0, cargo_jobs: 4 })),
        expect_refused("a build slot"),
        "no build slot is held yet"
    );
    assert_eq!(
        refused(|spec| spec.memory_limit_mb = Some(2048)),
        expect_refused("a job memory limit"),
        "no JOB_OBJECT_LIMIT_JOB_MEMORY yet"
    );
}

#[test]
fn a_spec_the_driver_cannot_carry_out_is_refused_before_a_spawn() {
    let cfg = HarnessConfig {
        claude_exe: PathBuf::from("/opt/claude/claude"),
        ..HarnessConfig::default()
    };
    let mut worker = ClaudeWorker::new(cfg, Arc::new(Redactor::new()));
    let mut spec = coding_spec();
    spec.memory_limit_mb = Some(2048);
    assert_eq!(
        at_once(worker.start(spec)).err(),
        Some(DriverError::Unsupported { command: "a job memory limit" }),
        "refused at once"
    );
}

#[test]
fn eileen_must_be_connected_in_mcp_status() {
    let status = |servers: Value| eileen_status(&json!({ "mcpServers": servers }));
    let eileen = |state: &str| json!([{ "name": "other", "status": "connected" }, { "name": "eileen", "status": state }]);
    assert_eq!(status(eileen("connected")), Status::Connected, "ready");
    assert_eq!(status(eileen("pending")), Status::Pending, "asked again");
    assert_eq!(status(json!([])), Status::Pending, "not listed yet: asked again");
    assert_eq!(eileen_status(&Value::Null), Status::Pending, "no list at all: asked again");
    for state in ["failed", "needs-auth", "disabled"] {
        assert_eq!(status(eileen(state)), Status::Refused, "eileen {state}");
    }
}

#[test]
fn only_the_reply_to_the_request_asked_is_taken() {
    let reply = |id: &str, subtype: &str| {
        json!({
            "type": "control_response",
            "response": { "subtype": subtype, "request_id": id, "response": { "mcpServers": [] } },
        })
        .to_string()
    };
    assert_eq!(
        reply_to(reply("eileen_ready_2", "success").as_bytes(), "eileen_ready_2"),
        Some(Ok(json!({ "mcpServers": [] }))),
        "its response"
    );
    assert_eq!(
        reply_to(reply("eileen_ready_2", "error").as_bytes(), "eileen_ready_2"),
        Some(Err(())),
        "an error reply"
    );
    let other = reply("eileen_ready_1", "success");
    assert_eq!(reply_to(other.as_bytes(), "eileen_ready_2"), None, "another request's reply");
    let init = br#"{"type":"system","subtype":"init"}"#;
    assert_eq!(reply_to(init, "eileen_ready_2"), None, "not a reply");
    assert_eq!(reply_to(b"not json", "eileen_ready_2"), None, "not JSON");
}

#[test]
fn a_read_dropped_partway_keeps_the_part_of_the_line_it_had_read() {
    let (mut writer, reader) = duplex(1024);
    let mut lines = Lines::new(BufReader::new(reader));
    let whole =
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"eileen_1"}}"#;
    let (head, tail) = whole.split_at(30);
    at_once(writer.write_all(head)).unwrap();
    // Dropped after one poll, as a `select!` drops it when another branch wins.
    let mut read = Box::pin(lines.next());
    let polled = read.as_mut().poll(&mut Context::from_waker(Waker::noop()));
    assert!(polled.is_pending(), "half a line is not a line");
    drop(read);
    at_once(writer.write_all(tail)).unwrap();
    at_once(writer.write_all(b"\r\n\n")).unwrap();
    let line = at_once(lines.next()).map(<[u8]>::to_vec);
    assert_eq!(line.as_deref(), Some(&whole[..]), "the whole line, once, without its ending");
    drop(writer);
    assert_eq!(at_once(lines.next()), None, "the blank line is passed over, then stdout ends");
}
