//! Every span and event name Eileen emits, with its allowed parents and its fields.
//!
//! Names are constants so grepping one finds every place that emits it, and low in
//! cardinality: IDs, tool names and paths go in fields, never in a name (observability report
//! section 2.4). [`ALL`] is the span-parent table [`crate::schema::check`] enforces; adding a
//! span means adding its constant and its row, and moving one under another parent without
//! updating the row fails the check.
//!
//! A field named `traceparent` on a span records a parent in another process (a W3C
//! `traceparent` from an MCP request's `_meta` or a speech frame).

/// Whether a name is a span or an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A span: it has a duration and children.
    Span,
    /// An event: a point in time inside a span, or at the root.
    Event,
}

/// Where a span may sit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Parent {
    /// No parent: a trace of its own.
    Root,
    /// This span, in the same process.
    Span(&'static str),
    /// This span, in another process of the same trace (through `traceparent`).
    Remote(&'static str),
    /// Anywhere (watchdog spans land under whatever was blocked).
    Any,
}

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameDef {
    /// The name.
    pub name: &'static str,
    /// Span or event.
    pub kind: Kind,
    /// The process that emits it.
    pub process: &'static str,
    /// Where it may sit.
    pub parents: &'static [Parent],
    /// A child that may outlive its parent (a job outlives the turn that started it).
    pub may_outlive_parent: bool,
    /// Its fields (`pii.` fields carry user content).
    pub fields: &'static [&'static str],
}

/// The name of the field that carries a remote parent's W3C `traceparent`.
pub const REMOTE_PARENT_FIELD: &str = "traceparent";

macro_rules! names {
    ($( $(#[$doc:meta])* $konst:ident = $name:literal, $kind:ident, $process:literal,
        parents: [$($parent:expr),* $(,)?], outlives: $outlives:literal, fields: [$($field:literal),* $(,)?]; )*) => {
        $( $(#[$doc])* pub const $konst: &str = $name; )*

        /// The whole table, in declaration order.
        pub const ALL: &[NameDef] = &[
            $( NameDef {
                name: $name,
                kind: Kind::$kind,
                process: $process,
                parents: &[$($parent),*],
                may_outlive_parent: $outlives,
                fields: &[$($field),*],
            }, )*
        ];
    };
}

use Parent::{Any, Remote, Root, Span};

names! {
    /// A user turn, the root of its trace.
    TURN = "turn", Span, "core", parents: [Root], outlives: false, fields: ["turn_id", "source", "outcome"];
    /// A stretch of speech the voice activity detector found, under the core's turn.
    SPEECH_SEGMENT = "speech.segment", Span, "speech", parents: [Remote(TURN)], outlives: false,
        fields: ["turn_id", "seg", "start_ms", "dur_ms", "vad_max"];
    /// End-of-turn detection.
    TURN_DETECT = "turn.detect", Span, "speech", parents: [Remote(TURN)], outlives: false,
        fields: ["silence_ms", "smart_turn_p", "decision"];
    /// Streaming recognition of a segment.
    STT_STREAM = "stt.stream", Span, "speech", parents: [Span(SPEECH_SEGMENT)], outlives: false,
        fields: ["model", "first_partial_ms", "final_ms", "pii.transcript"];
    /// Whether the turn was addressed to Eileen.
    ADDRESSEE = "addressee", Span, "core", parents: [Span(TURN)], outlives: false, fields: ["decision", "reason"];
    /// The route a turn takes.
    ROUTER_DECIDE = "router.decide", Span, "core", parents: [Span(TURN)], outlives: false,
        fields: ["route", "depth", "skill", "reason"];
    /// One brain turn.
    BRAIN_TURN = "brain.turn", Span, "core", parents: [Span(TURN)], outlives: false,
        fields: ["claude.session_id", "ttft_ms", "input_tokens", "output_tokens", "cache_read"];
    /// A deterministic fast-path command.
    FASTPATH_RUN = "fastpath.run", Span, "core", parents: [Span(TURN)], outlives: false, fields: ["command"];
    /// A job, which may outlive the turn that started it.
    JOB = "job", Span, "core", parents: [Span(TURN), Root], outlives: true, fields: ["job_id", "kind", "route", "origin_turn"];
    /// A pooled worker session starting, before any job exists.
    POOL_START = "pool.start", Span, "core", parents: [Root], outlives: false, fields: ["pool", "ws_id"];
    /// Binding a pooled session to a job (span links to the pool start and the job).
    POOL_BIND = "pool.bind", Span, "core", parents: [Span(JOB)], outlives: false, fields: ["ws_id", "job_id"];
    /// A model-free replay of a skill.
    REPLAY_RUN = "replay.run", Span, "core", parents: [Span(JOB)], outlives: false, fields: ["skill", "version"];
    /// One step of a replay.
    REPLAY_STEP = "replay.step", Span, "core", parents: [Span(REPLAY_RUN)], outlives: false, fields: ["step", "rung"];
    /// A worker session bound to a job.
    WORKER_SESSION = "worker.session", Span, "core", parents: [Span(JOB)], outlives: false,
        fields: ["route", "claude.session_id", "pid"];
    /// One tool use a worker reported.
    WORKER_TOOL_USE = "worker.tool_use", Span, "core", parents: [Span(WORKER_SESSION)], outlives: false,
        fields: ["tool_use_id", "tool"];
    /// One tool call through the dispatcher.
    DISPATCH_CALL = "dispatch.call", Span, "core",
        parents: [Span(WORKER_TOOL_USE), Span(BRAIN_TURN), Span(FASTPATH_RUN), Span(REPLAY_STEP)], outlives: false,
        fields: ["call_id", "tool", "origin", "tier", "decision"];
    /// Waiting for the user's approval.
    APPROVAL_WAIT = "approval.wait", Span, "core", parents: [Span(DISPATCH_CALL)], outlives: false,
        fields: ["tier", "answer", "wait_ms"];
    /// An MCP request to a sidecar.
    MCP_CALL = "mcp.call", Span, "core", parents: [Span(DISPATCH_CALL)], outlives: false,
        fields: ["otel.name", "mcp.method.name"];
    /// A capture in the desktop sidecar.
    DESKTOP_CAPTURE = "desktop.capture", Span, "desktop", parents: [Remote(MCP_CALL)], outlives: false,
        fields: ["target", "blob"];
    /// An action in the desktop sidecar.
    DESKTOP_ACTION = "desktop.action", Span, "desktop", parents: [Remote(MCP_CALL)], outlives: false,
        fields: ["action", "rung", "result"];
    /// A call in the apps sidecar.
    APPS_CALL = "apps.call", Span, "apps", parents: [Remote(MCP_CALL)], outlives: false,
        fields: ["target", "rung", "result"];
    /// Eileen speaking: a reply under its turn, narration under its job, or an announcement
    /// with neither (a reminder), which starts a trace of its own. The observability report's
    /// table lists only the first two; `docs/wp/WP0-foundation.md` records why the root stays.
    SPEAK = "speak", Span, "core", parents: [Span(TURN), Span(JOB), Root], outlives: false,
        fields: ["speak_id", "cause", "chars"];
    /// Synthesis in the speech process.
    TTS_SYNTH = "tts.synth", Span, "speech", parents: [Remote(SPEAK)], outlives: false,
        fields: ["voice", "first_audio_ms"];
    /// Playback of an utterance.
    AUDIO_PLAY = "audio.play", Span, "core", parents: [Span(SPEAK)], outlives: false,
        fields: ["played_samples", "heard_chars", "interrupted"];
    /// A barge-in.
    BARGEIN = "bargein", Span, "core", parents: [Span(SPEAK)], outlives: false, fields: ["detect_ms"];
    /// Recording a learning step.
    LEARN_RECORD = "learn.record", Span, "core", parents: [Span(JOB)], outlives: false, fields: ["job_id", "step"];
    /// A librarian run.
    LIBRARIAN_RUN = "librarian.run", Span, "core", parents: [Root, Any], outlives: false, fields: ["skills"];
    /// The supervisor restarting a child.
    SUPERVISOR_RESTART = "supervisor.restart", Span, "core", parents: [Root, Any], outlives: false,
        fields: ["child", "attempt", "delay_ms"];
    /// Writing an incident bundle.
    INCIDENT_CAPTURE = "incident.capture", Span, "core", parents: [Root, Any], outlives: false,
        fields: ["incident_id", "trigger"];
    /// The async runtime woke late; lands under whatever was blocked.
    RUNTIME_STALLED = "runtime.stalled", Span, "core", parents: [Root, Any], outlives: false, fields: ["late_ms"];

    /// A span took longer than its threshold (the slow-span layer).
    SPAN_SLOW = "span.slow", Event, "all", parents: [Any], outlives: false,
        fields: ["span", "dur_ms", "threshold_ms", "children", "slowest_child", "slowest_child_ms"];
    /// A soft assertion failed (`always!` or `never!`).
    SOFT_ASSERT_FAILED = "soft_assert.failed", Event, "all", parents: [Any], outlives: false,
        fields: ["file", "line", "message"];
    /// Tracing started without the `log` bridge: a `log` logger was set first (an
    /// `env_logger::init()`, say), and records logged through `log` go to it.
    LOG_BRIDGE_FAILED = "log_bridge.failed", Event, "all", parents: [Any], outlives: false, fields: ["error"];
    /// A task started through `eileen_rt::task` panicked; the message can quote anything.
    TASK_PANICKED = "task.panicked", Event, "all", parents: [Any], outlives: false, fields: ["task", "pii.message"];
    /// A bounded channel was full and a send was refused or had to wait.
    CHANNEL_FULL = "channel.full", Event, "all", parents: [Any], outlives: false,
        fields: ["channel", "capacity", "times"];
    /// The supervisor started a child.
    CHILD_SPAWNED = "child.spawned", Event, "all", parents: [Any], outlives: false, fields: ["child", "pid", "class"];
    /// A line a child wrote to stderr, which can quote paths and prompt text.
    CHILD_STDERR = "child.stderr", Event, "all", parents: [Any], outlives: false, fields: ["child", "pii.line"];
    /// A child exited.
    CHILD_EXITED = "child.exited", Event, "all", parents: [Any], outlives: false, fields: ["child", "code"];
    /// A child missed its heartbeat.
    HEARTBEAT_MISSED = "heartbeat.missed", Event, "all", parents: [Any], outlives: false, fields: ["child", "silent_ms"];
    /// A call to another process passed its deadline.
    DEADLINE_PASSED = "deadline.passed", Event, "all", parents: [Any], outlives: false, fields: ["call", "deadline_ms"];
}

/// The row for `name`, if it is declared.
#[must_use]
pub fn lookup(name: &str) -> Option<&'static NameDef> {
    ALL.iter().find(|def| def.name == name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{ALL, Kind, Parent, lookup};

    #[test]
    fn names_are_unique_lower_case_and_dotted() {
        let mut seen = BTreeSet::new();
        for def in ALL {
            assert!(seen.insert(def.name), "{} declared twice", def.name);
            assert!(
                def.name.bytes().all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_'),
                "{} is low cardinality and lower case",
                def.name
            );
            assert!(!def.parents.is_empty(), "{} has a parent rule", def.name);
        }
    }

    #[test]
    fn every_named_parent_is_a_declared_span() {
        for def in ALL {
            for parent in def.parents {
                if let Parent::Span(name) | Parent::Remote(name) = parent {
                    let parent_def =
                        lookup(name).unwrap_or_else(|| panic!("{name} is not declared"));
                    assert_eq!(parent_def.kind, Kind::Span, "{} is a span", parent_def.name);
                }
            }
        }
    }
}
