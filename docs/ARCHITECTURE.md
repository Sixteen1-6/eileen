# Architecture

This is the map of Eileen. Read it before changing code. It describes only what changes rarely; the detailed v0.1 specification, contracts and work packages are in [BUILD.md](BUILD.md), each crate's rules are in its crate doc (the `//!` comment at the top of `src/lib.rs`), and the reasons are in [DECISIONS.md](DECISIONS.md). Last revisited 2026-10-20, after the reviews of the v0.1 specification, its first verification round and the owner's answers.

WP0 moves this file to the repository root and adds the check that its code map lists exactly the workspace crates and that every path and type it names exists (BUILD.md section 4b). The Python-era design that this file used to hold is in git history at the tag `python-scaffold-final`; decisions D17 to D69 record what replaced it. How the repository itself is built, written for other projects to adopt, is in `docs/PLAYBOOK.md` (WP0 writes it, with reusable files in `templates/`).

## Bird's eye view

Eileen listens whenever the microphone is not muted, works out what the user means, and either acts at once on the PC or starts jobs that run in parallel while the conversation continues. It learns from every request, so the next request of the same kind is faster and eventually needs no model.

- **Input:** audio from the microphone, and the system's own output as the echo reference; hotkeys, tray and HUD clicks; typed text; what is on screen (window state, the UI Automation tree, app scripting state); events from connectors; the output of Claude sessions.
- **Output:** speech; the HUD; actions on the PC through tools; messages to Claude sessions running on the owner's own plan.
- **State:** the job registry and event log (`eileen.db`), learned skills, trajectories, the user model and the episodic ledger (`learn.sqlite`, from which each Claude session gets its own skill snapshot), settings with their version history, recordings and incidents. All of it lives on the user's PC.

One core process owns the microphone loop, the tray and hotkeys, the event bus, jobs, permissions, the learning loop's decisions and a supervisor. Everything that can hang or crash (speech models, UI Automation, app COM, the HUD, every Claude session) runs in a child process the supervisor can kill. The core decides everything in one deterministic state machine, `Core::step`, which the simulator drives in tests.

## Entry points

- `crates/eileen-cli/src/main.rs`: the `eileen` binary. Starts the runtime, the components and the core; also every subcommand.
- `Core::step` in `crates/eileen-core`: every decision starts here. Read its `Input` and `Effect` enums in `eileen-contracts` first.
- Each sidecar's `main.rs`: `eileen-speech`, `eileen-desktop`, `eileen-apps`, `eileen-hud`, and the small `eileen-shim` and `eileen-guard`.
- `crates/eileen-sim`: the fastest way to watch the core run, and to reproduce a bug from a seed; `crates/eileen-replay` reproduces it from a recording.

## Processes

| Process | Holds | Talks to the core over |
|---|---|---|
| `eileen.exe` | Audio I/O, loopback and echo cancellation, the tray, hotkeys and Raw Input, `Core::step`, the drivers, store, bus, loopback endpoint, dispatcher, supervisor, the learning store | n/a |
| `eileen-speech.exe` | VAD, streaming ASR, end of turn, TTS, cloned voices (Pocket TTS, Chatterbox), speaker check, the intent embedding model if adopted | binary frames over a named pipe |
| `eileen-desktop.exe` | UI Automation, capture, input, app index and launch, windows, DesktopState, text insertion | MCP over stdio |
| `eileen-apps.exe` | SMTC, Office and Explorer COM, the Resolve bridge, recent work and media history, face finding, tracking and sharpness | MCP over stdio |
| `eileen-hud.exe` | The overlay, the clickable strip, the live thumbnail | JSON lines over stdio |
| `claude -p` | The brain (warm, plus a hot spare), a small warm pool of worker sessions, each worker job, the librarian, the dictation scribe | stream-json over stdio, HTTP hooks to the loopback endpoint |
| `eileen-shim.exe` | One per Claude session: the `eileen` MCP server, forwarding to the dispatcher | named pipe with a per-session token; the dispatcher checks the caller's process and Job Object |
| `eileen-guard.exe` | Crash and hang minidumps, the core watchdog; outside the Job Object | local socket |

## Code map

| Crate | Kind | Platform | One line |
|---|---|---|---|
| `eileen-contracts` | lib | neutral | Every type that crosses a process, pipe, file or socket, the ports, and the recording, scenario and fleet formats |
| `eileen-stdx` | lib | neutral | Clock and ID ports, named threads and sleep, the seeded RNG, assertions, fault points, checked casts |
| `eileen-trace` | lib | neutral | Tracing setup, span names and their parent table, redaction and the scenario privacy rules, the flight recorder ring |
| `eileen-rt` | lib | neutral | The named task wrappers, bounded channels |
| `eileen-platform` | lib | neutral | The traits each operating system implements, including app hooks, secrets and system volume |
| `eileen-supervisor` | lib | neutral, Windows module | Spawning children in kill-on-close Job Objects; process QoS |
| `eileen-buildguard` | lib | neutral, Windows module | Build slots admitted by free memory, shared by `cargo xtask` and build-heavy coding jobs |
| `eileen-core` | lib | neutral | The sans-IO state machine that makes every decision |
| `eileen-record` | lib | neutral | Recordings, checkpoints, the blob store, trimming |
| `eileen-replay` | lib | neutral | Strict and scenario replay, the expectation matcher, the recorded-model responder |
| `eileen-sim` | lib, dev | neutral | Deterministic simulation, invariants, exhaustive turn-taking |
| `eileen-runtime` | lib | neutral | Drivers that carry out the core's effects: store, bus, loopback endpoint, dispatcher, sidecars, incidents |
| `eileen-mcp` | lib | neutral | rmcp glue, the shim protocol, the core-side tool handlers |
| `eileen-crash` | lib | neutral, Windows module | Panic hook and crash handler client |
| `eileen-shell-win` | lib | Windows | Tray, global hotkeys with the physical-key check, Raw Input, toasts, the Start menu and sign-in entries, the credential store |
| `eileen-scenarios` | lib | neutral | Saving, sharing and submitting scenarios; the real-desktop runner |
| `eileen-cli` | bin | neutral | The `eileen` binary and composition root |
| `eileen-shim` | bin | neutral | Per-session stdio MCP server forwarding to the dispatcher |
| `eileen-guard` | bin | neutral, Windows module | Minidump server and core watchdog |
| `eileen-harness` | lib | neutral | Claude stream-json driver, sessions and the warm pool, the brain backends and their benchmark, model aliases, worktrees; slots for Codex, DeepSeek, ACP and a local model |
| `eileen-stub-harness` | bin, dev | neutral | A fake `claude -p` that plays recorded scripts with faults |
| `eileen-audio` | lib | neutral logic, Windows devices | Capture, playback, loopback, echo cancellation, mute, barge-in accounting, WAV sources and sinks |
| `eileen-audio-win` | lib | Windows | Audio endpoints, loopback and MMCSS |
| `eileen-speech` | lib and bin | neutral logic, Windows models | The speech sidecar; every stage (voice activity, recognition, turn detection, voices) behind a plug-in trait |
| `eileen-dictation` | lib | neutral | Dictation cleanup rules |
| `eileen-desktop` | lib and bin | neutral logic | The desktop sidecar's tools and safety rules |
| `eileen-desktop-win` | lib | Windows | UI Automation, capture, input, app index, text insertion and system volume on Windows |
| `eileen-apps` | lib and bin | neutral logic | The apps sidecar's tools |
| `eileen-apps-win` | lib | Windows | SMTC, the app hooks (COM targets, the Resolve process gate), recent-work sources |
| `eileen-intent` | lib | neutral | The layered intent resolver, the grammar, the skill matcher, introspection |
| `eileen-skills` | lib | neutral | The learning loop's decisions: template grammar, linter, provenance, ladder, shadow agreement, replay state machine |
| `eileen-learn` | lib | neutral | The learning loop's IO: trajectories, the skill store and verified snapshots, the librarian, the user model and phrasings, route statistics and proposals, the ledger, forget |
| `eileen-config` | lib | neutral | TOML configuration, the settings model, versions, profiles, secrets through the platform trait |
| `eileen-hud` | lib and bin | neutral layout | The HUD sidecar |
| `eileen-hud-win` | lib | Windows | The HUD renderer: DirectWrite, the overlay, the strip, the thumbnail |
| `eileen-fleet` | lib | neutral | The opt-in test fleet runner and result upload |
| `eileen-e2e` | lib and bin, dev | neutral, Windows fixtures | Benchmarks, soak, fleet ingest and clustering |
| `xtask` | bin, tool | neutral | `cargo xtask`: CI, repository checks, the build guard, generators |
| `repo-tools` (`tools/repo-tools`) | bin, tool | neutral | The `syn`-based checks and the generated code map |

### `eileen-contracts`

Every serialized type: IDs, `TraceCtx` and `Envelope`, `Input`, `Effect`, `BusEvent`, DesktopState, `HudState`, tool argument structs and `ToolSpec`, connector manifests, learning types, configuration types, the scenario, recording and fleet formats, the ports.
**Architecture invariant:** this is the only crate whose types are serialized across a boundary. Other crates convert at their edge, so internal types change freely.
**Architecture invariant:** no IO, no async runtime. Text from outside Eileen is wrapped as `Untrusted` or `Personal`.
**API boundary:** wire formats, the event log, the skill and recipe format, the config schema, the scenario schema. All are versioned and snapshot-tested.

### `eileen-core`

`Core::step(now, rng, input, effects)`: turns, the addressee gate, intent dispatch, the brain turn and its authority, jobs and sessions, memory admission, routing, the permission gate, taint and approvals, announcements, barge-in and what the user heard, the input lock and leases, the journal, the settings flow, the first run, plan-usage policy, supervisor policy, and the learning loop's decisions through the `SkillEngine` port.
**Architecture invariant:** sans-IO. No clock, no OS randomness, no IO, no threads; every queue bounded; ordered maps wherever order can reach an effect.
**Architecture invariant:** depends only on `eileen-contracts`, `eileen-stdx` and `eileen-trace`. The resolver, dictation cleaner, settings validator and skill engine arrive through ports.
**Architecture invariant:** an action above tier 1 needs an approval built from the user's own input: a physical click, an approval hotkey pressed on a real keyboard, or speech in the approval window that contains the action word (and matches the enrolled voice once the speaker check exists). That type cannot be made from screen text, model output, tool results, a command line or a learned skill.

### `eileen-runtime`, `eileen-mcp`, `eileen-cli`, `eileen-shell-win`

The drivers that carry out effects and turn the world into inputs, and the binary that composes them. They hold no decisions.
**Architecture invariant:** only `eileen-mcp` knows rmcp. Every child process starts through `eileen-supervisor` (the build guard, the guard's restart of the core and the development tools are the listed exceptions); every task through `eileen-rt`, every thread through `eileen-stdx`.
**Architecture invariant:** a token alone proves nothing. The dispatcher and the loopback endpoint check the calling process and its Job Object; the owner's own Claude sessions connect only from outside every Eileen job, with their own token and only the session tools.
**API boundary:** the loopback endpoint (hooks, state, control) and the shim protocol. The bus token reaches status and control only; nothing reachable with it can approve.

### `eileen-harness`

**Architecture invariant:** the only crate that knows vendor CLI flags and protocols. It never reads vendor credential files and never runs a login; the brain gets a constructed allowlist environment, workers the user's environment minus a denylist, and every session per-session flags only.
**API boundary:** `WorkerDriver` (start a session, bind a job) and `BrainBackend` (built through `brain_for`). Claude is built; Codex app-server, DeepSeek, ACP and a local model are slots behind the same traits. Config names models by alias; command lines carry the full ids resolved per CLI version.

### Sidecars: `eileen-speech`, `eileen-desktop`, `eileen-apps`, `eileen-hud`

**Architecture invariant:** each may crash or be killed at any time and holds no state that is not rebuilt on start.
**Architecture invariant:** server logic is platform-neutral and tested on Linux; operating-system calls and native libraries live in the `-win` crates and Windows target tables behind `eileen-platform` traits.
**Architecture invariant:** the user's hands win. The desktop server stops synthetic input within one event when physical input arrives, and the HUD counts only physical clicks.
**API boundary:** the speech pipe frames, the MCP tool contracts, the HUD lines.

### `eileen-intent`, `eileen-skills`, `eileen-learn`, `eileen-config`

**Architecture invariant:** learned skills and settings can never raise a permission tier or skip a confirmation. A skill replays through the same dispatcher and gate as a worker.
**Architecture invariant:** provenance is computed, never taken from a model; text seen on screen or a page enters a skill only after the user approves it, and a draft from a job that read free text from outside is reviewed whole.
**Architecture invariant:** learned data stays on the PC; "forget" removes the skill, its versions and its evidence from every store.

### `eileen-replay`, `eileen-scenarios`, `eileen-fleet`

Shipped, because users replay incidents, save and share scenarios and run the fleet. **Architecture invariant:** nothing leaves the PC without a preview and the user's approval; a redacted scenario is replayed locally before it is offered.

### `eileen-sim`, `eileen-e2e`, `eileen-stub-harness`

Development only. The simulator owns the invariants; `eileen-e2e` owns benchmarks, soak and fleet ingest.

## Cross-cutting concerns

**Process isolation and supervision.** Every child runs in a kill-on-close Job Object, so killing the core kills its children. Apps Eileen opens for the user are not its children: the shell starts them (AUMID activation or the shell's own `ShellExecute`), so they live outside every Eileen job, carry none of Eileen's environment and survive a restart; a worker's job never allows breakaway. Every cross-process call has a deadline and every sidecar a heartbeat; a hung UI Automation thread is abandoned and replaced first, and only a repeatedly hung sidecar is hang-dumped, killed and restarted; the caller gets a typed error. Sidecars and Claude sessions are long-lived; nothing is spawned per request.

**Memory.** The core samples available memory, commit headroom and paging, and admits a job when commit headroom stays safe and either memory stays above a reserve, paging has been low for a while, or the user says "start it anyway"; it drops the warm pool and the brain's spare first, keeping the deep-answer session longest. The paging limit is learned from the PC's own week of paging, so a machine that always pages a little still admits work on an ordinary minute. Build-heavy coding jobs share the build slots with `cargo xtask`. A low-memory profile picks smaller models, and on a PC with little free memory the warm pool and the hot spare are usually absent, which Eileen says.

**Events, request IDs and the event log.** A `turn_` ID is minted when a request begins and travels on every bus event, frame, tool call, span and record. Across MCP it is `_meta.traceparent`; into Claude sessions it is `TRACEPARENT` and `EILEEN_SESSION_ID`. One newline-delimited event format serves the flight recorder, the simulator, the end-to-end harness and the learning record.

**Time, randomness and determinism.** The core gets `MonoTime` and a seeded RNG as arguments. IDs come from a port. A recording of port values with its checkpoints replays the core exactly on the commit that made it; scenario replay checks a scenario's expectations on any later commit.

**Hearing and turn-taking.** The echo reference is the system's own output, so video and music are cancelled too. The addressee gate uses the name, a conversation window that stays open while the user is engaged (inside it, commands and replies pass and other speech needs the local score), the command grammar and, when the user is alone, a local score with the brain as tie-breaker. A follow-up inside the window counts only when the intent resolver ties it to something Eileen just said, and a voice much quieter than the level the first run calibrated is not taken as addressed. Media-control and stop words work without the name while other audio plays. During a call Eileen answers only to its name and stays silent, showing its replies on the HUD. Barge-in ducks at the first 32 ms window and pauses by the third; a backchannel resumes; otherwise synthesis is cancelled and the samples actually played map back to the text the user heard.

**Permissions and untrusted input.** Tiers 0 to 3 (observe, reversible, consequential, blocked). Every tool declares its tier per op; undeclared means tier 2. Workers are contained by their folder, a path guard (any `.git` path asks first), a shell policy, per-job taint and one URL rule that the brain, dispatched tasks and tainted workers share. Screen text, page text, tool results and model output are data and can never approve, raise a tier, start a job from an ambient turn or become a skill without the user. Another person's voice can say the action word too, so once the speaker check is enrolled a voice approval must also match the user's voice; until then the HUD and the hotkey are the approvals only the user can give. Credentials, payments and security settings are never Eileen's to touch.

**Error handling and assertions.** Operating errors are values. Programmer errors are `never!` and `always!` in the core (log, count, dump the flight recorder, keep listening) and plain assertions in sidecars, tests and the simulator. Builders that can fail per item return partial results with diagnostics, so one unreadable window never loses the whole DesktopState.

**Testing.** Unit, property and snapshot tests everywhere; one integration test binary per crate; the core under deterministic simulation with invariants and fixed seeds on pull requests; fuzzing of every untrusted parser; end-to-end runs with recorded or synthesized audio and a stub harness; the owner's benchmarks as timed A/B scenarios; soak tests for the always-on process; and a scenario corpus that grows from real use on many PCs. Tests that need the owner's PC or real Claude live in `owner_pc::` and `real_claude::` modules, which the default test profiles exclude.

**Observability and debugging.** JSON log lines per process with file and line; an in-memory flight recorder of the last minutes, dumped into an incident on a panic, a crash, a watchdog trip, a spoken "something went wrong", a hotkey or a tray click; minidumps from the guard; `eileen trace`, `eileen incident`, `eileen replay`. User content travels only in `pii.` fields, which each sink handles by policy; speech not addressed to Eileen is never written to disk.

**Configuration.** TOML with comments in `%LOCALAPPDATA%\eileen\config`, generated defaults and schema, every setting reachable by voice and on the HUD, every change versioned and undoable, profiles, and safety bounds no setting can cross; safety-relevant settings change only on the user's own voice or click.

**Serialization and stability.** A format becomes stable when its first user appears: the event log, the skill format, the scenario format and the config schema carry a version from the first commit, and snapshot tests make every change a reviewed diff.

**Generated files.** `docs/CODEMAP.md`, `docs/reference/*`, the per-crate Claude rules, the config schema and default file, and the contract schemas are committed, derived only from the tree, regenerated by `cargo xtask codegen`, and checked fresh in CI. The scenario catalogue and benchmark results depend on runs, so they are rendered to the project's Pages site, never committed.

**Performance budgets.** End of speech to first sound about 0.5 s for the fast path; a brain turn targets 1.5 s to the first word on a quiet machine with an earcon at once; barge-in at most 150 ms; idle listening a few percent of one core. BUILD.md section 4.3 has the per-stage table with a typical-load column.

**Platform boundaries.** Neutral crates build and test on Linux in CI from the first commit. Windows code lives in `-win` crates, `cfg(windows)` modules and Windows target tables behind `eileen-platform` traits; a Linux port adds `-linux` crates.

**Connectors.** The PC is the first connector (`pc.desktop`, `pc.apps`). The phone, the home hub and other devices are new connectors with the same manifests, tiers, job model and learning loop. Room-sensing connectors are opt-in per device and local only.

**Learning.** Every tool call is recorded at the dispatcher, fast-path and brain actions included; successful jobs become native skills each Claude session gets in its own verified snapshot; proven skills replay with a check after every step and hand off to a worker on the first failed check; phrasings the user kept run from the fast path (a paraphrase only when it is close enough and names the same app or file); the user's own corrections become facts with no model; the user can ask what was learned, correct it and make Eileen forget it.

## Glossary

- **Turn:** one request from the user, voice or not; its ID is the request ID. A turn is addressed (from the user's own input) or ambient (from a job event or a command line).
- **Job:** one unit of work run by a worker, a replay, the fast path or the core itself (an app task such as a render).
- **Session:** one running Claude process; a job is bound to a session, which may come from the warm pool.
- **Worker:** a Claude session (later Codex, Grok, DeepSeek) running a job; named by route only.
- **Brain:** the warm Claude session that answers, dispatches and runs deterministic actions; it never drives the mouse.
- **Fast path:** a deterministic answer or action with no model call.
- **Skill:** a learned `SKILL.md` folder; a **recipe** is its optional list of tool calls with checks.
- **Connector:** a capability provider with a manifest of tools, tiers and events.
- **Sidecar:** a supervised child process that serves tools or speech.
- **Scenario:** a test written as data, from a developer or from real use.

## Tracing a bug from symptom to code

1. Say "Eileen, something went wrong" or press the report hotkey right after it happens. That freezes the last ten minutes into `%LOCALAPPDATA%\eileen\incidents\`.
2. `eileen incident show last` summarises the turns, routes, errors, restarts and stalls in the window.
3. `eileen trace <turn_id>` stitches the turn across processes; `timeline.json` opens in Perfetto; every line names its file and line.
4. `docs/CODEMAP.md`'s event map names the producer and handler of the first wrong event; the owning crate's crate doc names its invariants and tests.
5. `eileen replay <incident> --level core` tells a logic bug (it reproduces) from a model, desktop or timing problem (it replays clean).
6. The reproduction becomes a simulator regression, a fuzz regression or a scenario before the fix, and the fix's pull request leads with it as evidence.
