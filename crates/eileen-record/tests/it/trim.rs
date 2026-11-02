//! The crate doc's trim invariant: `trim` keeps every input of the chosen turns and jobs, the
//! latest setup input of each kind before the first of them, the unclaimed setup inputs among
//! them and each kept hook input's answer, in their order, and drops the rest.

use std::collections::BTreeSet;

use eileen_contracts::config::SettingsEvent;
use eileen_contracts::core_io::{ActivityEvent, Input, UserInput};
use eileen_contracts::jobs::MemoryPressure;
use eileen_contracts::record::RecordLine;
use eileen_contracts::speech::VoiceEvent;
use eileen_record::trim::{Selection, SetupKind, is_setup, setup_kind, trim};
use serde_json::json;

use crate::support::{answer, input, job, of, pre_tool_use, turn, typed};

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

fn mute(seq: u64, muted: bool) -> RecordLine {
    input(seq, Input::User(UserInput::SetMute(muted)))
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

fn turns(turns: &[u8]) -> Selection {
    Selection { turns: turns.iter().map(|n| turn(*n)).collect(), jobs: BTreeSet::new() }
}

#[test]
fn trim_keeps_the_latest_setup_input_of_each_kind_and_drops_the_other_inputs() {
    let lines = vec![
        mute(1, true),
        of(typed(2, "what's on my calendar"), Some(turn(1)), None),
        memory(3),
        mute(4, false),
        input(5, Input::User(UserInput::ToggleMute)),
        memory(6),
        of(typed(7, "open notepad"), Some(turn(2)), None),
        memory(8),
        of(typed(9, "and the calculator"), Some(turn(2)), None),
        mute(10, true),
    ];
    let kept = trim(&lines, &turns(&[2]));
    assert_eq!(
        seqs(&kept),
        ["4", "5", "6", "7", "8", "9"],
        "the latest SetMute and the toggle after it, the latest memory reading, the turn, and \
         the reading taken during it; the other turn, the older setup and what follows dropped"
    );
    assert_eq!(kept.get(3), lines.get(6), "kept whole");
}

#[test]
fn trim_keeps_a_turns_job_after_the_turn_and_drops_an_interleaved_turn() {
    let (asked, other, work) = (turn(1), turn(2), job(1));
    let lines = vec![
        of(typed(1, "summarise the notes"), Some(asked), None),
        of(typed(2, "what time is it"), Some(other), None),
        of(pre_tool_use(3, 1, json!({ "file_path": "notes.md" })), None, Some(work)),
        answer(3, 1, 3_500_000),
        of(typed(4, "the short version"), Some(asked), None),
        memory(5),
        of(typed(6, "thanks"), Some(other), None),
        of(pre_tool_use(7, 2, json!({ "file_path": "todo.md" })), None, Some(work)),
        answer(7, 2, 7_500_000),
    ];
    let selection = Selection { turns: BTreeSet::from([asked]), jobs: BTreeSet::from([work]) };
    assert_eq!(
        seqs(&trim(&lines, &selection)),
        ["1", "3", "answer to 3", "4", "5", "7", "answer to 7"],
        "the turn, its job's hooks and answers after the turn's last input, and the unclaimed \
         reading between them; the other turn dropped"
    );
}

#[test]
fn trim_keeps_each_kept_hook_inputs_answer_wherever_it_sits() {
    let (asked, background) = (Some(turn(1)), Some(job(9)));
    let lines = vec![
        of(pre_tool_use(1, 1, json!({ "file_path": "before.md" })), None, background),
        answer(1, 1, 1_500_000),
        of(typed(2, "summarise the notes"), asked, None),
        of(pre_tool_use(3, 2, json!({ "file_path": "notes.md" })), asked, None),
        of(pre_tool_use(4, 3, json!({ "file_path": "todo.md" })), asked, None),
        answer(4, 3, 4_200_000),
        of(typed(5, "thanks"), asked, None),
        answer(3, 2, 6_100_000),
        of(pre_tool_use(6, 4, json!({ "file_path": "after.md" })), None, background),
        answer(6, 4, 6_500_000),
    ];
    assert_eq!(
        seqs(&trim(&lines, &turns(&[1]))),
        ["2", "3", "4", "answer to 4", "5", "answer to 3"],
        "the turn's hooks with their answers, the answer after its last input included; another \
         job's hooks and their answers dropped"
    );
}

#[test]
fn trim_keeps_nothing_when_no_input_belongs_to_the_selection() {
    let lines = vec![mute(1, true), of(typed(2, "open notepad"), Some(turn(1)), None)];
    assert!(trim(&lines, &turns(&[2])).is_empty(), "no turn 2");
}

#[test]
fn setup_inputs_set_standing_state_and_requests_are_not_setup() {
    let setup = [
        (Input::User(UserInput::SetMute(true)), SetupKind::Mute),
        (Input::User(UserInput::ToggleMute), SetupKind::Mute),
        (Input::Activity(ActivityEvent::MicInUseElsewhere(true)), SetupKind::MicInUseElsewhere),
        (Input::Activity(ActivityEvent::SessionLocked(false)), SetupKind::SessionLocked),
        (
            Input::Settings(SettingsEvent::FileChanged { valid: true, error_line: None }),
            SetupKind::Settings,
        ),
        (Input::Voice(VoiceEvent::Calibration(None)), SetupKind::Calibration),
    ];
    for (input, kind) in &setup {
        assert!(is_setup(input), "{input:?} is setup");
        assert_eq!(setup_kind(input), Some(*kind), "{input:?}");
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
