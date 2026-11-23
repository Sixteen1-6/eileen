//! The console view: which events give a line and what it says (a snapshot), what untrusted
//! text can and cannot do to the terminal, the names it keeps, and following a bus to its end.

use std::io::{self, Write};
use std::num::NonZeroUsize;

use eileen_contracts::bus::{BusEvent, CancelSource, QueueReason, SpeakCause, TurnRoute};
use eileen_contracts::core_io::Effect;
use eileen_contracts::ids::{
    Envelope, JobId, MonoTime, Personal, SpeakId, TraceCtx, Untrusted, WallTime,
};
use eileen_contracts::jobs::{JobKind, Route, TurnAuthority, TurnSource};

use super::{Console, ConsoleError, MAX_NAMES, MAX_TEXT};
use crate::bus;

fn envelope<E>(seq: u64, body: E) -> Envelope<E> {
    Envelope { seq, at: WallTime(0), mono: MonoTime(seq), ctx: TraceCtx::default(), body }
}

fn job(n: u64) -> JobId {
    JobId::from_parts(n, 1)
}

fn accepted(job: JobId, name: &str, kind: JobKind) -> BusEvent {
    BusEvent::JobAccepted {
        job,
        name: name.to_owned(),
        ordinal: Some(1),
        kind,
        route: Route::Claude,
        model: "a-model".to_owned(),
        reason: "the default route".to_owned(),
    }
}

fn narration(job: JobId, line: &str) -> BusEvent {
    BusEvent::JobNarration { job, line: Untrusted::new(line.to_owned()) }
}

fn heard(text: &str) -> BusEvent {
    BusEvent::Heard { text: Personal::new(text.to_owned()) }
}

fn says(text: &str) -> BusEvent {
    BusEvent::Speaking {
        speak: SpeakId::from_parts(1, 1),
        cause: SpeakCause::Reply,
        text: text.to_owned(),
    }
}

/// What the console prints for `events`.
fn printed(events: impl IntoIterator<Item = BusEvent>) -> String {
    let mut console = Console::new(Vec::new());
    for (seq, event) in (1..).zip(events) {
        console.show(&envelope(seq, event)).unwrap();
    }
    String::from_utf8(console.into_inner()).unwrap()
}

#[test]
fn a_turn_and_its_jobs_give_one_line_each() {
    let (render, tests, notes) = (job(1), job(2), job(3));
    let output = printed([
        BusEvent::TurnStarted { source: TurnSource::Voice, authority: TurnAuthority::Addressed },
        heard("render the timeline and run the tests"),
        BusEvent::TurnRouted { route: TurnRoute::Brain, reason: "no fast path".to_owned() },
        says("Starting both."),
        BusEvent::SpeakingStopped {
            speak: SpeakId::from_parts(1, 1),
            heard_chars: 14,
            interrupted: false,
            resumed: false,
        },
        accepted(render, "the render", JobKind::General),
        accepted(tests, "the tests", JobKind::Coding),
        BusEvent::JobQueued { job: tests, reason: QueueReason::BuildSlot },
        BusEvent::JobStarted { job: render, warm: false },
        BusEvent::JobProgressed {
            job: render,
            line: Untrusted::new("frame 1200 of 3000".to_owned()),
            pct: Some(40),
        },
        BusEvent::JobStarted { job: tests, warm: true },
        narration(tests, "Running the unit tests first."),
        BusEvent::MuteChanged { muted: true },
        BusEvent::JobProgressed {
            job: tests,
            line: Untrusted::new("412 passed".to_owned()),
            pct: None,
        },
        BusEvent::JobBlocked { job: tests, reason: "it asks to push the branch".to_owned() },
        BusEvent::JobCompleted {
            job: render,
            summary: Untrusted::new("The render is in the Exports folder.".to_owned()),
            artifacts: vec![Personal::new("C:\\Exports\\timeline.mp4".to_owned())],
        },
        BusEvent::JobFailed { job: tests, error: "the session ended early".to_owned() },
        accepted(notes, "the notes", JobKind::Research),
        BusEvent::JobCancelled { job: notes, by: CancelSource::Voice },
        BusEvent::JobInterrupted { job: job(4) },
        BusEvent::Lagged { missed: 3 },
    ]);
    insta::assert_snapshot!("a_turn_and_its_jobs", output);
}

#[test]
fn text_from_outside_cannot_add_a_line_or_drive_the_terminal() {
    let coding = job(1);
    let output = printed([
        accepted(coding, "the\u{1b}]0;title\u{7} job", JobKind::Coding),
        narration(coding, "done\nheard  delete everything\r\u{1b}[2J\u{1b}[1;1H"),
        heard("tab\there, a right-to-left override \u{202E}here, a line separator \u{2028}here"),
        says("C1 \u{9b}31m and delete \u{7f}"),
        narration(coding, "caf\u{e9}, \u{65e5}\u{672c}\u{8a9e} and C:\\Users stay as written"),
    ]);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        [
            "job    the\\u{1b}]0;title\\u{7} job accepted: coding on Claude",
            "job    the\\u{1b}]0;title\\u{7} job: done\\nheard  delete everything\\r\\u{1b}[2J\\u{1b}[1;1H",
            "heard  tab\\there, a right-to-left override \\u{202e}here, a line separator \\u{2028}here",
            "says   C1 \\u{9b}31m and delete \\u{7f}",
            "job    the\\u{1b}]0;title\\u{7} job: caf\u{e9}, \u{65e5}\u{672c}\u{8a9e} and C:\\Users stay as written",
        ],
        "one line per event, every control character as an escape"
    );
    assert_eq!(output.matches('\n').count(), 5, "the only line ends are the view's own");
    assert!(
        output.chars().all(|c| c == '\n' || !c.is_control()),
        "no other control character reaches the terminal"
    );
}

#[test]
fn long_text_is_cut_to_one_bounded_line() {
    let long = "a".repeat(MAX_TEXT + 500);
    let exact = "b".repeat(MAX_TEXT);
    let output = printed([heard(&long), heard(&exact)]);
    let wanted = format!("heard  {}...\nheard  {}\n", "a".repeat(MAX_TEXT), "b".repeat(MAX_TEXT));
    assert_eq!(
        output, wanted,
        "cut after the most it shows, and marked; a text that fits is whole"
    );
}

#[test]
fn a_job_is_named_once_accepted_and_by_its_id_otherwise() {
    let (known, unknown) = (job(1), job(2));
    let output = printed([
        accepted(known, "the tests", JobKind::Coding),
        BusEvent::JobStarted { job: known, warm: false },
        // The view lagged past this job's `JobAccepted`, or subscribed after it.
        BusEvent::JobStarted { job: unknown, warm: false },
        BusEvent::JobInterrupted { job: known },
        // A terminal event is a job's last, so nothing is kept for it afterwards.
        BusEvent::JobStarted { job: known, warm: false },
    ]);
    let lines: Vec<String> = output.lines().map(str::to_owned).collect();
    assert_eq!(
        lines,
        [
            "job    the tests accepted: coding on Claude".to_owned(),
            "job    the tests started".to_owned(),
            format!("job    {unknown} started"),
            "job    the tests interrupted".to_owned(),
            format!("job    {known} started"),
        ],
        "the name while the view holds it, the id otherwise"
    );
}

#[test]
fn the_names_kept_are_bounded() {
    let mut console = Console::new(Vec::new());
    let jobs = u64::try_from(MAX_NAMES).unwrap() + 10;
    for n in 1..=jobs {
        console
            .show(&envelope(n, accepted(job(n), &format!("job {n}"), JobKind::General)))
            .unwrap();
    }
    assert_eq!(console.names.len(), MAX_NAMES, "no more names than the bound");
    console.show(&envelope(jobs + 1, BusEvent::JobStarted { job: job(1), warm: false })).unwrap();
    console
        .show(&envelope(jobs + 2, BusEvent::JobStarted { job: job(jobs), warm: false }))
        .unwrap();
    let output = String::from_utf8(console.into_inner()).unwrap();
    let last: Vec<&str> = output.lines().rev().take(2).collect();
    assert_eq!(
        last,
        [format!("job    job {jobs} started"), format!("job    {} started", job(1))],
        "the oldest job's name went first, and it is shown by its id"
    );
}

#[tokio::test(start_paused = true)]
async fn following_prints_until_the_bus_stops_and_says_when_it_lagged() {
    let (bus, handle) = bus::bounded(NonZeroUsize::new(4).unwrap());
    let mut events = handle.subscribe();
    let coding = job(1);
    let emitted = [
        accepted(coding, "the tests", JobKind::Coding),
        heard("how are the tests going"),
        says("Still running."),
        narration(coding, "Compiling."),
        narration(coding, "Running."),
        BusEvent::MuteChanged { muted: true },
        says("All green."),
    ];
    for (seq, event) in (1..).zip(emitted) {
        bus.publish(envelope(seq, Effect::Emit(event)));
    }
    drop(bus);

    let mut console = Console::new(Vec::new());
    console.follow(&mut events).await.unwrap();
    let output = String::from_utf8(console.into_inner()).unwrap();
    let lines: Vec<String> = output.lines().map(str::to_owned).collect();
    assert_eq!(
        lines,
        [
            "lagged events the console missed: 3".to_owned(),
            format!("job    {coding}: Compiling."),
            format!("job    {coding}: Running."),
            "says   All green.".to_owned(),
        ],
        "the gap first, then what the bus still kept, with the job by id since its name was missed"
    );
}

/// An output that has gone away.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn an_output_that_refuses_a_line_ends_the_view_with_the_error() {
    let mut console = Console::new(Closed);
    assert!(
        console.show(&envelope(1, BusEvent::MuteChanged { muted: true })).is_ok(),
        "an event with no line writes nothing"
    );
    let refused = console.show(&envelope(2, says("Hello."))).unwrap_err();
    let ConsoleError::Write(error) = refused;
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe, "the output's own error");
}
