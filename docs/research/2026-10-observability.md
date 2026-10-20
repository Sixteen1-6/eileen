# Tracing and debugging Eileen: request IDs, a flight recorder, replay and code maps

Research pass, written 2026-10-20. Question: how should Eileen be built so that a bug can be followed from the symptom ("it did not hear me", "it clicked the wrong button", "a job hung") to the span, the log line, the recording and the line of code, quickly, by the owner, by contributors and by AI agents working on the codebase?

This serves the "Engineered extremely well from the ground up" entry of [OWNER-DIRECTION.md](../OWNER-DIRECTION.md), in particular its last item: one request ID that follows a request through voice, brain, job, worker, tool call and result; structured logs and spans; a flight recorder that can be dumped when something goes wrong; replay of a recorded session; a generated code map kept current by CI.

It builds on, and does not repeat:

- [2026-10-rust-architecture.md](2026-10-rust-architecture.md): workspace lints, clippy pedantic, cargo-deny, nextest, `cargo xtask ci`, CI on `windows-latest`, the process model (core plus supervised sidecars in kill-on-close Job Objects), `panic = "unwind"` in the core, the build-slot rules for this 16 GB PC, and the already chosen `tracing` 0.1.44, `tracing-subscriber` 0.3.23 and `tracing-appender` 0.2.5.
- [2026-10-learning-loop.md](2026-10-learning-loop.md): the trajectory store, the `Redactor`, the dispatcher as the authoritative log of Eileen's own tool calls.
- [2026-10-voice-stack.md](2026-10-voice-stack.md), [2026-10-computer-use.md](2026-10-computer-use.md) and [2026-10-claude-mods.md](2026-10-claude-mods.md): the voice pipeline, the desktop service with one stdio MCP shim per worker, and per-job worker settings with HTTP hooks that carry `EILEEN_JOB_ID`.
- The engineering-practice report (`2026-10-engineering-practice.md`, being written in parallel) owns the test pyramid and the deterministic simulation harness. This report only states what the harness must expose so that recordings can drive it.

## How to read this

- **[primary]**: read on 2026-10-20 from a project's source, registry metadata, documentation or release page.
- **[inferred]**: reasoning from the above, not measured. Verify before relying on it.
- **[ran 10-xx]**: a measurement quoted from an earlier report, with its date.

Nothing was built or run on the PC for this pass: free memory was about 2 GB, and the rule for this phase is no builds and no tool installs. Every overhead figure below is therefore an estimate to measure (section 12).

## 1. Verdict

1. **Typed, prefixed ULIDs for everything the user or the system starts**: `turn_` (the request ID: one per user turn, voice or not), `job_`, `call_` (one tool call), `speak_` (one stretch of Eileen speaking), `inc_` (an incident), `rec_` (a recording), `sess_` (one run of `eileen.exe`). A prefix makes every ID greppable and self-describing in logs, in the HUD and in an agent's context [inferred; the `ulid` 3.0.0 crate is already in the workspace].
2. **Carry correlation explicitly, not only through `tracing` span context.** Span context does not cross a `tokio::sync::broadcast` channel, a named pipe or a process. So a small `TraceCtx { turn, job, call }` lives in `eileen-contracts` and rides on every bus event envelope, every IPC frame header, every MCP request (`_meta.traceparent`, per the Final MCP SEP-414, which rmcp 3.6.0 already implements) and every worker's environment (`TRACEPARENT`, which `claude -p` reads at start) [primary].
3. **One trace per request.** The W3C trace ID is the 128 bits of the `turn_` ULID, so any process can attach to the right trace without asking the core. A job is a child span of the turn that started it and may outlive it; later turns about that job link to it [inferred].
4. **Span and event names are static constants in one module (`eileen_trace::names`), user content only travels in fields whose name starts with `pii.`, and a span-parent table is checked in tests and against real incident bundles.** The `pii` naming rule and the schema checker are copied from LiveKit Agents, the clearest open-source voice agent tracing design read for this report [primary].
5. **Three sinks are always on, and each has its own filter**: JSON lines files at INFO (`%LOCALAPPDATA%\eileen\logs`, daily rotation, size-capped by Eileen's own janitor), an in-memory flight recorder at DEBUG, and, when started from a terminal, a human console at WARN. OpenTelemetry export (to a local viewer only), ETW and `tokio-console` are opt-in features that the PR lane never compiles [inferred].
6. **The flight recorder holds the last 10 minutes (32 MB cap in the core, 8 MB per sidecar) in two lanes**: a diagnostic lane (DEBUG spans and events) and an input lane (everything that entered the core through a port, plus a state checkpoint every minute). Because the input lane is in every incident, every incident is replayable. Audio is a third lane, off by default [inferred; the Go 1.25 flight recorder is the reference design, primary].
7. **Incidents are written on a panic, a crash, a watchdog trip, a supervisor kill, a spoken "Eileen, something went wrong", a hotkey or a tray click**, into `%LOCALAPPDATA%\eileen\incidents\<time>_inc_<ulid>\` with a readable `manifest.toml`, the merged rings of every process, a Perfetto-ready timeline, the last DesktopState, config with secrets removed and, where relevant, minidumps [inferred].
8. **A tiny `eileen-guard.exe` writes every minidump.** It runs outside the core's Job Object, serves Embark's `minidumper` protocol to every Eileen process, dumps a crashed process from outside (the reliable way), dumps a hung core or a hung sidecar without killing it first, and restarts the core under a rate limit. Embark's `crash-handler` 0.8.1 and `minidumper` 0.11.0 (MIT OR Apache-2.0) are the crates; Zed ships the same pattern [primary].
9. **Record and replay is built on a closed set of inputs ("ports").** Every source of nondeterminism (clock, IDs, randomness, speech events, brain output, worker output, tool results, desktop events, user input) enters the core through a port. Recording is the log of what the ports returned; replay feeds it back into the deterministic simulation harness and reports the first divergence. This is rust-analyzer's "closed event enum" principle and the idea behind Temporal's replay tests [primary].
10. **No duplicate storage.** DesktopState snapshots, screenshots and large tool results go once into a content-addressed blob store (`blobs\`, BLAKE3 keys) that both recordings and learning trajectories reference; a trajectory step and a recorded tool result share a `call_` ID [inferred].
11. **Code maps: CI generates and checks `docs/CODEMAP.md` (a Mermaid crate and process graph plus a crate table) on every PR with `cargo xtask codemap --check`, and generated references for spans, events and MCP tools are checked by tests.** Nightly CI adds module trees from `cargo-modules` and `cargo build --timings`. SCIP and LSIF are not worth generating in v0.1 (no local consumer; the `scip` CLI has no Windows build) [primary].
12. **Eileen's own Claude workers, when working on Eileen's code, get rust-analyzer through Claude's official LSP plugin mechanism, but from a repo-local `eileen-dev` plugin with memory-safe settings** (`checkOnSave` off, a separate target directory), an `eileen-debug` MCP server that reads traces, incidents and recordings, and Markdown skills for debugging and tracing conventions. `codegraph` (MIT) is an optional low-memory supplement. Serena (now GPL-3.0-or-later), GitNexus (PolyForm Noncommercial) and Sourcegraph (needs a server) are not recommended [primary for licenses and features].

## 2. One request ID, end to end

### 2.1 Identifiers

All are ULIDs (time-sortable, 128 bits) wrapped in newtypes in `eileen-contracts::ids`, with `Display` and `FromStr` that add and check the prefix [inferred].

| ID | Minted by | When | Lives on |
|---|---|---|---|
| `sess_` | core | process start | every log line of the run (a resource field) |
| `turn_` (the request ID) | `eileen-speech` for voice, at the first speech onset after the previous end of turn; the core for HUD clicks, hotkeys, dictation commands, phone messages and connector requests | a user turn begins | every span and event of the turn, every job it starts (`origin_turn`), the brain turn, the reply |
| `job_` | core job registry | a job is created | job, worker, tool calls, trajectory, HUD row |
| `call_` | core dispatcher | each tool call from any origin (brain, worker, replay, fast path) | dispatcher span, MCP request `_meta`, trajectory step, recorded tool result |
| `speak_` | core | Eileen starts speaking (reply, narration or announcement) | TTS request, playback, barge-in |
| `inc_`, `rec_` | core or guard | an incident or a recording starts | bundle directory, manifest |

A turn that Eileen decides was not addressed to it still gets a `turn_` ID, so "it ignored me" can be traced (section 7). A turn may span several VAD segments when the user pauses mid-sentence; the speech process owns end-of-turn detection, so it owns the boundary.

### 2.2 `TraceCtx`: correlation that crosses channels and processes

`tracing` spans give each process a context, but only within the task that entered them. Eileen moves work across `broadcast` channels, actor tasks, a named pipe to the speech process, stdio MCP to sidecars and child processes for workers. So correlation is carried as data:

```rust
// eileen-contracts/src/trace.rs
/// Correlation carried by every bus event, IPC frame and tool call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TraceCtx {
    pub turn: Option<TurnId>,
    pub job: Option<JobId>,
    pub call: Option<CallId>,
}

/// Every bus event is wrapped in this envelope.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Envelope<E> {
    pub seq: u64,          // core-wide sequence number, the replay order
    pub at_us: i64,        // wall clock, microseconds since the Unix epoch, UTC
    pub ctx: TraceCtx,
    pub event: E,
}
```

Rules [inferred]:

- A subscriber that handles an event opens its span with the envelope's IDs as fields (`turn_id`, `job_id`, `call_id`), so every log line it writes carries them.
- IPC frames to and from `eileen-speech` carry `TraceCtx` in their header. MCP requests from the core carry `_meta.traceparent` (and the `TraceCtx` IDs under an `eileen/` key); the sidecar's request handler opens its span with them. rmcp 3.6.0 has `set_traceparent` and `get_traceparent` on request metadata (added 2026-07-01, with a round-trip test) [primary].
- The traceparent is built without OpenTelemetry: version `00`, trace ID = the 32 hex digits of the turn ULID (or of the job ULID for a job with no turn), parent ID = a 64-bit span ID, flags `01`. A ULID is never all zeros, which W3C forbids [inferred; W3C Trace Context].
- Wall clock is enough to order events across Eileen's processes on one PC: Rust's `SystemTime::now` on Windows 8 and later uses the precise system time API. Within the core, `seq` is the order that replay uses [inferred].

### 2.3 The path of one spoken request

"Eileen, open Resolve and go to 15:30", from first sound to the spoken result.

| Stage | Process | Span (from `eileen_trace::names`) | Key fields | How the ID gets here |
|---|---|---|---|---|
| Speech onset | `eileen-speech` | `speech.segment` | `turn_id`, `seg`, `start_ms`, `dur_ms`, `vad_max` | minted here |
| Recognition | `eileen-speech` | `stt.stream` | `model`, `first_partial_ms`, `final_ms`, `pii.transcript` | same process |
| End of turn | `eileen-speech` | `turn.detect` | `silence_ms`, `smart_turn_p`, `decision` | same process |
| Turn opens in the core | core | `turn` (root) | `turn_id`, `source=voice`, `outcome` | frame header |
| Addressed or not | core | `addressee` | `decision`, `reason` (`name`, `follow_up`, `grammar`, `ambiguous`) | child of `turn` |
| Routing | core | `router.decide` | `route` (`fast_path`, `replay`, `brain`, `worker`), `depth`, `skill`, `reason` | child of `turn` |
| Brain turn | core | `brain.turn` | `claude.session_id`, `ttft_ms`, token counts, `cache_read` | child of `turn` |
| Job created | core | `job` | `job_id`, `kind`, `route`, `origin_turn` | child of `turn`, may outlive it |
| Worker | core | `worker.session` | `route=claude`, `claude.session_id`, `pid` | child of `job`; the worker gets `TRACEPARENT` and `EILEEN_JOB_ID` in its environment |
| Worker tool use | core | `worker.tool_use` | `tool_use_id`, `tool` | from stream-json `tool_use`, joined with the `PreToolUse` hook |
| Tool call | core | `dispatch.call` | `call_id`, `tool`, `origin`, `tier`, `decision` | minted here |
| Approval, if asked | core | `approval.wait` | `tier`, `answer`, `wait_ms` | child of `dispatch.call` |
| MCP to the sidecar | core | `mcp.call` | `otel.name = "tools/call resolve.goto"`, `mcp.method.name` | `_meta.traceparent` |
| App action | `eileen-apps` | `apps.call` | `target`, `rung`, `result` | remote parent from `_meta` |
| Result back | core | `dispatch.call` closes | `ok`, `result_bytes`, `blob` | |
| Learning | core | `learn.record` | `job_id`, `step` | child of `job` |
| Reply | core | `speak` | `speak_id`, `cause` (`reply`, `narration`, `announcement`), `chars` | child of `turn` or `job` |
| Synthesis | `eileen-speech` | `tts.synth` | `voice`, `first_audio_ms` | frame header |
| Playback and barge-in | core | `audio.play`, `bargein` | `played_samples`, `heard_chars`, `interrupted`, `detect_ms` | child of `speak` |

`eileen trace turn_01K8...` (section 8) stitches all of this from the files or from an incident, across processes, by these IDs.

### 2.4 Span design rules

These go into `crates/eileen-trace/CLAUDE.md` and into the `tracing-conventions` skill (section 6.5) [inferred unless marked]:

1. **Names are constants.** Every span and event name is a `pub const` in `eileen_trace::names`, with a doc comment, its allowed parents and its fields. Grepping the constant finds every place that emits it. A generated reference (`docs/reference/telemetry.md`) lists them.
2. **Names are low cardinality.** IDs, tool names and paths go in fields, never in a span name or a message. LiveKit writes the same rule down: content must never be embedded "in span names, event names, or log message bodies", because those cannot be redacted [primary].
3. **User content only in `pii.` fields.** Transcripts, window titles, file paths, selected text, typed text and URLs are recorded as `pii.transcript`, `pii.title` and so on. Each sink decides what to do with `pii.` fields (section 3.9). LiveKit marks the same class of attribute with a `pii` segment for the same reason [primary].
4. **Every spawned task is instrumented.** `tokio::spawn` is added to `clippy.toml`'s `disallowed-methods`, with the one allowed wrapper `eileen_core::task::spawn(name, future)` that attaches the current span, registers the task with `await-tree` (section 3.7) and logs a panic as `task.panicked` with the task name. Without this, work started inside a turn writes log lines that carry no `turn_id`.
5. **No `tracing` in the audio callback** (section 3.8).
6. **Errors carry context as fields.** Errors are logged once, where they are handled, as `error = %e` plus a `kind` field from the error enum. Internal errors capture a `tracing_error::SpanTrace` so a failure deep in a sidecar call shows the turn, job and call it belongs to.
7. **Slow-span logging is always on.** A small layer records the duration of every span on close; when it passes a per-name threshold from config (for example `brain.turn` 3,000 ms, `dispatch.call` 2,000 ms, `stt.stream` final 800 ms), it writes a `span.slow` INFO event with the span's children summarized. rust-analyzer's `hprof` (`RA_PROFILE='*>50'`) is the model; its authors call it "cheap enough to enable in production" [primary].
8. **OpenTelemetry names are fields, not our names.** Where a span maps to an OpenTelemetry convention, it sets `otel.name` and the convention's attributes (for MCP, `mcp.method.name` and `gen_ai.tool.name`, with the span named `tools/call {tool}`). The MCP and GenAI conventions are still in Development status and have moved to their own repository, so Eileen's own names stay authoritative [primary].

### 2.5 The span schema and its checker

A refactor can move a span under the wrong parent while every test still passes, because each test only checks the edge it cares about. LiveKit Agents solves this with a table of span names and their allowed parents, a short list of edges where a child may outlive its parent, and one checker that runs on in-memory spans in unit tests and on traces exported from real sessions [primary, `livekit/agents` `tests/trace_schema.py`].

Eileen copies the idea. `eileen_trace::schema` holds the table below as data; `schema::check(&[SpanRecord]) -> Vec<Violation>` runs in every simulation test and in `eileen trace check <incident>` [inferred].

| Span | Process | Allowed parents | May outlive parent |
|---|---|---|---|
| `turn` | core | root | |
| `speech.segment`, `turn.detect` | speech | remote `turn` (same trace) | |
| `stt.stream` | speech | `speech.segment` | |
| `addressee`, `router.decide`, `brain.turn`, `fastpath.run` | core | `turn` | |
| `job` | core | `turn`, root (scheduled and connector jobs) | yes: a job outlives the turn that started it |
| `replay.run` | core | `job` | |
| `replay.step` | core | `replay.run` | |
| `worker.session` | core | `job` | |
| `worker.tool_use` | core | `worker.session` | |
| `dispatch.call` | core | `worker.tool_use`, `brain.turn`, `fastpath.run`, `replay.step` | |
| `approval.wait`, `mcp.call` | core | `dispatch.call` | |
| `desktop.capture`, `desktop.action`, `apps.call` | sidecars | remote `mcp.call` | |
| `speak` | core | `turn`, `job` | |
| `tts.synth` | speech | remote `speak` | |
| `audio.play`, `bargein` | core | `speak` | |
| `learn.record` | core | `job` | |
| `librarian.run`, `supervisor.restart`, `incident.capture`, `runtime.stalled` | core | root, or any (`runtime.stalled` lands under whatever was blocked, as LiveKit's `event_loop_blocked` does) | |

Adding a span means adding a row; moving one without updating the row fails the simulation tests.

### 2.6 Workers: joining the claude CLI's own trace

Per-job workers are `claude -p` processes driven over stream-json. Four joins tie them to Eileen's trace [primary unless marked]:

1. **Environment at spawn.** The harness already builds each worker's environment explicitly (learning-loop and claude-mods reports). It adds `EILEEN_JOB_ID` (the claude-mods report's hooks already use it) and `TRACEPARENT` set to the `worker.session` span. The CLI documentation states that in `claude -p` and Agent SDK sessions, Claude "reads inbound `TRACEPARENT` and `TRACESTATE` from its environment, so its spans become children of the caller's trace", and that interactive sessions ignore it.
2. **Hooks.** The per-job `--settings` file's HTTP hooks send `X-Eileen-Job` and the hook payload's `session_id`, `tool_use_id`, `tool_name` and `tool_input`. The hook handler opens its span under the job.
3. **stream-json.** `system/init` gives `session_id`; each `tool_use` has an `id`; the `result` event has duration, turns and token use. These become fields on `worker.session` and `worker.tool_use`.
4. **The MCP shim.** A worker reaches Eileen's tools through its own stdio shim, which forwards to the core over a named pipe with a per-session token (computer-use report). The token identifies the job. The dispatcher matches the call to the `PreToolUse` hook that arrived just before it (same tool name, same input), which gives the `tool_use_id`. Whether the CLI also puts a tool-use ID or `traceparent` in `_meta` on stdio MCP calls is not documented; the CLI documents `traceparent` only on HTTP MCP requests. The shim should log the `_meta` it receives on the first call, so the build can adopt it if present [primary for the documented part, inferred for the rest].

**The brain is different.** It is one warm `claude -p` process serving many turns, and `TRACEPARENT` is only read at process start. So `brain.turn` spans correlate by `claude.session_id` and the per-turn `result` event, not by trace parenting [inferred].

**Optional: the CLI's own telemetry into Eileen's local sink.** When `otel.forward_claude = true` (section 3.5), the harness sets, in the worker environment only: `CLAUDE_CODE_ENABLE_TELEMETRY=1`, `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1`, `OTEL_TRACES_EXPORTER=otlp`, `OTEL_LOGS_EXPORTER=otlp`, `OTEL_METRICS_EXPORTER=none`, `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf` and `OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318`. Then the worker's `claude_code.interaction`, `claude_code.llm_request` (with `ttft_ms` and token counts) and `claude_code.tool` spans appear under Eileen's job in the same viewer. Content stays redacted: `OTEL_LOG_USER_PROMPTS`, `OTEL_LOG_TOOL_DETAILS`, `OTEL_LOG_TOOL_CONTENT`, `OTEL_LOG_ASSISTANT_RESPONSES` and `OTEL_LOG_RAW_API_BODIES` are all off by default and Eileen leaves them unset. The tracing part is labelled beta, so treat its span names as unstable [primary].

## 3. Sinks: what goes where

### 3.1 The subscriber stack in every Eileen process

`eileen_trace::init(ProcessKind, &TraceConfig) -> Result<TraceGuard, TraceError>` builds one `tracing_subscriber::Registry` with per-layer filters (`Layer::with_filter`) [inferred; the APIs are in tracing-subscriber 0.3.23, primary]:

| Layer | Default filter | Notes |
|---|---|---|
| `ErrorLayer` (tracing-error 0.2.1) | all spans | Makes `SpanTrace::capture()` work in errors and in the panic hook |
| JSON file (fmt `json`, through `tracing-appender` non-blocking) | `info` | Wrapped in a `reload::Layer` so the filter can change at runtime |
| Flight recorder ring (Eileen's own layer) | `eileen=debug,warn` | DEBUG for Eileen's crates, WARN for dependencies; TRACE is never recorded unless configured |
| Slow-span and stage-stats layer (Eileen's own) | span close only | `span.slow` events, per-stage latency histograms for `eileen debug stats` and the HUD's developer panel |
| Console (fmt, human format) | `warn` | Only when stdout is a terminal |
| `tracing-log` bridge | n/a | For dependencies that log through the `log` crate (tracing-subscriber's `tracing-log` feature) |
| Optional: OpenTelemetry (`eileen-otel`), ETW (`tracing-etw`), `console-subscriber` | off | Cargo features, section 3.5 to 3.7 |

Each process writes its own files and keeps its own ring. Sidecar stderr carries only text written before tracing starts and fatal messages; the supervisor already forwards it into the core's log as `child.stderr` (rust-architecture report).

### 3.2 JSON log files

- **Location**: `%LOCALAPPDATA%\eileen\logs\<process>.<YYYY-MM-DD>.jsonl`, for example `eileen.2026-10-20.jsonl`, `eileen-desktop.2026-10-20.jsonl`.
- **Rotation**: `tracing_appender::rolling::Builder` with `Rotation::DAILY`, `filename_prefix`, `filename_suffix("jsonl")` and `max_log_files(14)` per process. tracing-appender 0.2.5 has no size-based rotation [primary], so a janitor task in the core enforces `logging.max_total_mb` (default 500) by deleting the oldest files across all processes. (`logroller` 0.1.12, MIT, does size rotation, but one more dependency is not worth it.)
- **Writer**: `tracing_appender::non_blocking` with `lossy(true)`: when the buffer is full, lines are dropped instead of blocking an always-on process. Its error counter is exported as `logs.dropped` in `eileen debug stats`.
- **Fields on every line**: `timestamp` (UTC, microseconds), `level`, `target`, `filename`, `line_number`, `threadName`, the current span and the span list (`with_current_span(true)`, `with_span_list(true)`), and the event's fields flattened. `filename` and `line_number` cost nothing at run time (static metadata) and turn every log line into a code location for a person or an agent.
- **Content**: `pii.` fields are replaced by `{len, h}` (length and an 8-hex-digit keyed hash, so two lines about the same window title can be matched without the title) unless `logging.include_personal = true`.

### 3.3 Timelines in Perfetto

Eileen does not need `tracing-chrome` 0.7.2 (last release 2024-03) or `tracing-perfetto` 0.1.5 (2025-01) as always-on layers. Instead, `eileen_trace::chrome` converts span records (from the ring, a recording or the JSON files) into the Chrome JSON trace format, which "can be opened directly in the Perfetto UI", and which Perfetto's trace processor loads into SQL tables (`slice`, `track`, `thread`) [primary]. Own code (about 200 lines) beats the crates for two reasons [inferred]:

- **It merges processes**: one `pid` per Eileen process, all on one timeline aligned by wall clock.
- **It lays spans on tracks that nest.** Perfetto requires duration events to nest and puts overlapping ones on an overflow track; async events are not supported for that [primary]. Async Rust spans hop threads, so the converter gives each root span (each `turn`, each `job`, each `speak`) its own track and lays its descendants on it, and uses flow arrows (`s`, `f`) for cross-process parent links.

Every incident bundle contains `timeline.json`. `eileen trace <id> --chrome out.json` writes one on demand. To keep a trace out of a browser entirely, `trace_processor_shell` from Perfetto's v58.2 release (Apache-2.0, has a `windows-amd64` build) answers SQL questions locally, which also suits an agent ("the ten slowest `dispatch.call` slices in this incident") [primary]. Perfetto's embedding documentation says posted traces "stay in browser memory and are never uploaded" [primary].

**Tracy** (`tracing-tracy` 0.12.0, MIT OR Apache-2.0) builds the Tracy C++ client; useful later for frame-level profiling of the speech process, not for v0.1 [primary].

### 3.4 What the user can see

The HUD's developer panel (off by default) shows the current turn's stage timings from the stage-stats layer, and hovering a job shows its `job_` ID with a "copy" action. "Why did you do that?" is answered from the trace: the brain gets the last turn's `router.decide` reason, the skill and version used and the rung chosen for each action, which ARCHITECTURE.md section 3 already requires to be logged [inferred].

### 3.5 OpenTelemetry, optional and local only

- **Crate**: a separate `eileen-otel` library, linked by binaries only behind a cargo feature `otel` that is off by default. The PR lane never compiles it; nightly CI checks `--features otel` once. The reason is build cost: `opentelemetry-otlp` pulls `prost` and an HTTP client, and every clean build on this PC already takes 4 to 21 minutes [ran 10-11, ran 10-20]. Codex keeps OpenTelemetry in its own `codex-otel` crate behind a settings struct for the same kind of reasons [primary].
- **Versions**: `tracing-opentelemetry` 0.34.0 (2026-09-23, MIT), `opentelemetry`, `opentelemetry_sdk` and `opentelemetry-otlp` 0.33.1 (2026-10-09, Apache-2.0). `opentelemetry-otlp` defaults to `http-proto`, `reqwest-blocking-client`, `trace`, `metrics`, `logs` [primary]. Use `default-features = false, features = ["http-proto", "reqwest-blocking-client", "trace", "logs"]`; no TLS is needed for a loopback endpoint. Check that `reqwest` stays free of `native-tls`, which `deny.toml` bans.
- **Endpoint**: loopback only. `eileen-otel` refuses a non-loopback endpoint unless `otel.allow_remote = true` is set by hand, and logs a warning when it is.
- **Viewer**: the owner installs one himself. `otel-desktop-viewer` (Apache-2.0) is a single binary with a Windows build, listens on 127.0.0.1 for OTLP gRPC 4317 and HTTP 4318, serves its UI on port 8000, and keeps data in an in-memory DuckDB capped at 512 MB by default [primary]. Jaeger is the heavier alternative.
- **What is exported**: spans and events only, through the same `pii.` policy and redactor as the files (section 3.9). `pii.` fields are dropped unless `otel.include_personal = true`.

### 3.6 ETW, optional

`tracing-etw` 0.3.0 (Microsoft, MIT, released 2026-10-17, MSRV 1.82, marked experimental) emits `tracing` events as ETW events through `tracelogging` [primary]. Its value for Eileen is audio and scheduling bugs: Windows Performance Recorder can then show Eileen's `speech.segment`, `audio.play` and `bargein` spans on the same timeline as CPU scheduling and the OS audio stack. It is a cargo feature `etw`, off by default and checked nightly. Recording with WPR normally needs an administrator prompt, so it is a tool the owner runs by hand; Eileen never elevates [inferred].

### 3.7 Async debugging: what each task is waiting on

- **`await-tree`** 0.3.1 (RisingWave, Apache-2.0): each instrumented future is a node in a tree that can be dumped at any time, with how long each node has been pending [primary]. Eileen's `task::spawn` wrapper registers every task, and the job and dispatcher code mark the awaits that matter (`.instrument_await("approval.wait")`). `eileen debug tasks` prints the tree, and every incident includes `tasks.txt`. This answers "a job hung" on Windows without any unstable flags. Overhead must be measured before it is on by default (section 12).
- **Runtime lag probe**: a task wakes every 100 ms and measures how late it woke. Lateness over 250 ms logs `runtime.stalled` with the await tree attached; over 5 s it opens an incident. This works on stable tokio. `tokio-metrics` 0.5.2 (MIT) adds per-task poll statistics, but several runtime metrics need `--cfg tokio_unstable` [primary], so it is optional.
- **`tokio-console`**: `console-subscriber` 0.5.0 (MIT) needs `--cfg tokio_unstable` [primary], which changes `RUSTFLAGS` and so rebuilds every crate. On this PC that is a 20-minute rebuild. So `cargo xtask console` builds into its own target directory (`target\console`) with the flag set and the `console` feature on, takes a build slot, and is never part of CI or of the default build. Tokio's task dumps are not an option here: they are only supported on Linux [inferred from tokio documentation, re-check].

### 3.8 The audio thread

The WASAPI callback in `eileen-audio` must never allocate, lock or block. Formatting a `tracing` event allocates, so the callback never calls `tracing` [inferred; standard real-time audio practice]. Instead:

- The callback increments an `AtomicU64` frame counter (the heartbeat) and pushes a fixed-size `AudioTick { frames, peak, flags }` into an `rtrb` single-producer ring.
- An ordinary thread drains the ring every 50 ms and emits `tracing` events for anomalies only: underrun, glitch, device change, level stuck at zero.
- Debug and test builds wrap the callback in an allocation check: a counting global allocator that records allocations on threads flagged real-time. `assert_no_alloc` 1.1.2 is the reference design but is BSD-1-Clause, which is not on `deny.toml`'s allow list, so write the 40 lines instead of adding the license.

### 3.9 Redaction in every sink

The rule "nothing personal or secret leaves a sink it should not" is enforced in five layers [inferred unless marked]:

1. **Types.** Secrets are `secrecy::SecretString` (0.10.3), whose `Debug` prints a redacted marker. Config structs never derive `Debug` on secret fields without it.
2. **Fields.** User content only in `pii.` fields (section 2.4).
3. **Sink policy.**

   | Sink | `pii.` fields | Secrets scan |
   |---|---|---|
   | Flight recorder (memory) | kept | at dump time |
   | Incident bundle on disk | kept (local, for the owner) | yes |
   | Incident share pack (`eileen incident pack --share`) | removed | yes; minidumps left out unless added by name |
   | JSON log files | length and hash only, unless `logging.include_personal` | yes |
   | OpenTelemetry, ETW | dropped, unless `otel.include_personal` | yes |
   | Recordings | kept; audio only when opted in | yes, before write |
   | Console | kept | no (a terminal the owner is watching) |

4. **One scanner.** The learning loop's `Redactor` (API-key and token patterns, JWTs, private-key blocks, Luhn-valid card numbers, values of UIA password fields) runs on every line a file or OpenTelemetry sink writes, as a `MakeWriter` wrapper for the fmt layers and as a processor for OpenTelemetry. It counts redactions in `redactions.total`. A cheap literal prefilter (`sk-`, `eyJ`, `-----BEGIN`, `ghp_`, `xox`) keeps the cost per line low. The `Redactor` moves from the learning crate to `eileen-trace` so both use one implementation.
5. **A canary test in CI.** A simulation run injects fake secrets (an `sk-ant-` style key, a JWT, a Luhn-valid card, a UIA password value) and a marked transcript into events, tool results and worker output; the test then searches the bytes of every sink (log files, a ring dump, an in-memory OpenTelemetry exporter, an incident bundle, a share pack, a recording) and fails if any raw string appears where the policy says it must not. This extends the learning loop's acceptance test 2 from `learn.sqlite` to every sink.

Panic messages get one extra rule from Zed: Rust's string-slicing panics include the user's string ("byte index 4 is out of bounds of `...`"), so the panic hook replaces the quoted part with `<redacted>` [primary, Zed `crates/crashes`].

Minidumps contain process memory, which can include transcripts and tokens in flight. They stay on the PC, the share pack leaves them out by default, and they use the normal (stacks and modules) dump type, not full memory [inferred].

## 4. Flight recorder, crash and hang handling

### 4.1 The rings

Go 1.25's flight recorder is the reference: it "buffers the last few seconds of the trace in memory", bounded by a minimum age and a maximum size, and a program snapshots it when it detects a problem [primary]. Eileen keeps three lanes per process [inferred]:

| Lane | Contents | Default bound | Where |
|---|---|---|---|
| Diagnostic | Span open and close records and events at the ring's filter (DEBUG for Eileen crates), as compact JSON | 10 minutes or 32 MB in the core, 8 MB per sidecar, whichever comes first | every process |
| Input | Every value that entered the core through a port (section 5), plus a state checkpoint every 60 s | same window, about 4 MB | core |
| Audio | 16 kHz mono after echo cancellation; optionally the raw 48 kHz mic and the playback reference, for echo and barge-in bugs | off; up to 120 s (about 3.8 MB for the mono lane) when `flight.audio_seconds` is set | core (it owns capture) |

Per-window VAD data (31 windows a second) is TRACE and stays out of the ring; one DEBUG summary per segment goes in. In v0.1 the rings are on the heap. A memory-mapped ring file would survive a hard crash and could be read by the guard without the dying process's help, which matters because of the fail-fast case in section 4.5; that is a v0.2 option once the heap version is measured.

### 4.2 Triggers

| Trigger | Who notices | What is captured |
|---|---|---|
| Panic in a core task | panic hook (the core unwinds, the task's `JoinError` fails its job) | all rings, the span trace and backtrace of the panic; no minidump |
| Panic on a critical thread (audio, message pump, main) or in a sidecar | panic hook | the above plus a minidump through `simulate_exception` before the thread or process dies |
| Native crash (access violation, stack overflow, invalid parameter, purecall) | `crash-handler` in the crashing process, which asks the guard | minidump written from outside; rings of the other processes; the crashed process's ring only if it was dumped in time |
| Sidecar heartbeat or call deadline missed | supervisor | the sidecar's ring if it answers within 1 s, a hang minidump of it through the guard, then the kill; its JSON log tail otherwise |
| Core heartbeat missed for 10 s | guard | hang minidump of the core, restart under the rate limit, an incident written by the guard |
| Audio heartbeat stalled for 1 s while capture should run | watchdog thread in the core | rings, audio lane if enabled, hang minidump of the core (shows the stuck thread's stack), then a stream restart |
| Runtime lag over 5 s | lag probe | rings, `tasks.txt` |
| Job with no progress event for its kind's limit | job registry | `job.stalled` event; an incident only if the user asks |
| "Eileen, something went wrong", "save a bug report" | fast-path grammar, no model call | rings, `tasks.txt`, the last DesktopState, the user's note (the utterance) |
| Hotkey (proposed Ctrl+Alt+Shift+B), tray "Save a debug report", `eileen incident capture` | shell, CLI | same as the spoken trigger |

"That's wrong" stays a learning signal (learning-loop report) and does not write an incident; after it, the HUD offers "Save a report?". Each trigger kind is rate-limited to one incident per 10 minutes, and the incident directory is capped (section 8.3).

### 4.3 The incident bundle

```
%LOCALAPPDATA%\eileen\incidents\2026-10-20T15-31-02Z_inc_01K82Q...\
  manifest.toml          what happened, versions, which files are here
  README.txt             plain description of the contents and their privacy
  events.jsonl           diagnostic lanes of every process, merged and sorted by time
  inputs.jsonl.zst       the core's input lane with its checkpoints (replayable)
  timeline.json          Chrome JSON trace for Perfetto
  tasks.txt              await-tree dump of every live task
  desktop-state.json     last DesktopState and the deltas in the window
  jobs.json              job registry snapshot
  config.redacted.toml   the effective config, secrets removed
  logs\                  the last 2,000 lines of each process's JSON log
  audio\                 mic-16k.wav (and raw and reference) only if enabled
  dumps\                 minidumps, one per process dumped
```

```toml
# manifest.toml
id = "inc_01K82Q9V7T0J4X3M2N8A5B6C7D"
trigger = "watchdog.sidecar_hung"            # or panic, crash, user, hotkey, runtime_stalled, audio_stalled
note = ""                                     # the user's words for a spoken trigger
at = "2026-10-20T15:31:02.118Z"
session = "sess_01K82M..."
turns = ["turn_01K82Q8...", "turn_01K82Q9..."]  # turns inside the window
jobs = ["job_01K82Q8..."]
eileen_version = "0.1.0"
git_commit = "3108de0"
build_profile = "release"
os = "Windows 11 Home 26200"
cpu_load_pct = 71
free_ram_mb = 1840
processes = ["eileen", "eileen-speech", "eileen-desktop", "eileen-apps", "eileen-hud"]
contains_personal = true                       # transcripts and window titles are in events.jsonl
contains_audio = false
redactions = 3
dumps = ["dumps/eileen-desktop-14812.dmp"]
```

### 4.4 `eileen-guard.exe`

Embark's crates split crash handling in two: `crash-handler` runs user code when the process crashes, and `minidumper` is "an IPC client and server for creating minidumps of an external process", with `minidump-writer` doing the dumping [primary]. Dumping from outside is the reliable way: `minidump-writer`'s own documentation says "it is inherently unreliable to dump the currently running process" during a real exception [primary]. The repository's example has the application spawn itself as the dump server; Zed does the same in its `crates/crashes` [primary].

Eileen's version [inferred, except where marked]:

- **A separate tiny binary**, `eileen-guard.exe`, crate `eileen-guard`, std plus `minidumper` only. The core starts it first, through the supervisor, but **outside the kill-on-close Job Object**, so it survives the core. It exits when the core exits cleanly.
- **One server, many clients.** Every Eileen process (core and sidecars) connects with `minidumper::Client::with_name`, sends its process kind and version with `send_message`, and pings every few seconds (Zed pings every 10 s) [primary].
- **Crash**: the client's crash callback calls `request_dump(&crash_context)`, which blocks until the guard has written the dump [primary]. The guard's `create_minidump_file` puts it in `incidents\pending\`, and the core (or the guard, if the core is gone) assembles the bundle.
- **Hang, without killing**: for a hung core or sidecar, the guard builds a crash context with no exception pointers and the target's process ID and calls `MinidumpWriter::dump_crash_context`. The Windows implementation opens the external process, and treats a null exception pointer as "no exception information", so this gives every thread's stack of a live, hung process [primary, `minidump-writer` source]. That is exactly what the 38.7-minute UIA hang in the rust-architecture report needed: which COM call each thread was stuck in, captured before the supervisor kills the sidecar.
- **Core liveness**: a dedicated core thread sends a heartbeat message every second carrying the runtime tick count and the audio frame counter. No heartbeat for 10 s means a hang dump, then a restart: at most one restart per 10 minutes, with a toast ("Eileen stopped responding and was restarted; a report was saved"). Restart policy is configurable and can be off.
- **No unsafe code in Eileen's guard.** `crash_handler::make_crash_event` is an `unsafe fn`, so the client side lives in a small `eileen-crash` library whose single unsafe call sits in one `#[expect(unsafe_code)]` module with a safety comment, following the rust-architecture report's FFI rule.
- **Windows support**: the crates support `x86_64-pc-windows-msvc`, not Windows on ARM [primary]. The socket is an AF_UNIX path, which Windows has supported since 2018 [inferred].
- **Zed's crash code is GPL-licensed** (its `crates/crashes` carries `LICENSE-GPL`), so Eileen copies the design, not the code [primary].

### 4.5 Panic policy

- The panic hook is the first thing every Eileen process installs (`eileen_crash::install`). It records a `panic` event with the cleaned message, the location, the thread name, a `SpanTrace` (which carries the turn and job IDs of the panicking code) and a backtrace (`std::backtrace::Backtrace::force_capture`; `debug = "line-tables-only"` resolves our frames), then asks the core's incident task to dump the rings.
- **In the core, panics unwind.** A panicking task fails its job with an internal error and the core keeps listening; this is why the rust-architecture report chose `panic = "unwind"`. Only a panic on a critical thread also takes a minidump.
- **In sidecars, and on critical threads, the hook requests a minidump through `CrashHandler::simulate_exception` before the process can abort.** Zed does exactly this on Windows, then aborts [primary].
- **Fail-fast is invisible to in-process handlers.** Rust's `abort` on Windows uses `__fastfail`, which "will terminate the process immediately without running any in-process exception handlers" [primary, Rust std source]. A double panic, a panic under `panic = "abort"` after the hook has run, an out-of-memory abort or a `/GS` stack check therefore reaches no crash handler. The guard sees the exit code, writes an incident that says "terminated by fail-fast, no dump", and attaches the other processes' rings. Windows Error Reporting's local dumps would capture these, but they are a machine-wide registry setting that needs administrator rights, so that is the owner's choice and Eileen never changes it.

### 4.6 Watchdogs, summarized

| What | Signal | Threshold | Action |
|---|---|---|---|
| Audio capture | frame counter stops | 1 s while capturing | `audio.stalled`, hang dump of the core, restart the stream (device lost is the usual cause) |
| Audio playback | frames handed to the device stop while speaking | 500 ms | `audio.play_stalled`, stop the `speak`, restart output |
| Tokio runtime | lag probe | 250 ms warn, 5 s incident | `runtime.stalled` with the await tree |
| Core process | guard heartbeat | 10 s | hang dump, restart under rate limit |
| Sidecar | supervisor heartbeat and per-call deadline (rust-architecture report) | per sidecar, for example 5 s for a UIA call | ring request, hang dump through the guard, kill, restart, typed error to the job |
| Worker | no stream-json event and no hook | per job kind, for example 120 s while a tool runs | `worker.silent` event; the HUD shows it; the job is not killed automatically |
| Job | no progress event | per kind | `job.stalled` |

### 4.7 From a minidump to a stack

- **Symbols**: release CI already uploads `.pdb` files (rust-architecture report). It also runs Mozilla's `dump_syms` (v2.3.9, Apache-2.0, has a Windows build) to turn each PDB into a Breakpad `.sym` file and attaches `eileen-<version>-symbols.zip` to the GitHub release [primary].
- **Stack walk**: `cargo xtask symbolicate <dmp>` downloads the matching symbols zip (by version in the manifest) and runs `minidump-stackwalk` (rust-minidump v0.27.0, MIT, prebuilt `x86_64-pc-windows-msvc` zip) with the symbols path; it writes `stack.txt` into the incident [primary].
- **WinDbg** reads PDBs and Microsoft's symbol server directly, and is the tool for OS frames (COM, UIA, audio).
- **Time Travel Debugging** (`ttd.exe`) records a process at instruction level and replays it in WinDbg, with a ring-buffer mode (`-ring`, `-maxFile`). It slows the target "5x-20x or more", "recording TTD traces requires administrative rights", and recordings "might contain personally identifiable or security related information" [primary]. It is a last-resort tool the owner runs by hand on one sidecar, never something Eileen starts.

## 5. Record and replay

### 5.1 The principle: a closed set of inputs

rust-analyzer's architecture document describes its main loop: "Rather than spawning futures or scheduling callbacks (open), the event loop accepts an `enum` of possible events (closed). It's easy to see all the things that trigger rust-analyzer processing" [primary]. Temporal applies the same idea to durable workflows: a workflow's event history is replayed against current code in tests, and any mismatch is reported as a nondeterminism error at a specific event [secondary, community documentation].

For Eileen this means: every source of nondeterminism enters the core through a port trait, and nothing else does. The engineering-practice report's simulation harness implements the ports with fakes driven by a seed; recording implements them as a tap that logs what the real port returned; replay implements them from a log.

| Port | Real implementation | What a recording stores |
|---|---|---|
| `Clock` | system time and tokio time | the time each input was observed; timers fire from the log |
| `Ids` | `Ulid::generate()` | each ID handed out, in order |
| `Rng` | seeded RNG | the seed |
| `Speech` | frames from `eileen-speech` (segments, partials, finals, end of turn, TTS progress) | each event |
| `AudioIn` | WASAPI capture | audio lanes, only when audio recording is on |
| `Brain` | warm `claude -p` session | per turn: hash of the input block and the full stream-json output |
| `Workers` | `claude -p` per job, hooks | each stream-json line and hook payload, per job |
| `Tools` | connectors and sidecars behind the dispatcher | per `call_`: arguments hash and full result (large results as blobs) |
| `Desktop` | DesktopState deltas and app-hook events | each delta; full snapshots as blobs |
| `UserInput` | hotkeys, HUD, tray, phone | each input |
| `Config` | config file watcher | the effective config hash and changes |

The dispatcher's policy decisions, the router, the job registry, the scheduler, the learning matcher and every timer are core logic, so replay re-executes them; that is the code under test.

### 5.2 Where things are stored, without duplication

| Data | Stored in | Referenced by |
|---|---|---|
| Bus events (outputs of the core) | `events` table in `eileen.db` (ARCHITECTURE.md section 5), now with `seq` and `TraceCtx` columns | recordings (as the expected output for a time range), incidents |
| Inputs (port values) | input lane in memory; spilled to `recordings\rec_<id>\inputs.jsonl.zst` only when recording | replay |
| DesktopState snapshots, screenshots, large tool results, brain prompt prefixes | `blobs\<first two hex>\<blake3 hex>.zst`, written once | input lane entries, trajectory steps |
| Trajectories (per job, redacted, summarized) | `learn.sqlite` (learning-loop report) | skills as evidence |
| Join key between a trajectory step and a recorded tool result | `call_` ID in both | `eileen trace`, the librarian |

Blob garbage collection is by reachability from trajectories, live recordings and incidents. The learning loop's 200 MB cap and the recording cap (section 8.3) are enforced separately; the blob store counts toward whichever owner still references a blob [inferred].

### 5.3 Replay levels

| Level | What runs | What is replayed | Deterministic | Where |
|---|---|---|---|---|
| L0 inspect | nothing | logs and rings | n/a | `eileen trace`, Perfetto |
| L1 core | the real core logic inside the simulation harness, fake clock | every port from the log, starting at the nearest checkpoint | yes | `eileen replay <rec or inc> --level core`; CI on committed fixtures |
| L2 voice | L1 plus the real speech process | recorded audio into VAD, STT and turn detection with fixed thread counts; speech events compared with a time tolerance | nearly (float results can differ with ONNX Runtime threading) | locally, or a nightly job with cached models |
| L3 live | the real Eileen | only the user's inputs (audio or transcripts and hotkeys), against the real brain, workers and desktop | no; uses plan quota | by hand, to reproduce UI bugs |

An L1 replay compares each bus event it produces with the recorded one and stops at the first difference:

```
replay inc_01K82Q... (level core): diverged at seq 412 (t+73.204 s)
  expected  dispatch.call  tool=desktop.click  args.element=17  job_01K82Q8... step 6
  actual    dispatch.call  tool=desktop.click  args.element=19
  span path turn_01K82Q8... > job job_01K82Q8... > worker.session > worker.tool_use toolu_01...
  inputs    seq 409 desktop.delta (blob b3:5f2a...), seq 411 worker stream line 38
```

In L1 the model's choices are replayed as recorded, so a divergence is a change in Eileen's own code (element indexing in the shim, a policy decision, routing, a timer), which is what a regression test should catch.

### 5.4 From an incident to a regression test

1. The incident is replayable as it is: `eileen replay inc_... --level core` reproduces the last minutes from the checkpoint nearest the start of the window.
2. `eileen record trim inc_... --turn turn_... --out tests/replays/<name>` keeps the inputs and blobs that one turn needs, plus the checkpoint before it.
3. For a fixture from a real session, `--synthesize` replaces audio and transcripts with Kokoro-voiced synthetic equivalents and `pii.` values with placeholders, and sets `origin = "synthetic-from-user"` in the fixture manifest.
4. The fixture is committed with an expected outcome; CI replays it at L1 on every PR.
5. A tidy check in `cargo xtask ci` rejects any fixture whose manifest says `origin = "user"`, any WAV under `tests/replays/` that lacks a synthetic marker, and any `pii.` value that is not a placeholder. The repository is public; the owner's voice and screen never go into it.

The owner's benchmark requests become timed scenarios the same way: a scripted synthetic session, recorded once against the real desktop (L3), then replayed at L1 in CI for logic and at L3 by hand for timing.

### 5.5 Privacy rules for recording

- The input and diagnostic lanes are always on, in memory only. Nothing is written until an incident or an explicit recording.
- Writing a recording to disk is opt-in: "Eileen, record this session", "stop recording", `recording.enabled = true` or `eileen record start`. The HUD shows a recording indicator next to the mic indicator whenever anything is being spilled to disk.
- Audio is a separate opt-in (`recording.audio`, `flight.audio_seconds`), because audio cannot be redacted.
- The `Redactor` runs before anything is written. DesktopState never holds the value of a password field.
- Retention: recordings 7 days and 2 GB, incidents 30 days and 1 GB, all under the user's profile. "Forget that" and "delete my recordings" remove recordings and their unreferenced blobs.
- Encryption at rest (a DPAPI-wrapped key with `chacha20poly1305`) is a v0.2 option; v0.1 relies on the user profile's file permissions, as the learning store does.

### 5.6 Interfaces

```rust
// eileen-contracts/src/record.rs (wire types; snapshot-tested like every contract)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InputRecord {
    pub seq: u64,
    pub at_us: i64,
    pub ctx: TraceCtx,
    pub port: Port,                    // Clock, Ids, Speech, Brain, Workers, Tools, Desktop, UserInput, Config
    pub value: serde_json::Value,      // redacted; large parts replaced by {"$blob": "b3:..."}
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Checkpoint { pub seq: u64, pub at_us: i64, pub state: CoreState }  // jobs, mute, presence, connectors, config hash, skill versions

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecordingManifest {
    pub id: RecId, pub origin: Origin, pub started_us: i64, pub ended_us: i64,
    pub eileen_version: String, pub git_commit: String, pub config_hash: String,
    pub audio: Vec<AudioLane>, pub turns: Vec<TurnId>, pub jobs: Vec<JobId>,
}
```

```rust
// eileen-record/src/lib.rs (signatures only)
pub trait InputTap: Send + Sync {
    fn record(&self, port: Port, ctx: TraceCtx, value: &serde_json::Value);
}
pub trait BlobStore: Send + Sync {
    fn put(&self, bytes: &[u8]) -> Result<BlobRef, RecordError>;   // BLAKE3, zstd, idempotent
    fn get(&self, r: &BlobRef) -> Result<Vec<u8>, RecordError>;
}
pub struct Recording { /* reader over manifest, inputs, blobs, audio */ }
impl Recording {
    pub fn open(dir: &Path) -> Result<Self, RecordError>;
    pub fn inputs_from(&self, checkpoint: &Checkpoint) -> impl Iterator<Item = InputRecord>;
    pub fn expected_events(&self) -> impl Iterator<Item = Envelope<BusEvent>>;
}
```

The replayer that implements the ports from a `Recording` lives with the simulation harness (engineering-practice report).

## 6. Code maps for people and agents

### 6.1 What CI generates

| Artifact | Lane | How | Kept where |
|---|---|---|---|
| `docs/CODEMAP.md`: Mermaid graph of workspace crates grouped by process, a process and IPC diagram, a crate table | every PR (`cargo xtask codemap --check`, seconds) | `cargo metadata` plus `[package.metadata.eileen]` in each crate manifest | committed; CI fails if stale |
| `docs/reference/telemetry.md`: every span and event name, kind, allowed parents, fields, `pii.` marking | every PR (a unit test) | rendered from `eileen_trace::names` and `schema` | committed; test fails if stale |
| `docs/reference/events.md`, `docs/reference/mcp-tools.md` | every PR (unit tests) | rendered from `eileen-contracts` schemas (schemars) | committed |
| Tidy: every module file is listed in its crate's `CLAUDE.md` "Modules" section | every PR | `cargo xtask ci` | n/a |
| Module trees and module dependency graphs per crate | nightly | `cargo modules structure` and `cargo modules dependencies` (cargo-modules 0.28.0) | artifact |
| Compile-time report | nightly | `cargo build --timings` | artifact |
| Private-items API docs | nightly | `cargo doc --document-private-items` | artifact, optionally GitHub Pages |
| `--features otel,etw` check build | nightly | `cargo check` | n/a |

rust-analyzer is the model for generated files: they are "updated automatically on `cargo test`" and "generally committed to the git repository", and its tidy checks run as ordinary tests [primary]. Committed generated Markdown renders on GitHub (Mermaid included) and is readable by an agent without running anything.

### 6.2 `cargo xtask codemap`

- Reads `cargo metadata --format-version 1` and each member's `[package.metadata.eileen]`:

  ```toml
  [package.metadata.eileen]
  process = "eileen-desktop.exe"   # or "eileen.exe", "lib", "tool"
  owns = "UI Automation reads, window state and DesktopState; the eileen-desktop MCP server"
  unsafe = "ffi"                   # or "forbid"
  ```

  plus `[workspace.metadata.eileen.processes]` in the root manifest (owned by WP0), which lists each process, its transport to the core and its peers.
- Writes between `<!-- generated:codemap:begin -->` and `<!-- generated:codemap:end -->` markers: a `graph LR` of workspace crates (normal dependencies, one subgraph per process), a process diagram (core, guard, speech, desktop, apps, HUD, worker shims, harness children and their transports), and a table: crate, process, unsafe policy, what it owns, notable external dependencies (a short allow list: tokio, rmcp, windows, uiautomation, sherpa-onnx, ort, rusqlite, cpal), link to its `CLAUDE.md`.
- Checks that every declared process has a binary target and that every crate has `owns`.
- xtask stays fast: add `serde_json` with `Value` only (no derive); it is already compiled for the rest of the workspace.

### 6.3 Tools evaluated

| Tool | License | What it gives | Cost on this PC | Verdict |
|---|---|---|---|---|
| `cargo metadata` plus xtask | n/a | crate graph, kinds, features | seconds | **Use** (PR lane) |
| cargo-modules 0.28.0 (2026-10-18) | MPL-2.0 (a tool, not linked) | module trees, item-level dependency graphs | no prebuilt release; MSRV 1.98; builds rust-analyzer libraries from source | **Nightly CI only**, never installed locally |
| cargo-depgraph 1.6.0 (2023-12) | GPL-3.0-or-later (a tool) | full dependency graph as DOT | needs Graphviz to render | Not needed; xtask covers workspace crates, cargo-deny covers duplicates |
| `cargo tree` | n/a | ad hoc dependency questions | built in | Use by hand |
| rust-analyzer SCIP (`rust-analyzer scip`) | MIT OR Apache-2.0 | a precise cross-reference index | full analysis per run | **Defer**: no local consumer; the `scip` CLI v0.10.0 ships no Windows build [primary] |
| rust-analyzer LSIF (`rust-analyzer lsif`) | same | older index format, still in the CLI | same | **No**: superseded by SCIP |
| rustdoc, private items | n/a | browsable, searchable map with source links | one doc build | Nightly artifact |
| Claude's LSP plugin mechanism with rust-analyzer | plugin Apache-2.0, rust-analyzer MIT OR Apache-2.0 | definition, references, hover, symbols, implementations, call hierarchy, and diagnostics pushed after each edit [primary] | rust-analyzer can take several GB (2.5 GB reported on a company repo, 12 GB idle on Zed's) [secondary] | **Use, through `eileen-dev` with tuned settings** (section 6.4) |
| Rust Glancer 0.3.0 (2026-10-01) | Apache-2.0 OR MIT | an experimental LSP "optimized for low memory usage", aiming at 90 percent of rust-analyzer [primary] | prebuilt Windows build | **Measure** as a drop-in `command` in the same `.lsp.json` |
| codegraph v1.6.2 (2026-10-03) | MIT | a tree-sitter graph in local SQLite behind one MCP tool, `codegraph_explore` (source, call paths, blast radius); file watcher keeps it current [primary] | light compared with rust-analyzer; its own Rust extraction score is 86.7 percent on ripgrep, and tree-sitter cannot see macro expansion or resolve trait dispatch | **Optional** supplement; telemetry is on unless turned off (`CODEGRAPH_TELEMETRY=0`, `DO_NOT_TRACK=1`) |
| Serena | the application is GPL-3.0-or-later, its SolidLSP part MIT [primary] | LSP-backed MCP tools, symbol editing | Python plus its own rust-analyzer | **No**: duplicates the native LSP tool, adds a second rust-analyzer |
| GitNexus | PolyForm Noncommercial 1.0.0 [primary] | code graph | | **No**: not an open-source license |
| CodeGraphContext | MIT | code graph in a graph database | needs KuzuDB, FalkorDB or Neo4j | No: heavier than codegraph for the same job |
| mcp-language-server | BSD-3-Clause, last push 2026-03-01 | LSP as MCP tools | Go binary plus rust-analyzer | No: the native LSP tool replaces it |
| Sourcegraph | needs a Sourcegraph instance | code search and navigation | a server | No |
| ast-grep (CLI, and an MIT MCP server) | MIT | structural search, for example every `tokio::spawn` outside the wrapper | small | Use the CLI through Bash when needed; no MCP |

### 6.4 What Eileen's own Claude workers get on Eileen's codebase

When a job's working directory is an Eileen worktree, the harness adds `--plugin-dir <repo>\tools\claude-plugins\eileen-dev` to the worker's flags. The plugin lives in the repository, so its configuration is reviewed Markdown and JSON like everything else [inferred unless marked]:

```
tools/claude-plugins/eileen-dev/
  .claude-plugin/plugin.json          name "eileen-dev", license MIT
  .lsp.json                           rust-analyzer with tuned settings
  .mcp.json                           eileen-debug (and optionally codegraph)
  skills/debugging-eileen/SKILL.md    section 7 as a procedure
  skills/tracing-conventions/SKILL.md section 2.4 and how to add a span
  skills/adding-a-crate/SKILL.md      manifest metadata, CLAUDE.md template, codemap
```

```json
{
  "rust-analyzer": {
    "command": "rust-analyzer",
    "extensionToLanguage": { ".rs": "rust" },
    "initializationOptions": {
      "checkOnSave": false,
      "cargo": { "targetDir": true },
      "cachePriming": { "numThreads": 2 },
      "numThreads": 2,
      "lru": { "capacity": 64 },
      "files": { "exclude": [".codegraph", "tests/replays"] }
    },
    "startupTimeout": 120000,
    "requestTimeout": 60000,
    "maxRestarts": 2
  }
}
```

Why not the official `rust-analyzer-lsp` plugin as it is: its marketplace entry starts `rust-analyzer` with no options [primary], and rust-analyzer's default `checkOnSave = true` runs `cargo check` after every save [primary]. On this PC that is a compile outside the build-slot guard each time a worker edits a file. With `checkOnSave` off the worker still gets rust-analyzer's own diagnostics after each edit and full navigation, and runs clippy through `cargo xtask` as the rust-architecture report requires. `cargo.targetDir = true` keeps rust-analyzer's build-script and proc-macro output in a subdirectory, so it never locks the worktree's own target directory [primary for the setting]. The `.lsp.json` keys are strict (an unknown key fails validation), and `requestTimeout` needs CLI 2.1.288 or later [primary]; validate with `claude plugin validate --strict` in CI, as the claude-mods report already does for the `eileen` plugin. On a larger machine, set `checkOnSave: true` with `check.command = "clippy"`.

Memory policy for this PC: at most one worker with `eileen-dev`'s LSP at a time (`dev.max_lsp_workers = 1` in config, enforced by the harness), because one rust-analyzer can need more than the 2 GB free today. Other coding workers use `docs/CODEMAP.md`, the crate `CLAUDE.md` files, `rg` and optionally codegraph. rust-analyzer comes from `rustup component add rust-analyzer` in `cargo xtask setup`, prebuilt.

`eileen-debug` is a small read-only MCP server in `eileen-mcp` (served by `eileen mcp-debug`) with tools `trace_get(id)` (a turn, job, call or incident ID, returned as a stitched Markdown timeline with file and line of each event), `logs_query(filter, since)`, `incident_list`, `incident_read(id, part)`, `recording_list` and `replay_core(id)` (runs an L1 replay and returns the divergence report). It applies the share-pack redaction policy to everything it returns, because a worker's context goes to a model vendor.

## 7. Debugging Eileen: guide outline

This becomes `docs/DEBUGGING.md` and the `debugging-eileen` skill. Every entry follows the same path: what to look at first, the spans and events, the recording, then the code. `eileen trace last` shows the most recent turn; `eileen trace last --job` the most recent job.

**Start for any symptom**

1. Say "Eileen, something went wrong" (or press the hotkey) right after it happens. That freezes the last ten minutes.
2. `eileen incident show last` prints the manifest and a one-screen summary: turns in the window, their routes and outcomes, errors, restarts, stalls.
3. `eileen trace <turn_...>` gives the stitched timeline; `timeline.json` opens in Perfetto.
4. `eileen replay inc_... --level core` tells whether the bug is in Eileen's logic (it diverges or reproduces) or in a model, the desktop or timing (it replays clean).

| Symptom | Look first | Spans and events | Recording | Code |
|---|---|---|---|---|
| It did not hear me | HUD mic state; `mute.changed`; is there a `turn` at all? | none: `audio.stalled`, `audio.device`, `speech.segment` count and `vad_max`, `supervisor.restart{proc=eileen-speech}`. A `turn` with `addressee.decision = not_addressed`: the `reason` | L2 with audio, if audio was on: try VAD thresholds | `eileen-audio` (capture), `eileen-speech` (`vad`, `turn`), `eileen-core::router::addressee` |
| It heard the wrong words | `pii.transcript` on `stt.stream` | `stt.stream` partials and final, `model` | L2 replay with another model or with hotwords | `eileen-speech::stt`; vocabulary in the learning store |
| It cut me off, or waited too long | `turn.detect` | `silence_ms`, `smart_turn_p`, `decision`; time between the last segment and `turn` | L2 | `eileen-speech::turn` (voice-stack report section 5) |
| It answered slowly | `span.slow` events; `eileen debug stats` | stage durations on `turn`: end of speech to `turn.detect`, `router.decide`, `brain.turn` (`ttft_ms`), `speak` (`first_audio_ms`); `runtime.stalled`; CPU and free RAM in the manifest | L1 shows whether core logic is slow; L0 for the rest | budget tables in the voice-stack (section 9) and brain-latency reports |
| It did something I did not ask for | `router.decide` | `route`, `skill`, `reason`; `brain.turn` output in the input lane; `replay.step` checks | L1: same decision from the same inputs means logic; else a model choice | `eileen-core::router`, the learning crate's matcher and ladder |
| It clicked the wrong button | the job's `desktop.action` spans | `rung`, target element, rect, DPI and monitor; the `desktop.capture` blob before the action; `dispatch.call` arguments; the worker's text before its `tool_use` | L1 with the recorded DesktopState shows whether the shim mapped the element correctly; L3 to reproduce on screen | `eileen-desktop` action ladder (computer-use report sections 2.4 and 6) |
| A job hung | HUD job row; `job.stalled`, `worker.silent` | open spans under the `job` in `tasks.txt`: `approval.wait` (waiting for the user), `mcp.call` past its deadline (sidecar), `worker.session` with no events (the CLI), `rate_limit_event` in the stream | incident with the sidecar's hang dump | `eileen-core` jobs and supervisor, `eileen-harness` |
| It did not stop when I interrupted, or talked over me | `bargein` | `detect_ms`, action (duck, stop), `audio.play.played_samples`, `heard_chars`; echo-canceller statistics | L2 with raw and reference audio (opt-in) | `eileen-audio` barge-in, voice-stack report section 3 |
| A learned skill misbehaved | `replay.run` | skill ID and version, each `replay.step` with pre and post checks, `ReplayHandoff` | L1 | the learning crate's `replay`; `eileen skills history <id>` |
| Eileen restarted or a sidecar crashed | `eileen incident show last` | `supervisor.restart`, `panic`, the manifest's trigger | minidump: `cargo xtask symbolicate` | the frame at the top of `stack.txt` |
| High memory or CPU at idle | `eileen debug stats` | process tree working set and handles; `logs.dropped`; ring sizes | soak test baselines (engineering-practice report) | `cargo xtask console` build; ETW with WPA for scheduling |
| The HUD froze or shows stale state | is `eileen-hud` alive? | `supervisor.restart{proc=eileen-hud}`, HUD event backlog | L0 | `eileen-hud` |

**For an AI agent working on a bug**: read `docs/CODEMAP.md` and the crate's `CLAUDE.md`; call `trace_get` on the ID from the report; every line it returns has `filename` and `line_number`; grep the span or event constant in `eileen_trace::names` to find every emitter; run `replay_core` before and after the fix; turn the incident into a fixture (section 5.4) in the same pull request.

## 8. Where each piece lives

### 8.1 Crates and modules

| Piece | Crate and module | Unsafe | Notes |
|---|---|---|---|
| ID newtypes, `TraceCtx`, `Envelope`, incident and recording manifests, `InputRecord`, `Checkpoint` | `eileen-contracts::{ids, trace, incident, record}` | forbidden | snapshot-tested like every contract |
| Subscriber setup per process, name constants, span schema and checker, `pii.` policy, `Redactor` (moved here from the learning crate), ring layer, slow-span and stage-stats layer, Chrome trace converter, traceparent formatting and parsing, test capture layer | new `eileen-trace::{init, names, schema, pii, redact, ring, stats, chrome, ctx, test_support}` | forbidden | every binary links it; no OpenTelemetry dependency |
| OpenTelemetry export | new `eileen-otel` | forbidden | behind feature `otel` in binaries |
| Panic hook, crash handler, guard client | new `eileen-crash` | one `#[expect(unsafe_code)]` module for `make_crash_event` | every binary links it |
| Minidump server, liveness watchdog, hang dumps, core restart | new `eileen-guard` (binary `eileen-guard.exe`) | none of our own | std plus `minidumper` |
| `task::spawn` wrapper, lag probe, watchdogs, incident assembly, janitor, supervisor hooks (ring request, hang dump, then kill) | `eileen-core::{task, watchdog, incident, janitor, supervisor}` | forbidden | |
| Ports (traits) and the recording tap | `eileen-core::ports` | forbidden | the simulation harness implements the same traits |
| Recording reader and writer, blob store | new `eileen-record` | forbidden | the learning crate uses `BlobStore` too |
| Replayer and divergence report | the simulation harness crate (engineering-practice report) | forbidden | |
| Audio heartbeat, `rtrb` tick ring, real-time allocation check | `eileen-audio` | `ffi` module only, as now | |
| `eileen-debug` MCP server | `eileen-mcp::debug` | forbidden | |
| CLI: `trace`, `logs`, `incident {list, show, capture, pack}`, `record {start, stop, list, trim}`, `replay`, `debug {stats, tasks}`, `mcp-debug` | `eileen-cli` | forbidden | |
| `codemap`, `symbolicate`, `console`, generated-docs update, fixture tidy checks | `xtask` | forbidden | |
| `eileen-dev` Claude plugin | `tools/claude-plugins/eileen-dev/` | n/a | Markdown and JSON |
| Debugging guide | `docs/DEBUGGING.md` (and the `debugging-eileen` skill) | n/a | |

The learning crate is called `eileen-skills` in the rust-architecture report and `eileen-learn` in the learning-loop report; BUILD.md should pick one name.

### 8.2 Configuration (`eileen.toml`, commented)

```toml
[logging]
# What goes into the log files in %LOCALAPPDATA%\eileen\logs (tracing filter syntax).
file = "info"
console = "warn"            # only when Eileen is started from a terminal
keep_days = 14
max_total_mb = 500
include_personal = false    # transcripts, window titles and paths in log files

[flight]
# The in-memory record of the last few minutes, saved only when something goes wrong.
filter = "eileen=debug,warn"
minutes = 10
max_mb = 32
audio_seconds = 0           # 0 keeps audio out of reports; at most 120

[incidents]
keep_days = 30
max_total_mb = 1024
minidumps = true
restart_core_on_hang = true

[recording]
# Off unless you say "Eileen, record this session" or set enabled = true.
enabled = false
audio = false
keep_days = 7
max_total_mb = 2048

[otel]
# Optional export to an OpenTelemetry viewer on this PC. Needs a build with the otel feature.
enabled = false
endpoint = "http://127.0.0.1:4318"
include_personal = false
forward_claude = false      # also send each Claude worker's own telemetry here

[debug]
slow_span_ms = { "brain.turn" = 3000, "dispatch.call" = 2000, "speak" = 1500 }
developer_panel = false     # stage timings on the HUD

[dev]
max_lsp_workers = 1         # Claude workers with rust-analyzer at once, on this PC
```

A change to `logging.file` takes effect at once through the `reload` handle. A spoken "turn on detailed logging for the desktop" can set `eileen_desktop=debug` for 30 minutes through the fast-path grammar, then revert [inferred].

### 8.3 Data directory additions

```
%LOCALAPPDATA%\eileen\
  logs\                  <process>.<date>.jsonl
  incidents\             <time>_inc_<id>\ ...; pending\ for dumps not yet assembled
  recordings\            rec_<id>\ manifest.toml, inputs.jsonl.zst, audio\
  blobs\                 <2 hex>\<blake3>.zst, shared with learning\
  run\                   guard socket, process ID files
```

## 9. Crates and tools, versions to verify at build time

From the crates.io API and GitHub release pages on 2026-10-20 [primary]. None of these were compiled for this report.

| Crate or tool | Version (date) | License | MSRV | Use | Where |
|---|---|---|---|---|---|
| tracing | 0.1.44 (2025-12-18) | MIT | 1.65 | spans and events | everywhere (already chosen) |
| tracing-subscriber | 0.3.23 (2026-03-13) | MIT | 1.65 | features `registry`, `fmt`, `json`, `env-filter`, `tracing-log`, `smallvec` | `eileen-trace` |
| tracing-appender | 0.2.5 (2026-04-17) | MIT | 1.63 | daily files, non-blocking writer | `eileen-trace` |
| tracing-error | 0.2.1 (2024-11-29) | MIT | 1.63 | `SpanTrace` | `eileen-trace`, `eileen-crash` |
| crash-handler | 0.8.1 (2026-09-25) | MIT OR Apache-2.0 | | exception, invalid-parameter and purecall handlers; `simulate_exception` | `eileen-crash` |
| minidumper | 0.11.0 (2026-07-20) | MIT OR Apache-2.0 | 1.85 | dump IPC client and server | `eileen-crash`, `eileen-guard` |
| minidump-writer | 0.13.0 (2026-07-20) | MIT | 1.88 | through minidumper; external hang dumps | `eileen-guard` |
| await-tree | 0.3.1 (2025-06-05) | Apache-2.0 | | async task trees, features `tokio`, `serde` | `eileen-core` |
| rtrb | 0.4.0 (2026-08-17; crates.io lists 0.3.5 as newest, check) | MIT OR Apache-2.0 | 1.38 | real-time-safe ring for audio ticks | `eileen-audio` |
| secrecy | 0.10.3 (2024-10-09) | Apache-2.0 OR MIT | 1.60 | secret types | `eileen-config` |
| blake3 | 1.8.7 (2026-08-20) | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | | blob keys; consider the `pure` feature to skip its C and assembly build | `eileen-record` |
| zstd | 0.14.3 (2026-10-16) | BSD-3-Clause | 1.64 | blob and input-log compression (also in the learning loop) | `eileen-record` |
| hound | 3.5.1 (2023-09-25) | Apache-2.0 | | WAV lanes | `eileen-record` |
| tracing-opentelemetry | 0.34.0 (2026-09-23) | MIT | 1.75 | optional | `eileen-otel` |
| opentelemetry, opentelemetry_sdk, opentelemetry-otlp | 0.33.1 (2026-10-09) | Apache-2.0 | 1.75 | optional, `http-proto` with blocking reqwest | `eileen-otel` |
| tracing-etw | 0.3.0 (2026-10-17) | MIT | 1.82 | optional, experimental | `eileen-trace` feature `etw` |
| console-subscriber | 0.5.0 (2025-10-30) | MIT | 1.74 | optional, `tokio_unstable` | `eileen-cli` feature `console` |
| tokio-metrics | 0.5.2 (2026-08-28) | MIT | 1.70 | optional | `eileen-core` |
| tracing-test | 0.2.6 (2026-02-07) | MIT | | optional dev-dependency | tests |
| tracing-tracy | 0.12.0 (2026-08-25) | MIT OR Apache-2.0 | 1.70 | later, speech profiling | not v0.1 |
| tracing-chrome, tracing-perfetto | 0.7.2 (2024-03-15), 0.1.5 (2025-01-02) | MIT | | not needed (own converter) | |
| assert_no_alloc | 1.1.2 (2021-08-03) | BSD-1-Clause | | reference only; license not on the allow list | |
| minidump-stackwalk (tool) | 0.27.0 (2026-08-11), Windows zip | MIT | | stack walks | `cargo xtask symbolicate` |
| dump_syms (tool) | 2.3.9, Windows zip | Apache-2.0 | | PDB to Breakpad symbols | release CI |
| cargo-modules (tool) | 0.28.0 (2026-10-18), no prebuilt | MPL-2.0 | 1.98 | module maps | nightly CI |
| rust-analyzer (tool) | 2026-10-19 release, via rustup | MIT OR Apache-2.0 | | LSP for workers | `eileen-dev` plugin |
| Rust Glancer (tool) | 0.3.0 (2026-10-01), Windows build | Apache-2.0 OR MIT | | candidate low-memory LSP | measure |
| codegraph (tool) | v1.6.2 (2026-10-03), Windows zip | MIT | | optional code-graph MCP | `eileen-dev` `.mcp.json`, opt-in |
| Perfetto trace processor (tool) | v58.2 (2026-08-24), Windows zip | Apache-2.0 | | SQL over timelines | by hand, agents |
| otel-desktop-viewer (tool) | release with a Windows build | Apache-2.0 | | local OTLP viewer | owner installs |
| rmcp | 3.6.0 | Apache-2.0 | 1.88 | `_meta` trace context accessors (SEP-414) | already chosen |

New licenses for `deny.toml`: none beyond the allow list (MPL-2.0 and GPL tools run in CI, they are not linked). Run `cargo deny check` with `--all-features` in the nightly job, since `otel` and `etw` pull crates the default build does not (for example `tracing-etw`'s `hashers`).

## 10. Changes for BUILD.md and the work packages

**WP0 (workspace, contracts, quality gates) ships the scaffolding every later package inherits:**

1. `eileen-contracts::{ids, trace, incident, record}` with snapshot tests: prefixed ULID newtypes, `TraceCtx`, `Envelope`, `IncidentManifest`, `RecordingManifest`, `InputRecord`, `Checkpoint`, `Port`.
2. `eileen-trace` with `init`, `names` (seeded with the spans of section 2.5), `schema` and its checker, `pii`, `redact`, `ring`, `stats`, `chrome`, `ctx` and `test_support` (an in-memory capture layer with `assert_schema_ok`). Every later crate calls `eileen_trace::init` and adds rows to `names` and `schema` for its own spans.
3. `eileen-crash` (panic hook plus crash handler plus guard client) and `eileen-guard` (dump server and core watchdog), with a CI test that crashes a test binary on purpose (Embark's `sadness-generator` exists for this) and checks a dump appears, and a test that hang-dumps a sleeping child.
4. `clippy.toml`: add `tokio::spawn` (and `tokio::task::spawn`) to `disallowed-methods`, pointing to `eileen_core::task::spawn`.
5. `[workspace.metadata.eileen.processes]` in the root manifest, `[package.metadata.eileen]` in each crate template, `cargo xtask codemap` and `--check` in the PR lane, `docs/CODEMAP.md` generated from the first commit.
6. Generated `docs/reference/telemetry.md`, `events.md`, `mcp-tools.md`, checked by tests.
7. The redaction canary test and the fixture tidy check (no `origin = "user"` fixtures, no unmarked WAVs).
8. Nightly lane additions: `cargo check --features otel,etw`, `cargo deny check --all-features`, cargo-modules artifacts, `cargo build --timings`, private-items docs. Release lane: `dump_syms` symbols zip next to the PDBs.
9. `tools/claude-plugins/eileen-dev/` with `.lsp.json`, `.mcp.json` and the three skills, validated by `claude plugin validate --strict` (alongside the claude-mods report's `eileen` plugin).
10. `docs/DEBUGGING.md` from section 7, and a "Tracing" section in the crate `CLAUDE.md` template: which spans the crate owns, which `pii.` fields it emits, how to test its spans.

**Other packages:**

- **Core package**: `task::spawn`, the lag probe and watchdogs, `incident` assembly, the janitor, ports and the input tap, supervisor hooks (ask for the ring, hang-dump through the guard, then kill), `events` table gains `seq` and `TraceCtx` columns, the fast-path grammar for "something went wrong" and "record this session".
- **Voice package**: `eileen-speech` mints `turn_` IDs, puts `TraceCtx` in every frame header, and emits the speech spans; `eileen-audio` gets the heartbeat, the tick ring, the audio lanes and the allocation check.
- **Desktop and apps packages**: read `_meta.traceparent` in every tool handler; emit `desktop.capture` with the snapshot blob and `desktop.action` with the rung; answer the supervisor's ring request; log the `_meta` the shim receives from the CLI on the first call.
- **Harness package**: `TRACEPARENT` and `EILEEN_JOB_ID` in each worker's environment; `worker.session` and `worker.tool_use` spans from stream-json; join hooks to dispatcher calls; `otel.forward_claude`; add `--plugin-dir eileen-dev` only for jobs in an Eileen worktree and only up to `dev.max_lsp_workers`.
- **Learning package**: use `eileen-trace::redact` and `eileen-record::BlobStore` instead of its own; key trajectory steps by `call_` ID.
- **MCP package**: `eileen-debug` server; rmcp clients set `_meta.traceparent` on every request.
- **HUD package**: recording indicator, developer panel, "Save a report?" after "that's wrong".
- **Simulation harness (engineering-practice report)**: implement the port traits; an L1 replayer with the divergence report; run `schema::check` on every simulated scenario.

## 11. Pitfalls

1. **Span context does not cross channels or processes.** Without `TraceCtx` in envelopes and frames, half the log lines of a turn have no `turn_id`.
2. **A bare `tokio::spawn` drops the current span.** Hence the disallowed method and the wrapper.
3. **`tokio_unstable` changes `RUSTFLAGS` and rebuilds everything**; keep `tokio-console` in its own target directory.
4. **The `tracing` macros in an audio callback allocate.** Use the tick ring.
5. **`tracing-appender` drops lines when its buffer fills** (`lossy` is the default); watch `logs.dropped`, and never rely on the file log for the last lines before a crash (that is the ring's and the dump's job).
6. **`tracing-appender` has no size-based rotation**; the janitor enforces the cap.
7. **Fail-fast exits skip every in-process handler** (Rust's `abort` on Windows). Request the dump from the panic hook first; the guard reports the rest.
8. **Dumping your own process during a real exception is unreliable**; dump from the guard.
9. **Minidumps hold memory contents**, so they are personal data; normal dump type, never in a share pack by default.
10. **Perfetto needs nested slices**; async spans must be laid on their own tracks by the converter.
11. **Overlapping IDs in different formats confuse people and agents**; always print IDs with their prefix.
12. **The official rust-analyzer plugin runs `cargo check` on every save** with default settings; on this PC that defeats the build-slot guard.
13. **rust-analyzer's memory can exceed what this PC has free**; one LSP worker at a time.
14. **The claude CLI's `TRACEPARENT` is read at process start**, so it parents per-job workers, not the warm brain's individual turns.
15. **The CLI's OpenTelemetry tracing is beta**; its span names may change between releases.
16. **The OpenTelemetry GenAI and MCP conventions are in Development status** and have moved repositories; keep Eileen's own names authoritative and map at export.
17. **A replay that diverges on `Ids` or `Clock` means a port was bypassed** (a direct `Ulid::generate()` or `SystemTime::now()` in core logic); add both to `disallowed-methods` for the core crates, with the port implementation as the one `#[expect]`.
18. **codegraph's telemetry is on unless turned off**, and its installer edits agent configuration; Eileen wires its MCP server through the plugin's `.mcp.json` with telemetry off and never runs the installer for the owner.

## 12. Open risks and measurements to take

- **Overhead of the always-on DEBUG ring.** Formatting every DEBUG event costs CPU. Target: under 1 percent of one core while idle and listening, and under 3 percent during a busy turn, measured with `scripts\measure-idle.ps1` with and without the ring. If it is over, record compact binary records and format at dump time.
- **Ring sizing.** Events per minute in a real session are unknown; 32 MB for 10 minutes is a guess. Log ring fill rate in `eileen debug stats` from day one.
- **await-tree overhead** in Eileen's hot paths is unmeasured; keep it on for jobs, the dispatcher and sidecar calls, and measure before instrumenting more.
- **Hang dumps of a process stuck in cross-process COM** have not been tried on this PC. `MiniDumpWriteDump` on a process whose thread is blocked in a UIA call should work (it suspends threads and reads memory from outside), but test it against a deliberately hung test sidecar in WP0's CI test.
- **Guard outside the Job Object**: confirm that a child spawned by the core without being assigned to the job survives the core's termination on Windows 11 and on the CI runner, and that the guard never becomes an orphan (it must exit when the core exits cleanly and after its restart budget is spent).
- **L2 determinism**: ONNX Runtime with several intra-op threads can give slightly different floats; check whether STT finals and turn decisions replay identically with fixed thread counts.
- **Whether the claude CLI sends `_meta` on stdio MCP calls**, and whether stream-json carries anything that ties a warm-brain turn to the CLI's own `prompt.id`. Check on the first harness build.
- **rust-analyzer and Rust Glancer memory on Eileen's workspace**, with the `windows` crate and its features. Measure working set after indexing with `measure-idle.ps1`; pick the LSP by the number.
- **Redaction cost per log line** with the prefilter; measure lines per second.
- **Plan usage of `eileen-debug` and `replay_core`** when an agent calls them repeatedly is small (local work, no model), but the Markdown they return goes into a worker's context; keep `trace_get` output bounded (for example 400 lines, with a pointer to the full file).
- **Recording disk use**: 2 GB cap with about 19 GB free on C: today; the janitor must keep total Eileen data under a configured ceiling.

## Sources

Registries and releases, read 2026-10-20:

- crates.io API, per crate: `https://crates.io/api/v1/crates/<name>` (tracing, tracing-subscriber, tracing-appender, tracing-error, tracing-chrome, tracing-perfetto, tracing-opentelemetry, opentelemetry, opentelemetry_sdk, opentelemetry-otlp, console-subscriber, tokio-metrics, await-tree, async-backtrace, crash-handler, minidumper, minidump-writer, minidump-stackwalk, sentry, sentry-rust-minidump, secrecy, tracing-etw, tracing-tracy, rtrb, assert_no_alloc, hound, blake3, zstd, tracing-test, human-panic, logroller, cargo-modules, cargo-depgraph, veil, turmoil, madsim)
- rust-minidump releases: https://github.com/rust-minidump/rust-minidump/releases
- dump_syms: https://github.com/mozilla/dump_syms
- Perfetto releases: https://github.com/google/perfetto/releases
- codegraph releases: https://github.com/colbymchenry/codegraph/releases
- Rust Glancer: https://github.com/rust-glancer/rust-glancer
- SCIP: https://github.com/scip-code/scip/releases
- rust-analyzer releases: https://github.com/rust-lang/rust-analyzer/releases

Source code read:

- Embark crash handling: https://github.com/EmbarkStudios/crash-handling (`crash-handler/src/windows.rs`, `crash-handler/src/windows/state.rs`, `minidumper/src/ipc/client.rs`, `minidumper/examples/diskwrite.rs`, changelogs)
- minidump-writer Windows: https://github.com/rust-minidump/minidump-writer/blob/main/src/windows/minidump_writer.rs
- Zed crash handling (GPL, design reference only): https://github.com/zed-industries/zed/blob/main/crates/crashes/src/crashes.rs
- rmcp trace context: https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/src/model/meta.rs and `crates/rmcp/tests/test_trace_context.rs`
- LiveKit Agents telemetry: https://github.com/livekit/agents/blob/main/livekit-agents/livekit/agents/telemetry/trace_types.py and https://github.com/livekit/agents/blob/main/tests/trace_schema.py
- Codex OpenTelemetry crate: https://github.com/openai/codex/tree/main/codex-rs/otel
- rust-analyzer configuration and CLI: https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/config.rs and `crates/rust-analyzer/src/cli/flags.rs`
- rust-analyzer architecture document: https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md
- Rust standard library, Windows abort: https://github.com/rust-lang/rust/blob/master/library/std/src/sys/pal/windows/mod.rs
- Official rust-analyzer plugin entry: https://github.com/anthropics/claude-plugins-official/blob/main/.claude-plugin/marketplace.json

Documentation:

- claude CLI monitoring and OpenTelemetry: https://code.claude.com/docs/en/monitoring-usage
- claude CLI plugin manifest (`lspServers`): https://code.claude.com/docs/en/plugins-reference
- claude CLI code intelligence plugins: https://code.claude.com/docs/en/plugins/code-intelligence
- claude CLI tools reference (LSP tool): https://code.claude.com/docs/en/tools-reference
- MCP SEP-414, trace context in `_meta`: https://modelcontextprotocol.io/seps/414-request-meta
- OpenTelemetry GenAI conventions (moved): https://opentelemetry.io/docs/specs/semconv/gen-ai/mcp/ and https://github.com/open-telemetry/semantic-conventions-genai
- tracing-appender builder: https://docs.rs/tracing-appender/0.2.5/tracing_appender/rolling/struct.Builder.html
- Pipecat OpenTelemetry: https://docs.pipecat.ai/server/utilities/opentelemetry
- Go flight recorder: https://go.dev/blog/flight-recorder
- Perfetto, other formats: https://perfetto.dev/docs/getting-started/other-formats and embedding: https://perfetto.dev/docs/visualization/embedding-the-ui
- Time Travel Debugging command line: https://learn.microsoft.com/en-us/windows-hardware/drivers/debuggercmds/time-travel-debugging-ttd-exe-command-line-util
- otel-desktop-viewer: https://github.com/CtrlSpice/otel-desktop-viewer
- await-tree: https://github.com/risingwavelabs/await-tree
- tokio-metrics: https://github.com/tokio-rs/tokio-metrics
- tokio-console subscriber: https://github.com/tokio-rs/console/tree/main/console-subscriber
- tracing-etw: https://github.com/microsoft/tracing-etw
- Serena license: https://github.com/oraios/serena/blob/main/LICENSE
- GitNexus license: https://github.com/abhigyanpatwari/GitNexus/blob/main/LICENSE
- codegraph: https://github.com/colbymchenry/codegraph

Secondary:

- rust-analyzer memory reports: https://users.rust-lang.org/t/rust-analyzer-ram-consumption/50420 and https://github.com/rust-lang/rust-analyzer/discussions/20871
- Temporal replay testing, community documentation: https://cljdoc.org/d/io.github.manetu/temporal-sdk/2.3.0/doc/replay-testing
