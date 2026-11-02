use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use eileen_contracts::harness::{JobBinding, WorkerCmd};
use eileen_contracts::ids::{JobId, SkillId, TraceCtx};
use eileen_contracts::jobs::JobKind;
use eileen_contracts::learn::SkillRef;
use eileen_trace::Redactor;
use serde_json::{Value, json};

use super::{ClaudeWorker, check_init, first_turn, user_line};
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
    let Line::Init(init) = parsed else { panic!("not an init: {parsed:?}") };
    *init
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
    let mut worker = ClaudeWorker::new(HarnessConfig::default());
    let spec = crate::session::tests::coding_worker();
    assert_eq!(at_once(worker.start(spec)).err(), Some(DriverError::NoExecutable), "no CLI");
}

#[test]
fn a_session_that_was_never_started_takes_no_job_and_no_turn() {
    let mut worker = ClaudeWorker::new(HarnessConfig {
        claude_exe: PathBuf::from("/opt/claude/claude"),
        ..HarnessConfig::default()
    });
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
