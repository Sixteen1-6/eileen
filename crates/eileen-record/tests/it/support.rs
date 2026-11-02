//! What the integration tests share: a scratch folder and recorded lines.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use eileen_contracts::core_io::{Input, UserInput};
use eileen_contracts::harness::{HookDecision, HookReply, HookRequest};
use eileen_contracts::ids::{MonoTime, Personal, TraceCtx, Untrusted, WallTime, WorkerSessionId};
use eileen_contracts::record::{HookAnswerRecord, InputRecord, RecordLine};
use serde_json::Value;

/// A fresh folder of the temporary directory, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(name: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("eileen-record-it-{name}-{}-{n}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

/// The worker session the hook lines name.
pub(crate) fn session() -> WorkerSessionId {
    "ws_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap()
}

/// An input line numbered `seq`, a millisecond apart from the one before.
pub(crate) fn input(seq: u64, input: Input) -> RecordLine {
    RecordLine::Input(Box::new(InputRecord {
        seq,
        mono: MonoTime(seq * 1_000_000),
        at: WallTime(1_792_627_199_000_000 + i64::try_from(seq).unwrap() * 1_000),
        ctx: TraceCtx::default(),
        input,
    }))
}

/// Text typed into Eileen's own window.
pub(crate) fn typed(seq: u64, text: &str) -> RecordLine {
    input(seq, Input::User(UserInput::Typed { text: Personal::new(text.to_owned()) }))
}

/// A worker's `PreToolUse` hook for `request`.
pub(crate) fn pre_tool_use(seq: u64, request: u64, tool_input: Value) -> RecordLine {
    input(
        seq,
        Input::Hook(
            session(),
            HookRequest::PreToolUse {
                request,
                tool_name: "Read".to_owned(),
                tool_input: Untrusted::new(tool_input),
                tool_use_id: format!("toolu_{request}"),
                paths: Vec::new(),
                git: Vec::new(),
                urls: Vec::new(),
            },
        ),
    )
}

/// The core's answer to the hook input numbered `input`.
pub(crate) fn answer(input: u64, request: u64, mono: u64) -> RecordLine {
    RecordLine::HookAnswer(HookAnswerRecord {
        input,
        mono: MonoTime(mono),
        reply: HookReply::PreToolUse { request, decision: HookDecision::None },
    })
}
