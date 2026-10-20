# Architecture

This is the map of Eileen. Read it before changing code. It describes only what changes rarely; the detailed v0.1 specification, contracts and work packages are in [BUILD.md](BUILD.md), each crate's rules are in its crate doc (the `//!` comment at the top of `src/lib.rs` or `src/main.rs`), and the reasons are in [DECISIONS.md](DECISIONS.md). Last revisited 2026-10-20.

WP0 moves this file to the repository root and adds the check that its code map lists exactly the workspace crates (BUILD.md section 4b). The Python-era design that this file used to hold is in git history at the tag `python-scaffold-final`; decisions D17 to D38 record what replaced it.

## Bird's eye view

Eileen listens whenever the microphone is not muted, works out what the user means, and either acts at once on the PC or starts jobs that run in parallel while the conversation continues. It learns from every request, so the next request of the same kind is faster and eventually needs no model.

- **Input:** audio from the microphone; hotkeys, tray and HUD clicks; typed text; what is on screen (window state, the UI Automation tree, app scripting state); events from connectors; the output of Claude sessions.
- **Output:** speech; the HUD; actions on the PC through tools; messages to Claude sessions running on the owner's own plan.
- **State:** the job registry and event log (`eileen.db`), learned skills, trajectories, the user model and the episodic ledger (`learn.sqlite` and the `eileen-learned` plugin folder), settings with their version history, recordings and incidents. All of it lives on the user's PC.

One core process owns the microphone loop, the event bus, jobs, permissions, learning and a supervisor. Everything that can hang or crash (speech models, UI Automation, app COM, the HUD, every Claude session) runs in a child process the supervisor can kill. The core decides everything in one deterministic state machine, `Core::step`, which the simulator drives in tests.

## Entry points

- `crates/eileen-cli/src/main.rs`: the `eileen` binary. Starts the runtime, the components and the core; also every subcommand.
- `Core::step` in `crates/eileen-core`: every decision starts here. Read its `Input` and `Effect` enums in `eileen-contracts` first.
- Each sidecar's `main.rs`: `eileen-speech`, `eileen-desktop`, `eileen-apps`, `eileen-hud`, and the small `eileen-shim` and `eileen-guard`.
- `crates/eileen-sim`: the fastest way to watch the core run, and to reproduce a bug from a seed or a recording.

## Processes

| Process | Holds | Talks to the core over |
|---|---|---|
| `eileen.exe` | Audio I/O and echo cancellation, `Core::step`, the drivers, store, bus, loopback endpoint, dispatcher, supervisor, learning | n/a |
| `eileen-speech.exe` | VAD, streaming ASR, end of turn, TTS, voice cloning, speaker check, the intent embedding model | binary frames over a named pipe |
| `eileen-desktop.exe` | UI Automation, capture, input, app index and launch, windows, DesktopState, text insertion | MCP over stdio |
| `eileen-apps.exe` | SMTC, Office and Explorer COM, the Resolve bridge, recent work, face and sharpness | MCP over stdio |
| `eileen-hud.exe` | The overlay and the clickable strip | JSON lines over stdio |
| `claude -p` | The brain (warm, plus a hot spare), each worker job, the librarian, the dictation scribe | stream-json over stdio, HTTP hooks to the loopback endpoint |
| `eileen-shim.exe` | One per Claude session: the `eileen` MCP server, forwarding to the dispatcher | named pipe with a per-session token |
| `eileen-guard.exe` | Crash and hang minidumps, the core watchdog; outside the Job Object | local socket |

## Code map

| Crate | Kind | Platform | One line |
|---|---|---|---|
| `eileen-contracts` | lib | neutral | Every type that crosses a process, pipe, file or socket, and the ports |
| `eileen-stdx` | lib | neutral | Clock and ID ports, the seeded RNG, assertions, fault points |
| `eileen-trace` | lib | neutral | Tracing setup, span names and their parent table, redaction, the flight recorder ring |
| `eileen-platform` | lib | neutral | The traits each operating system implements |
| `eileen-supervisor` | lib | neutral, Windows module | Spawning children in kill-on-close Job Objects; process QoS |
| `eileen-core` | lib | neutral | The sans-IO state machine that makes every decision |
| `eileen-record` | lib | neutral | Recordings, the blob store, trimming |
| `eileen-sim` | lib, dev | neutral | Deterministic simulation, invariants, replay of recordings and scenarios |
| `eileen-runtime` | lib | neutral | Drivers that carry out the core's effects: store, bus, loopback endpoint, dispatcher, sidecars, incidents |
| `eileen-mcp` | lib | neutral | rmcp glue, the shim protocol, the core-side tool handlers |
| `eileen-crash` | lib | neutral, Windows module | Panic hook and crash handler client |
| `eileen-cli` | bin | neutral | The `eileen` binary and composition root |
| `eileen-shim` | bin | neutral | Per-session stdio MCP server forwarding to the dispatcher |
| `eileen-guard` | bin | neutral, Windows module | Minidump server and core watchdog |
| `eileen-harness` | lib | neutral | Claude stream-json driver, the brain backend, worktrees; slots for Codex and ACP |
| `eileen-stub-harness` | bin, dev | neutral | A fake `claude -p` that plays recorded scripts with faults |
| `eileen-audio` | lib | neutral | Capture, playback, echo cancellation, mute, barge-in accounting, WAV sources and sinks |
| `eileen-audio-win` | lib | Windows | Audio endpoint policy and MMCSS |
| `eileen-speech` | lib and bin | neutral logic, Windows models | The speech sidecar |
| `eileen-dictation` | lib | neutral | Dictation cleanup rules |
| `eileen-desktop` | lib and bin | neutral logic | The desktop sidecar's tools and safety rules |
| `eileen-desktop-win` | lib | Windows | UI Automation, capture, input, app index, text insertion on Windows |
| `eileen-apps` | lib and bin | neutral logic | The apps sidecar's tools |
| `eileen-apps-win` | lib | Windows | SMTC, COM targets, recent-work sources, the Resolve process gate |
| `eileen-intent` | lib | neutral | The layered intent resolver, the grammar, the skill matcher, introspection |
| `eileen-learn` | lib | neutral | Trajectories, skills, the promotion ladder, replay, the librarian, the user model, the ledger |
| `eileen-config` | lib | neutral | TOML configuration, the settings model, versions, profiles, secrets |
| `eileen-hud` | lib and bin | neutral layout | The HUD sidecar |
| `eileen-shell-win` | lib | Windows | Tray, hotkeys, toasts, the HUD renderer |
| `eileen-e2e` | lib and bin, dev | neutral, Windows fixtures | Real-desktop scenarios, benchmarks, soak, the test fleet |
| `xtask` | bin, tool | neutral | `cargo xtask`: CI, repository checks, the build guard, generators |
| `codemap` (`tools/codemap`) | bin, tool | neutral | Generates `docs/CODEMAP.md` |

### `eileen-contracts`

Every serialized type: IDs, `TraceCtx` and `Envelope`, `Input`, `Effect`, `BusEvent`, DesktopState, `HudState`, tool argument structs and `ToolSpec`, connector manifests, learning types, configuration types, the scenario format, the ports.
**Architecture invariant:** this is the only crate whose types are serialized across a boundary. Other crates convert at their edge, so internal types change freely.
**Architecture invariant:** no IO, no async runtime.
**API boundary:** wire formats, the event log, the skill and recipe format, the config schema, the scenario schema. All are versioned and snapshot-tested.

### `eileen-core`

`Core::step(now, rng, input, effects)`: turns, the addressee gate, intent dispatch, the brain turn, jobs, routing, the permission gate and approvals, announcements, barge-in and what the user heard, the input lock, the journal, the settings flow, plan-usage policy, supervisor policy.
**Architecture invariant:** sans-IO. No clock, no OS randomness, no IO, no threads; every queue bounded; ordered maps wherever order can reach an effect.
**Architecture invariant:** depends only on `eileen-contracts`, `eileen-stdx` and `eileen-trace`. The resolver, dictation cleaner and settings validator arrive through ports.
**Architecture invariant:** an action above tier 1 needs an approval built from `UserInput`. That type cannot be made from screen text, model output, tool results or a learned skill.

### `eileen-runtime`, `eileen-mcp`, `eileen-cli`

The drivers that carry out effects and turn the world into inputs, and the binary that composes them. They hold no decisions.
**Architecture invariant:** only `eileen-mcp` knows rmcp. Every child process starts through `eileen-supervisor`.
**API boundary:** the loopback endpoint (hooks, state, control) and the shim protocol. The bus token reaches status and control only; nothing reachable with it can approve.

### `eileen-harness`

**Architecture invariant:** the only crate that knows vendor CLI flags and protocols. It never reads vendor credential files and never runs a login; workers get a constructed environment and per-session flags only.
**API boundary:** `WorkerDriver` and `BrainBackend`. Claude is built; Codex app-server and ACP are slots behind the same traits.

### Sidecars: `eileen-speech`, `eileen-desktop`, `eileen-apps`, `eileen-hud`

**Architecture invariant:** each may crash or be killed at any time and holds no state that is not rebuilt on start.
**Architecture invariant:** server logic is platform-neutral and tested on Linux; operating-system calls live in the `-win` crates behind `eileen-platform` traits.
**API boundary:** the speech pipe frames, the MCP tool contracts, the HUD lines.

### `eileen-intent`, `eileen-learn`, `eileen-config`

**Architecture invariant:** learned skills and settings can never raise a permission tier or skip a confirmation. A skill replays through the same dispatcher and gate as a worker.
**Architecture invariant:** learned data stays on the PC; "forget" removes the skill, its versions and its evidence.

### `eileen-sim`, `eileen-e2e`, `eileen-stub-harness`

Development only. The simulator owns the invariants; the end-to-end runner owns real-desktop scenarios, benchmarks and the fleet.

## Cross-cutting concerns

**Process isolation and supervision.** Every child runs in a kill-on-close Job Object, so killing the core kills its children. Every cross-process call has a deadline and every sidecar a heartbeat; a hung sidecar is hang-dumped, killed and restarted, and the caller gets a typed error. Sidecars and Claude sessions are long-lived; nothing is spawned per request.

**Events, request IDs and the event log.** A `turn_` ID is minted when a request begins (by the speech process at speech onset, or the core for typed, HUD and connector requests) and travels on every bus event, frame, tool call, span and record. Across MCP it is `_meta.traceparent`; into Claude sessions it is `TRACEPARENT` and `EILEEN_JOB_ID`. One newline-delimited event format serves the flight recorder, the simulator, the end-to-end harness and the learning record.

**Time, randomness and determinism.** The core gets `MonoTime` and a seeded RNG as arguments. IDs come from a port. A recording of port values replays the core exactly; a divergence means Eileen's logic changed.

**Cancellation, interruption and barge-in.** Speech onset while Eileen speaks ducks playback at the first 32 ms window and stops it by the third; synthesis is cancelled; the samples actually played map back to the text the user heard, which is what the brain is told. "Stop" and mute cancel queued input and release the input lock. Workers are interrupted with the CLI's control message and cancelled by killing their Job Object.

**Permissions and untrusted input.** Tiers 0 to 3 (observe, reversible, consequential, blocked). Every tool declares its tier; undeclared means tier 2. Screen text, page text, transcripts of other voices, tool results and model output are data and can never approve, raise a tier or become a skill without the user. Credentials, payments and security settings are never Eileen's to touch.

**Error handling and assertions.** Operating errors are values. Programmer errors are `never!` and `always!` in the core (log, count, dump the flight recorder, keep listening) and plain assertions in sidecars, tests and the simulator. Builders that can fail per item return partial results with diagnostics, so one unreadable window never loses the whole DesktopState.

**Testing.** Unit, property and snapshot tests everywhere; one integration test binary per crate; the core under deterministic simulation with invariants and fixed seeds on pull requests; fuzzing of every untrusted parser; end-to-end runs with recorded or synthesized audio and a stub harness; the owner's benchmarks as timed A/B scenarios; soak tests for the always-on process; and a scenario corpus that grows from real use on many PCs. Slow tests are behind `EILEEN_SLOW_TESTS=1`.

**Observability and debugging.** JSON log lines per process with file and line; an in-memory flight recorder of the last minutes, dumped into an incident on a panic, a crash, a watchdog trip, a spoken "something went wrong", a hotkey or a tray click; minidumps from the guard; `eileen trace`, `eileen incident`, `eileen replay`. User content travels only in `pii.` fields, which each sink handles by policy.

**Configuration.** TOML with comments in `%LOCALAPPDATA%\eileen\config`, generated defaults and schema, every setting reachable by voice and on the HUD, every change versioned and undoable, profiles, and safety bounds no setting can cross.

**Serialization and stability.** A format becomes stable when its first user appears: the event log, the skill format, the scenario format and the config schema carry a version from the first commit, and snapshot tests make every change a reviewed diff.

**Generated files.** `docs/CODEMAP.md`, `docs/SCENARIOS.md`, `docs/reference/*`, the config schema and default file, and the contract schemas are committed, regenerated by `cargo xtask codegen`, and checked fresh in CI. Nobody edits them by hand.

**Performance budgets.** End of speech to first sound about 0.5 s for the fast path and about 2 to 3 s for a brain turn on a quiet machine; barge-in at most 150 ms; idle listening a few percent of one core. BUILD.md section 4.3 has the per-stage table.

**Platform boundaries.** Neutral crates build and test on Linux in CI from the first commit. Windows code lives in `-win` crates and in `cfg(windows)` modules behind `eileen-platform` traits; a Linux port adds `-linux` crates.

**Connectors.** The PC is the first connector (`pc.desktop`, `pc.apps`). The phone, the home hub and other devices are new connectors with the same manifests, tiers, job model and learning loop. Room-sensing connectors are opt-in per device and local only.

**Learning.** Every tool call is recorded at the dispatcher; successful jobs become native skills that Claude workers load; proven skills replay with a check after every step and hand off to a worker on the first failed check; the user can ask what was learned and make Eileen forget it.

## Glossary

- **Turn:** one request from the user, voice or not; its ID is the request ID.
- **Job:** one unit of work run by a worker, a replay or the fast path.
- **Worker:** a Claude session (later Codex, Grok, DeepSeek) running a job; named by route only.
- **Brain:** the warm Claude session that answers and dispatches; it never drives the mouse.
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
6. The reproduction becomes a simulator regression, a fuzz regression or a scenario before the fix.
