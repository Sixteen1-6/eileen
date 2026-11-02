//! The crate doc's trim invariant: `trim` keeps every input of its window, every setup input
//! before it and each kept hook input's answer, in their order, and drops the rest.

use eileen_contracts::config::SettingsEvent;
use eileen_contracts::core_io::{ActivityEvent, Input, UserInput};
use eileen_contracts::jobs::MemoryPressure;
use eileen_contracts::record::RecordLine;
use eileen_contracts::speech::VoiceEvent;
use eileen_record::trim::{is_setup, trim};
use serde_json::json;

use crate::support::{answer, input, pre_tool_use, typed};

fn memory(seq: u64) -> RecordLine {
    input(
        seq,
        Input::Memory(MemoryPressure {
            available_mb: 1800,
            commit_headroom_mb: 5600,
            paging_kb_s: 4000,
            paging_limit_kb_s: 5000,
            largest_process: None,
        }),
    )
}

fn mute(seq: u64) -> RecordLine {
    input(seq, Input::User(UserInput::SetMute(true)))
}

fn seqs(lines: &[RecordLine]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            RecordLine::Input(record) => record.seq.to_string(),
            RecordLine::HookAnswer(answer) => format!("answer to {}", answer.input),
        })
        .collect()
}

#[test]
fn trim_keeps_the_setup_inputs_before_the_window_and_drops_the_other_inputs() {
    let lines = vec![
        mute(1),
        typed(2, "what's on my calendar"),
        memory(3),
        input(4, Input::User(UserInput::SetMute(false))),
        typed(5, "open notepad"),
        memory(6),
        typed(7, "and the calculator"),
        mute(8),
    ];
    let kept = trim(&lines, &(5..=7));
    assert_eq!(seqs(&kept), ["1", "3", "4", "5", "6", "7"], "every setup input before the window");
    assert_eq!(kept[0], lines[0], "kept whole");
}

#[test]
fn trim_keeps_each_kept_hook_inputs_answer_wherever_it_sits() {
    let lines = vec![
        pre_tool_use(1, 1, json!({ "file_path": "before.md" })),
        answer(1, 1, 1_500_000),
        typed(2, "summarise the notes"),
        pre_tool_use(3, 2, json!({ "file_path": "notes.md" })),
        pre_tool_use(4, 3, json!({ "file_path": "todo.md" })),
        answer(4, 3, 4_200_000),
        typed(5, "thanks"),
        answer(3, 2, 6_100_000),
        pre_tool_use(6, 4, json!({ "file_path": "after.md" })),
        answer(6, 4, 6_500_000),
    ];
    let kept = trim(&lines, &(2..=5));
    assert_eq!(
        seqs(&kept),
        ["2", "3", "4", "answer to 4", "5", "answer to 3"],
        "the window's hooks with their answers, the answer after the window included; \
         the hooks outside the window and their answers dropped"
    );
}

#[test]
fn setup_inputs_set_standing_state_and_requests_are_not_setup() {
    let setup = [
        Input::User(UserInput::SetMute(true)),
        Input::User(UserInput::ToggleMute),
        Input::Activity(ActivityEvent::MicInUseElsewhere(true)),
        Input::Activity(ActivityEvent::SessionLocked(false)),
        Input::Settings(SettingsEvent::FileChanged { valid: true, error_line: None }),
        Input::Voice(VoiceEvent::Calibration(None)),
    ];
    for input in &setup {
        assert!(is_setup(input), "{input:?} is setup");
    }
    let not_setup = [
        Input::User(UserInput::SaveScenario),
        Input::User(UserInput::PauseDesktopJobs),
        Input::Activity(ActivityEvent::PhysicalInput),
        Input::Timer(eileen_contracts::core_io::TimerId(3)),
    ];
    for input in &not_setup {
        assert!(!is_setup(input), "{input:?} is not setup");
    }
}
