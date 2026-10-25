# The Rust core: workspace, quality bar and build discipline

Research pass, written 2026-10-20. Question: what should the Rust workspace that becomes Eileen's always-on core look like, which crate versions does it start from, what does "spotless" mean as concrete configuration, how does it talk to Python and Node adapters, and how do several engineer agents build it at once on the owner's 16 GB PC without freezing it?

This is for contributors who will create the Rust workspace in this repository. It sits next to [2026-10-subscription-harnesses.md](2026-10-subscription-harnesses.md) (harness drivers, the brain), [2026-10-app-hooks.md](2026-10-app-hooks.md) (deep app hooks, the `eileen-apps` process) and [2026-10-brain-latency.md](2026-10-brain-latency.md) (warm `claude -p` turns). It must satisfy [OWNER-DIRECTION.md](../OWNER-DIRECTION.md); where it proposes a change to [ARCHITECTURE.md](../../ARCHITECTURE.md) or [DECISIONS.md](../DECISIONS.md), that change needs a decision record.

## How to read this

Almost everything here was run by two earlier research attempts that were cut off before they wrote a report, one by a usage limit (2026-10-11) and one when the PC ran out of memory and the session ended (2026-10-20). This report is their write-up. Every claim carries a tag:

- **[ran 10-11]**: run on the owner's PC on 2026-10-11 by the first attempt.
- **[ran 10-20]**: run on the owner's PC on 2026-10-20 by the second attempt.
- **[ran 10-20w]**: the single light measurement taken while writing this report (2026-10-20, 15:05 local time).
- **[primary]**: read from a project's own source, registry metadata, docs or release page, on the date given.
- **[inferred]**: reasoning from the above, not measured. Verify before relying on it.

The PC: Windows 11 Home 26200, Intel Core Ultra 7 155H (16 cores, 22 logical processors, no NVIDIA GPU), 16 GB RAM of which the Claude desktop app uses about 4 GB, Rust 1.99.0 stable MSVC (LLVM 23.1.1), VS 2022 C++ tools.

**Every 2026-10-20 timing was taken while other research agents were compiling or running speech models on the same PC** (CPU load 66 percent on average and often 100, free RAM between 0.46 and 2.6 GB). Treat those absolute times as upper bounds. Compare rows taken under the same conditions, not across dates.

## 1. Verdict

1. **One Cargo workspace, several small binaries, one failure domain per risky dependency.** The core process (`eileen.exe`) owns the microphone, mute, the event bus, the job registry, the skill store and the supervisor, and links no UI Automation, no app COM automation, no ONNX runtime and no WebView. UIA, app COM, speech models and the HUD each run in their own supervised child process. The measured reason: one UIA `next_sibling` call blocked for 38.7 minutes on this PC with no window reported hung, far past UIA's own 20 s transaction timeout [ran 10-20]. Only a process boundary plus a kill recovers from that.
2. **Every child starts through one supervisor that puts it in a kill-on-close Windows Job Object.** `process-wrap` 10.0.1 already does this and is what `rmcp`'s child transport uses. Do not use the ACP crate's own `process` helper on Windows; spawn the child yourself and hand ACP the pipes [primary, ws-next written 10-20 but not compiled].
3. **Versions to start from (all checked on crates.io on 2026-10-20):** tokio 1.54.0, serde 1.0.229, serde_json 1.0.154, schemars 1.2.3, thiserror 2.0.21, anyhow 1.0.104, tracing 0.1.44, rusqlite 0.40.2 (bundled), ulid 3.0.0, rmcp 3.6.0 (released that morning), agent-client-protocol 3.4.0, windows 0.62.2, windows-sys 0.61.2, uiautomation 0.25.1, global-hotkey 0.9.0, tray-icon 0.26.1, clap 4.6.7, toml 1.1.8, keyring-core 1.0.1 with windows-native-keyring-store 1.1.0, tauri-winrt-notification 0.8.1. The skeleton passed `clippy -D warnings`, tests, `cargo deny` and `cargo machete` on these versions [ran 10-20].
4. **The skeleton is fast at rest.** The debug build of `eileen run` (tray icon, mute hotkey, bus, SQLite job registry, tokio with two workers) idled at 26.6 MB working set for its process tree, 7.9 MB private, 0.0 percent CPU and 12 threads, with the machine at 100 percent load [ran 10-20w]. It was ready 90 ms after start [ran 10-11]. Release size and release idle were never measured, because the fat-LTO release build was still running when the session died.
5. **The quality bar is written and has run clean**: rustfmt with 2024 style, workspace `[lints]` with clippy `all` denied and `pedantic` and `cargo` at warn (so `-D warnings` makes them errors in CI), `unwrap_used`, `expect_used`, `panic` and friends denied outside tests, `missing_docs`, `unsafe_code` denied workspace-wide and forbidden in safe crates, cargo-deny with permissive licenses only, cargo-nextest, insta snapshots, CI on `windows-latest` with `Swatinem/rust-cache`. Section 6 has the files.
6. **Python adapters must not import the Python MCP SDK at startup.** The same one-tool adapter handshook in 1.7 to 2.9 s as a stdlib-only JSON-RPC shim, against 37 to 69 s with `mcp` 2.3.0 on the loaded machine; importing `mcp.server.mcpserver` alone took 35 to 42 s [ran 10-20]. Resolve itself goes through a Lua bridge (app-hooks report), so Python is a fallback path anyway.
7. **Node ACP adapters work from Rust but are not the v0.1 Claude route.** `claude-agent-acp` 0.89.1 answered `initialize` in 1.5 to 1.8 s [ran 10-11], but it bundles its own 256 MB Claude binary [primary]. v0.1 drives the owner's installed `claude` CLI directly over stream-json, as the harness and brain-latency reports recommend; ACP stays for later routes.
8. **HUD: start with a plain Win32 layered overlay in its own process.** The earlier attempt wrote three HUD prototypes (raw Win32, egui, WebView2 via wry) and never built them, so this pick rests on reasoning, not numbers [inferred]. The lab is ready to measure (section 8).
9. **Parallel agents on this PC: at most two compiling at once, `CARGO_BUILD_JOBS=6` each, one if a release build or a speech model is running; a shared sccache; a target directory per worktree; no debug info for dependencies; no local fat-LTO builds.** One clean dev build under contention drove free RAM to 462 MB, and free RAM reached 0 MB at 13:55 on 10-20 while a fat-LTO release build ran beside other agents, just before the session died [ran 10-20]. Section 9 has the reasoning and a `cargo xtask` guard that enforces it.

## 2. What the earlier attempts built and ran

| Artifact | What it is | State |
|---|---|---|
| `ws` | The skeleton workspace: `eileen-contracts`, `eileen-core` (bus, jobs), `eileen-mcp`, `eileen-harness`, `eileen-shell`, `eileen-cli` (binary `eileen`), with rustfmt, lints, clippy.toml, deny.toml, nextest config, justfile and CI workflow | Built 10-11, bumped to current versions and rebuilt 10-20; fmt, clippy `-D warnings`, 9 tests, deny and machete all pass [ran 10-11, ran 10-20] |
| `ws-next` | `ws` plus `eileen_core::supervisor` (Job Object spawning), the ACP harness rewritten to use it through `ByteStreams`, a disallowed-methods rule for `Command::new`, a corrected lint table, a pre-commit hook and `just setup` | **Written on 10-20, never compiled** |
| `snapshot-2026-10-11` | The 10-11 `Cargo.toml` and `Cargo.lock`, for comparing versions | Reference |
| `compat` | One binary that compile-checks the remaining candidate crates (uiautomation, windows DPAPI, two toast crates, keyring-core, toml, serde-saphyr) and times a few real calls | Built and run three times [ran 10-20] |
| `hud-lab` | Three HUD prototypes: `hud-win32` (layered, topmost, click-through, GDI text), `hud-egui` (eframe 0.37, glow), `hud-wry` (tao 0.37.1 and wry 0.57.0, WebView2) | **Written, never built** |
| `pyadapter` | A Python MCP adapter for Resolve status, once with the `mcp` 2.3.0 SDK and once stdlib-only, in a uv venv | Probed from Rust [ran 10-11, ran 10-20] |
| `acpfetch` | An empty crate used only to download the ACP crate source | Reference |
| `scripts/measure-idle.ps1` | Samples a process tree's working set, private bytes, CPU, threads and handles once a second | Used for [ran 10-20w] |
| `versions/*.json` | crates.io API responses for 66 crates, fetched 10-20 11:35 to 11:41 | Source for section 4 |
| `logs/load.csv` | CPU load and free RAM sampled during the 10-20 builds | Source for load context |

Durable copy (no target directories): `C:\Users\shubh\Projects\eileen-workfiles\2026-10-scratch\rust-arch\`. Originals, with the build outputs and cargo's HTML timing report: `C:\Users\shubh\AppData\Local\Temp\claude\C--Users-shubh-Projects--claude\8b2a7b12-9d74-4cce-a7ac-9d5f3ba7299c\scratchpad\rust-arch\`. The condensed notes of both attempts: `C:\Users\shubh\Projects\eileen-workfiles\predecessor-notes\rust-architecture.md`.

## 3. Workspace layout

### 3.1 What the skeleton proved

The `ws` skeleton is about 1,450 lines of Rust and shows the shape works end to end on this PC [ran 10-11, ran 10-20]:

- `eileen run` starts a tokio runtime with two worker threads, a `tokio::sync::broadcast` event bus, a SQLite job registry, and a separate OS thread that owns a Win32 message pump for the tray icon and the Ctrl+Alt+M mute hotkey. Ready in 90 ms; clean exit on a timer [ran 10-11].
- `eileen mcp-jobs` serves the job registry as an MCP server on stdio with `rmcp`; `eileen mcp-probe` is an rmcp client that spawns any MCP server, lists its tools and calls one. Rust to Rust and Rust to Python both worked [ran 10-11, ran 10-20].
- `eileen acp-probe` spawns an ACP agent, runs `initialize` and hangs up. It worked against `claude-agent-acp` 0.89.1 and `codex-acp` 2.2.2 [ran 10-11].
- `eileen-contracts` holds wire types with serde and schemars and pins their JSON shape with insta snapshots, so a change to a tool's input schema shows up as a reviewed diff.
- All `unsafe` in the workspace is 4 calls in one private module (`eileen-shell/src/ffi.rs`: `GetCurrentThreadId`, `PostThreadMessageW`, `GetMessageW` plus `TranslateMessage` plus `DispatchMessageW`), each block with a `SAFETY:` comment, and clippy enforces it.

### 3.2 Process model

The owner wants the mic loop to keep listening no matter what else breaks. Three things on this PC can hang or crash a process: cross-process COM (UIA and app automation), native inference runtimes (ONNX Runtime, OpenVINO, DirectML), and embedded browsers. None of them can be cancelled from Rust once a call is in flight: a blocked COM call holds its thread, and Rust cannot kill a thread. So each lives in a process the supervisor can kill.

| Process | Contains | What a failure costs | Talks to the core over |
|---|---|---|---|
| `eileen.exe` (core, the root) | Audio capture and playback thread (WASAPI), a ring buffer of recent audio, mute, energy-level barge-in fallback, tray and hotkey thread, event bus, job registry, router, permission tiers, skill store, connector registry, supervisor, harness drivers | Everything; so it links nothing that can hang. Built with `panic = "unwind"` so a panicking task becomes a `JoinError`, not an abort | n/a |
| `eileen-speech.exe` | VAD, turn detection, STT and TTS sessions (sherpa-onnx or ort) | Speech recognition pauses. The core keeps capturing into its ring buffer, the HUD says "speech restarting", and the buffer is replayed after restart | Binary frames over a named pipe (audio in, transcripts and PCM out) [inferred] |
| `eileen-desktop.exe` | UI Automation client, Win32 window state, DesktopState builder; an `rmcp` server | Desktop grounding pauses for a restart; the job falls back to screenshots and vision | MCP over stdio |
| `eileen-apps.exe` | One STA thread per COM target (Word, Excel, Explorer, SMTC) and the Resolve Lua bridge as its child, as the app-hooks report specifies; an `rmcp` server | Deep app hooks pause; UIA and keystrokes still work | MCP over stdio |
| `eileen-hud.exe` | The on-screen overlay | The overlay disappears and restarts; listening and work continue | JSON events over stdio or a named pipe |
| Harness children | `claude -p` stream-json (brain, warm, and workers), later `codex app-server`, `grok agent stdio`, ACP adapters | One job or the brain session; the router restarts it | stream-json, JSON-RPC, ACP |
| Adapter children | Node sidecars (Playwright MCP), Python fallbacks | One app's adapter | MCP over stdio |

Rules:

- **Every child is started by `eileen_core::supervisor`**, which wraps a `tokio::process::Command` in `process-wrap` with `CreationFlags(CREATE_NO_WINDOW)`, `JobObject` and `KillOnDrop`. Killing a `node` or `npx` wrapper then kills what it spawned, and if the core dies Windows closes the job handle and every child dies with it. `clippy.toml` bans `std::process::Command::new` and `tokio::process::Command::new` everywhere except the one `#[expect]`ed function in the supervisor [primary, ws-next].
- **Every sidecar call has a deadline and every sidecar has a heartbeat.** When a UIA or COM call passes its deadline (for example 5 s), the supervisor kills the sidecar's job, restarts it, and returns a typed error to the job, which falls back to the next rung of the action ladder [inferred]. UIA's own `TransactionTimeout` (default 20 s per Microsoft) did not bound the 38.7 minute hang [ran 10-20, primary].
- **Stdout carries the protocol; stderr is for logs.** The supervisor forwards a child's stderr into `tracing` line by line [primary, ws-next].
- **Sidecars are long-lived.** Spawning per utterance costs seconds (sections 5 and 7).
- **Separate binaries, not one binary with subcommands**, so a HUD change relinks only `eileen-hud` and an agent working on UIA never links the speech stack [inferred]. The `eileen` binary keeps `mcp-probe`, `acp-probe` and `bench` as developer subcommands.

### 3.3 Crates and binaries for v0.1

| Crate | Kind | Unsafe | Purpose |
|---|---|---|---|
| `eileen-contracts` | lib | forbidden | Wire types for jobs, events, tool inputs, skill records and connector capabilities; serde and schemars; insta snapshots. No I/O |
| `eileen-core` | lib | forbidden | Bus, job registry (rusqlite), supervisor, permission tiers, router |
| `eileen-skills` | lib | forbidden | The learning loop: job recording, skill extraction, matching, replay with step checks, repair history, "what have you learned" and "forget" (section 3.4) |
| `eileen-connectors` | lib | forbidden | The `Connector` trait and registry; PC connectors are the first implementations |
| `eileen-mcp` | lib | forbidden | rmcp servers (jobs, approvals, skills) and client helpers |
| `eileen-harness` | lib | forbidden | Claude driver (`claude -p` stream-json), ACP driver, slots for Codex, Grok and DeepSeek |
| `eileen-config` | lib | in `ffi` only | TOML config, DPAPI-protected secrets, Credential Manager through keyring-core |
| `eileen-audio` | lib | in `ffi` only | WASAPI capture and render thread, ring buffer, mute, echo-cancellation hook |
| `eileen-shell` | lib | in `ffi` only | Tray icon, global hotkey, message pump |
| `eileen-cli` | bin `eileen` | forbidden | The core process |
| `eileen-speech` | bin | in `ffi` only if needed | Speech models |
| `eileen-desktop` | bin | in `ffi` only | UIA and window state |
| `eileen-apps` | bin | in `ffi` only | App COM targets, Resolve bridge |
| `eileen-hud` | bin | in `ffi` only | Overlay |
| `xtask` | bin, not shipped | forbidden | `cargo xtask ci` and the build guard (section 6.9) |

Library crates that only describe data or logic stay free of Win32 types, so they compile and test on any OS and in parallel with the FFI crates [inferred].

### 3.4 Connectors, the learning loop and the job model

The owner's 2026-10-20 direction asks for two things the layout has to make cheap later: reaching beyond the PC through pluggable connectors, and learning from every request.

- **A connector is a capability provider with tiers.** Each connector declares its tools (name, JSON Schema from schemars, permission tier per tool), its event streams, and where it runs: in the core as async tasks when it is pure network code in safe Rust (a Home Assistant WebSocket client, a phone bridge), or as a supervised sidecar when it brings FFI, COM or drivers (desktop, apps, speech) [inferred]. The router, the permission gate, the job registry and the skill recorder only see the connector interface, so the phone, the home hub (Home Assistant, Matter through it), other devices on the user's network and earbuds as microphones are new `Connector` implementations, not a redesign. Room-sensing connectors carry an "opt-in per device, local processing only" flag that the registry enforces.
- **The learning loop lives in `eileen-skills`, inside the core.** Every tool call already crosses the core (MCP results, ACP updates, stream-json events), so the core records each job as rows: request, DesktopState snapshot reference, each tool call and result, and the outcome (accepted, corrected, undone). A successful job becomes a named, versioned skill; replay runs the same connector tools with a check per step and hands the job to a worker the moment a check fails. Skills inherit the permission tier of every tool they call, so a learned skill can never skip a confirmation [inferred]. The schema belongs in `eileen-contracts` with snapshot tests from day one.

## 4. Crate versions, verified 2026-10-20

From the crates.io API, fetched 2026-10-20 11:35 to 11:41, cross-checked against the lockfile of the skeleton that compiled [primary, ran 10-20]. "Tested" means the skeleton or `compat` compiled and ran with that exact version.

| Area | Crate | Version (date) | License | MSRV | Tested | Notes |
|---|---|---|---|---|---|---|
| Async | tokio | 1.54.0 (2026-10-19) | MIT | 1.85 | yes | `default-features = false`, features per crate; core uses `new_multi_thread().worker_threads(2)` |
| Async | tokio-util | 0.7.20 (2026-10-09) | MIT | 1.85 | written | `compat` feature bridges tokio pipes to ACP's futures-io traits |
| Data | serde | 1.0.229 (2026-07-18) | MIT OR Apache-2.0 | 1.56 | yes | |
| Data | serde_json | 1.0.154 (2026-10-11) | MIT OR Apache-2.0 | 1.71 | yes | ACP turns on `preserve_order` for the whole build (pitfall 7) |
| Data | schemars | 1.2.3 (2026-10-12) | MIT | 1.74 | yes | Same major as rmcp's |
| Data | ulid | 3.0.0 (2026-07-16) | MIT | none | yes | Features `std` and `serde`; constructor is `Ulid::generate()` |
| Errors | thiserror | 2.0.21 (2026-09-23) | MIT OR Apache-2.0 | 1.77 | yes | Libraries |
| Errors | anyhow | 1.0.104 (2026-07-18) | MIT OR Apache-2.0 | 1.68 | yes | Binaries only |
| Logs | tracing, tracing-subscriber, tracing-appender | 0.1.44 (2025-12-18), 0.3.23 (2026-03-13), 0.2.5 (2026-04-17) | MIT | 1.65 | yes | Non-blocking daily file appender in the core |
| Storage | rusqlite | 0.40.2 (2026-08-08) | MIT | none | yes | `bundled` (libsqlite3-sys 0.38.2). Its C build was the slowest unit of a clean build (section 5). sqlx 0.9.0 (MSRV 1.94) not chosen: async and macro-heavy for one local file [inferred] |
| MCP | rmcp | 3.6.0 (2026-10-20) | Apache-2.0 | 1.88 | clippy and tests | Features `server`, `client`, `macros`, `transport-io`, `transport-child-process`. 3.6.0 moved client-only and server-only helpers behind their features and rejects JSON-RPC messages that carry both `result` and `error` [primary, release notes] |
| ACP | agent-client-protocol | 3.4.0 (2026-10-16) | Apache-2.0 | 1.88 | clippy and tests | Schema crate 1.12.0. Use without the `process` feature (section 7) |
| Processes | process-wrap | 10.0.1 | permissive (passed the cargo-deny license allowlist) | | via rmcp | Features `tokio1`, `job-object`, `kill-on-drop`, `creation-flags` |
| Win32 | windows | 0.62.2 (2025-10-06) | MIT OR Apache-2.0 | 1.82 | yes | Still the newest umbrella crate. The focused crates (`windows-core`, `-result`, `-strings`, `-registry`, `-future`, `-collections`, `-numerics`, `-bindgen`) jumped to 0.100.0 on 2026-09-03 with MSRV 1.95 and do not mix with 0.62.2: take core types from `windows::core` |
| Win32 | windows-sys | 0.61.2 (2025-10-06) | MIT OR Apache-2.0 | 1.71 | yes | For plain function calls (message pump) |
| UIA | uiautomation | 0.25.1 (2026-09-04) | Apache-2.0 | none | yes | Depends on `windows ^0.62.2`, so it shares our copy |
| Shell | global-hotkey | 0.9.0 (2026-10-16) | Apache-2.0 OR MIT | 1.77 | yes | keyboard-types 0.8; the Super modifier is now `META` [primary, changelog] |
| Shell | tray-icon | 0.26.1 (2026-10-07) | MIT OR Apache-2.0 | 1.90 | yes | `default-features = false` drops the Linux `libappindicator` default; menus via muda 0.21.2 |
| Notifications | tauri-winrt-notification | 0.8.1 (2026-07-17) | MIT OR Apache-2.0 | 1.82 | built, not shown | v0.1 choice: thin and Windows-only |
| Notifications | notify-rust | 4.18.2 (2026-10-07) | MIT OR Apache-2.0 | 1.89 | built, not shown | Cross-platform alternative |
| Secrets | keyring-core + windows-native-keyring-store | 1.0.1 (2026-10-13), 1.1.0 (2026-05-24) | MIT OR Apache-2.0 | | yes | keyring 4.2.0 is now glue for its CLI; applications link keyring-core plus a store [primary, docs.rs] |
| Secrets | DPAPI through `windows` | 0.62.2 | | | yes | `CryptProtectData` and `CryptUnprotectData` (feature `Win32_Security_Cryptography`); windows-dpapi 0.2.0 exists but adds little |
| Config | toml | 1.1.8 (2026-10-09) | MIT OR Apache-2.0 | 1.85 | yes | |
| Config | serde-saphyr | 2.0.0 | | | yes | Only if YAML must stay. serde_yml 0.0.13 is unsound and unmaintained (RUSTSEC-2025-0068); serde_yaml is published as `0.9.34+deprecated` [primary, RustSec db] |
| CLI | clap | 4.6.7 (2026-09-14) | MIT OR Apache-2.0 | 1.85 | yes | derive |
| HUD | eframe, egui | 0.37.0 (2026-10-15) | MIT OR Apache-2.0 | 1.95 | no | Candidate |
| HUD | wry, tao | 0.57.0 (2026-09-08), 0.37.1 (2026-09-26) | Apache-2.0 OR MIT | 1.85 | no | Candidate (WebView2) |
| HUD | tauri | 2.12.4 (2026-10-14) | Apache-2.0 OR MIT | 1.90 | no | Not proposed for the HUD |
| Tests | insta, proptest | 1.49.0 (2026-10-03), 1.12.0 (2026-10-10) | Apache-2.0; MIT OR Apache-2.0 | 1.66, 1.88 | insta yes | |
| Speech (owned by the voice research) | ort, sherpa-onnx, cpal, wasapi | 2.0.0-rc.15 (2026-10-19), 1.13.8 (2026-09-11), 0.18.2, 0.26.0 | | | no | ort is still a release candidate |
| Capture | windows-capture, xcap | 2.0.1, 0.9.8 | MIT; Apache-2.0 | | no | |

Tools [primary, crates.io and GitHub, 10-20]: cargo-nextest 0.9.148, cargo-deny 0.20.2, cargo-machete 0.9.2, cargo-insta 1.49.0, just 1.59.0 (1.58.0 is installed), sccache 0.18.0, cargo-binstall 1.26.0 (GPL-3.0-only; a tool we run, never link), typos 1.51.1, cargo-udeps 0.1.61 (needs nightly to run, so not used).

GitHub Actions, latest releases on 10-20 [primary]: `actions/checkout` v7.0.1, `Swatinem/rust-cache` v2.9.3 (released 10-20), `taiki-e/install-action` v2.87.29, `EmbarkStudios/cargo-deny-action` v2.1.1, `crate-ci/typos` v1.51.1, `actions/upload-artifact` v7.0.2, `mozilla-actions/sccache-action` v0.0.11, `dtolnay/rust-toolchain` (tag per toolchain). `windows-latest` is Windows Server 2025, image 20261013, with Rust 1.99.0, Clippy 0.1.99 and rustfmt 1.10.0 preinstalled, VS 2022 17.14 and Windows SDK 10.0.26100 [primary, runner-images readme].

Adapter-side packages [primary, 10-20]: PyPI `mcp` 2.3.0 (FastMCP is renamed `MCPServer` in 2.x), `fastmcp` 4.1.2; npm `@agentclientprotocol/claude-agent-acp` 0.94.0 (0.89.1 was tested), `@agentclientprotocol/codex-acp` 2.2.2, `@agentclientprotocol/sdk` 2.0.0, `@modelcontextprotocol/sdk` 1.32.1.

Version moves between the two attempts (2026-10-11 to 2026-10-20): tokio 1.53.2 to 1.54.0, serde_json 1.0.152 to 1.0.154, schemars 1.2.2 to 1.2.3, rmcp 3.5.1 to 3.6.0, agent-client-protocol 3.3.0 to 3.4.0, global-hotkey 0.8.0 to 0.9.0. `cargo update` changed 22 lockfile entries and clippy with `-D warnings` still passed, so none of these broke the skeleton's code [ran 10-20]. Expect this churn rate in the protocol crates: rmcp shipped six releases from 10 September (3.3.0) to 20 October (3.6.0).

## 5. Measurements

### Builds

| What | Result | Conditions |
|---|---|---|
| Clean dev build of the skeleton (`cargo build -p eileen-cli`, own target dir) | **241.9 s** | [ran 10-11]; other agents active (load 91 percent, 2.6 GB free just before) |
| Clean dev build of the bumped skeleton (`cargo build --locked --timings`) | **1,278 s (19 min 41 s)** wall; 215 units; cargo's sum of unit times 11,161 s, so about 9.4 units ran at once on average | [ran 10-20]; load average 66 percent (2 to 100), free RAM average 1,601 MB, **minimum 462 MB** |
| Slowest units in that build | libsqlite3-sys build script 666 s (on the critical path); then proc-macro crates: derive_more-impl 325 s, schemars_derive 299 s, serde_derive 293 s, clap_derive 274 s, strum_macros 257 s, pin-project-internal 234 s, serde_with_macros 207 s, tracing-attributes 199 s, rmcp-macros 194 s, thiserror-impl 186 s; also tokio 237 s, rmcp 227 s, syn 2 and syn 3 about 172 s each | [ran 10-20], same build |
| Incremental: touch `eileen-core`, rebuild | 736 s | [ran 10-20], free RAM down to 483 MB during the window; almost certainly paging, re-measure on a quiet PC |
| Incremental: touch `eileen-cli`, rebuild | 87 s | [ran 10-20] |
| Incremental clippy after a small edit | 6.3 s; 28.6 s after a lint-table change | [ran 10-11] |
| Clippy `-D warnings` after the version bump (old target dir) | 824 s, exit 0 | [ran 10-20] |
| Test build plus 9 tests under nextest | 12 min 16 s to build; tests 0.3 to 0.4 s on 10-11, 2.7 to 7.7 s each on 10-20 | [ran 10-11, ran 10-20] |
| Release build, `lto = "fat"`, `codegen-units = 1` | **Never finished.** Started 13:17:01, still running at 13:29; free RAM hit 0 MB at 13:55 and the session ended | [ran 10-20] |
| `compat` release build (thin LTO, 249 packages) | 449 s cold, 138 s after an edit; `compat.exe` 3.27 MB | [ran 10-20] |
| `cargo install --locked` of cargo-deny, cargo-machete, cargo-nextest and just from source | 3,565 s (59 min) | [ran 10-11] |
| `cargo machete`, `cargo deny check` | 2.1 s; deny passes advisories, bans, licenses and sources; duplicate warnings for `syn` (2 and 3) and `windows-sys` (0.59 and 0.61) | [ran 10-20] |
| Lockfile size | 255 packages | [ran 10-20] |

### Size and resources

| What | Result | Conditions |
|---|---|---|
| Debug `eileen.exe` | 14.0 MB (10-11), 14.4 MB (10-20) | `debug = "line-tables-only"`, dependencies at `opt-level = 2` |
| Release `eileen.exe` | **Not measured** | Release build never finished. Proxies: `compat.exe` 3.27 MB with thin LTO; the app-hooks report's `hookprobe` 313 to 449 KB |
| Idle `eileen run` (debug) | **26.6 MB working set** (process tree: eileen plus its console host), **7.9 MB private**, **0.0 percent CPU** of one core over 15 one-second samples, 12 threads, 279 handles | [ran 10-20w], machine at 100 percent load and 935 MB free |
| Time to ready (tray up, hotkey registered) | 90 ms | [ran 10-11] |
| One `rustc.exe` during clippy | 417 MB working set | [ran 10-20] |
| Target directories | 931 MB after one clean dev build; 3.3 GB after a week of builds and tests in one dir | [ran 10-20]; 19.4 GB free on C: that day |

### Interop

| What | Handshake | Tool call | Conditions |
|---|---|---|---|
| Rust client to Rust server (`eileen mcp-jobs`, debug) | 5,110 ms, 3,286 ms, 633 ms | 4.6 to 21 ms | [ran 10-20], loaded |
| Rust client to Python adapter, `mcp` 2.3.0 SDK | 2,442 ms, 13,216 ms, 14,168 ms | 7.7 to 25 ms | [ran 10-11] |
| Same | 68,585 ms, 50,795 ms, 36,800 ms | 10 to 26 ms | [ran 10-20], loaded |
| Rust client to Python adapter, stdlib only | **1.7 to 2.9 s** (first 1,820 ms) | 2.5 to 9.7 ms | [ran 10-20], loaded; negotiated MCP protocol version `2026-07-28` |
| Bare `python -I -c pass` from the venv | 4.5 s, 6.9 s | | [ran 10-20], loaded |
| `python -I -c "import mcp.server.mcpserver"` | 34.9 s, 41.7 s | | [ran 10-20]; `-X importtime` shows the server import pulling `mcp.client`, then the streamable-HTTP stack (`sse_starlette` and its web framework) even for a stdio server |
| ACP `initialize`, `claude-agent-acp` 0.89.1 under Node 24 | 1,757 ms, 1,484 ms | | [ran 10-11]; protocol version 1, `load_session` true, 0 auth methods |
| ACP `initialize`, `codex-acp` 2.2.2 | 6,559 ms | | [ran 10-11]; 2 auth methods advertised (Codex not signed in) |

### Win32 calls from Rust (`compat`, release, three runs)

| Call | Result [ran 10-20] |
|---|---|
| Create the UIA client | 115 ms, 349 ms, 1,045 ms |
| Walk the 11 top-level UIA elements (name, class, then `next_sibling`) | 16.7 s, 26.1 s, and once **2,326 s**: `next_sibling` after the desktop window (`Progman`) blocked for 38.7 minutes. A separate probe with `IsHungAppWindow` and `SendMessageTimeout` found no hung top-level window at the time |
| Single `next_sibling` hops on a normal run | 12 to 826 ms |
| Foreground window title through Win32 | 0.04 to 0.1 ms |
| DPAPI round trip (28 bytes to 246 and back) | 8.2 to 16.7 ms |
| keyring-core lookup of an absent entry | 2.6 to 13.5 ms, `NoEntry` as expected |
| Parse a small config: toml, serde-saphyr YAML | 0.4 to 5.1 ms, 1.1 to 12.2 ms |

## 6. The quality bar as configuration

These are the files from `ws-next`. Everything except the `tokio::process` disallowed-method line, the corrected lint names and the supervisor exception was run clean in `ws` on 10-20 (fmt, clippy `-D warnings`, nextest, doc tests, `cargo doc` with `-D warnings`, deny, machete). `just ci` ran the same gate on 10-11.

### 6.1 Toolchain and formatting

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.99.0"
components = ["rustfmt", "clippy"]
targets = ["x86_64-pc-windows-msvc"]
profile = "minimal"
```

`rustfmt.toml` (stable options only, so `cargo fmt --check` behaves the same on every toolchain):

```toml
edition = "2024"
style_edition = "2024"
max_width = 100
use_small_heuristics = "Max"
newline_style = "Unix"
use_field_init_shorthand = true
use_try_shorthand = true
reorder_imports = true
reorder_modules = true
```

The workspace uses `resolver = "3"` and `rust-version = "1.99"`. With resolver 3, Cargo prefers dependency versions whose MSRV fits ours (`incompatible-rust-versions = "fallback"`) [primary, Cargo resolver docs].

### 6.2 Workspace lints

In the root `Cargo.toml`; every member crate has `[lints] workspace = true`. Groups are set at priority -1 so single lints can override them. Local builds show pedantic findings as warnings; CI runs `cargo clippy -- -D warnings`, which turns every warning, including `missing_docs`, into an error.

```toml
[workspace.lints.rust]
unsafe_code = "deny"            # safe crates add #![forbid(unsafe_code)]; FFI crates scope an #[expect] to one ffi module
unsafe_op_in_unsafe_fn = "deny"
missing_unsafe_on_extern = "deny"
non_ascii_idents = "forbid"
missing_docs = "warn"
missing_debug_implementations = "warn"
unreachable_pub = "warn"
unused_qualifications = "warn"
unused_lifetimes = "warn"
redundant_lifetimes = "warn"
single_use_lifetimes = "warn"
trivial_casts = "warn"
trivial_numeric_casts = "warn"
let_underscore_drop = "warn"
elided_lifetimes_in_paths = "warn"
explicit_outlives_requirements = "warn"
meta_variable_misuse = "warn"
unit_bindings = "warn"
unused_macro_rules = "warn"
unexpected_cfgs = { level = "warn", check-cfg = [] }

[workspace.lints.rustdoc]
broken_intra_doc_links = "deny"
private_intra_doc_links = "deny"
bare_urls = "warn"
unescaped_backticks = "warn"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
cargo = { level = "warn", priority = -1 }

# Pedantic and cargo lints turned off, each for a stated reason.
missing_errors_doc = "allow"        # every fallible fn returns a typed error enum that documents itself
missing_panics_doc = "allow"        # panics are denied below, so the section would always be empty
multiple_crate_versions = "allow"   # windows-sys has two majors in the tree; cargo-deny reports duplicates instead
cargo_common_metadata = "allow"     # crates are publish = false

# Nursery lints stable enough to enforce.
use_self = "warn"
or_fun_call = "warn"
redundant_clone = "warn"
needless_collect = "warn"
significant_drop_tightening = "warn"
significant_drop_in_scrutinee = "warn"
option_if_let_else = "allow"        # rewrites readable matches into map_or_else chains
missing_const_for_fn = "warn"
equatable_if_let = "warn"
debug_assert_with_mut_call = "warn"
useless_let_if_seq = "warn"

# Restriction lints: the spotless bar.
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "deny"
unimplemented = "deny"
unreachable = "warn"
dbg_macro = "deny"
print_stdout = "deny"
print_stderr = "deny"
exit = "deny"
mem_forget = "deny"
undocumented_unsafe_blocks = "deny"
multiple_unsafe_ops_per_block = "deny"
unnecessary_safety_comment = "deny"
unnecessary_safety_doc = "deny"
allow_attributes = "deny"           # use #[expect(...)] so a stale suppression fails the build
allow_attributes_without_reason = "deny"
get_unwrap = "deny"
indexing_slicing = "warn"
string_slice = "deny"
clone_on_ref_ptr = "warn"
rc_buffer = "warn"
rc_mutex = "deny"
str_to_string = "warn"
lossy_float_literal = "deny"
float_cmp_const = "deny"
empty_structs_with_brackets = "warn"
rest_pat_in_fully_bound_structs = "warn"
tests_outside_test_module = "warn"
unused_result_ok = "warn"
let_underscore_must_use = "deny"
let_underscore_untyped = "warn"
verbose_file_reads = "warn"
mod_module_files = "deny"           # one module style: foo.rs plus foo/, never foo/mod.rs
infinite_loop = "warn"
large_include_file = "deny"
fn_to_numeric_cast_any = "deny"
if_then_some_else_none = "warn"
renamed_function_params = "warn"
semicolon_outside_block = "warn"
unseparated_literal_suffix = "warn"
wildcard_enum_match_arm = "warn"
missing_assert_message = "warn"
create_dir = "warn"
filetype_is_file = "warn"
cfg_not_test = "deny"
```

Every lint name above was checked against the stable Clippy lint index as published on 10-20; the check found and fixed three errors in the 10-11 table (pitfall 9) [ran 10-20].

### 6.3 `clippy.toml`

```toml
# Tests may unwrap, expect, panic and index: a failing test should fail loudly.
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-panic-in-tests = true
allow-indexing-slicing-in-tests = true
allow-dbg-in-tests = true
allow-print-in-tests = true

avoid-breaking-exported-api = false   # an application, not a library
too-many-lines-threshold = 80
too-many-arguments-threshold = 6
cognitive-complexity-threshold = 20

disallowed-methods = [
  { path = "std::thread::sleep", reason = "blocks a runtime worker; use tokio::time::sleep, or a dedicated thread with a reason" },
  { path = "std::env::set_var", reason = "process-global and unsound with threads; pass config explicitly" },
  { path = "std::env::remove_var", reason = "process-global and unsound with threads; pass config explicitly" },
  { path = "std::process::Command::new", reason = "start children through eileen_core::supervisor (Job Object, kill on close)" },
  { path = "tokio::process::Command::new", reason = "start children through eileen_core::supervisor (Job Object, kill on close)", allow-invalid = true },
  { path = "std::process::exit", reason = "skips destructors and log flushing; return from main" },
]
disallowed-types = [
  { path = "std::sync::mpsc::Sender", reason = "use tokio::sync or crossbeam channels" },
]
disallowed-names = ["foo", "bar", "baz", "tmp", "data2"]
```

So `unwrap_used` and `expect_used` are errors everywhere except `#[cfg(test)]` code. `allow-invalid = true` keeps the tokio entry from erroring in crates that do not depend on tokio's `process` feature [primary, Clippy docs].

### 6.4 Unsafe code

- Workspace: `unsafe_code = "deny"`. Every crate that needs no FFI adds `#![forbid(unsafe_code)]` at its root, which nothing below can override.
- An FFI crate (`eileen-shell`, `eileen-audio`, `eileen-config`, `eileen-desktop`, `eileen-apps`, `eileen-hud`, maybe `eileen-speech`) keeps every unsafe call in one private module, declared as:

  ```rust
  #[expect(unsafe_code, reason = "Win32 message pump; every block carries a SAFETY comment")]
  mod ffi;
  ```

  `forbid` would not allow this, which is why the workspace level is `deny`. Inside, `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` make every block one call with a `// SAFETY:` comment, and the module exposes only safe functions.
- Prefer crates that own their unsafe code (`process-wrap` for Job Objects, `uiautomation`, `tray-icon`) over hand-written bindings. The supervisor has no unsafe code at all.

### 6.5 cargo-deny

```toml
[graph]
targets = ["x86_64-pc-windows-msvc"]
all-features = false

[advisories]
unmaintained = "workspace"
yanked = "deny"
ignore = []

[licenses]
# Permissive only; everything linked must be compatible with shipping one MIT binary.
allow = ["MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause",
         "BSD-3-Clause", "ISC", "Zlib", "0BSD", "Unicode-3.0", "Unlicense", "CC0-1.0", "BSL-1.0"]
confidence-threshold = 0.9

[bans]
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = true
highlight = "all"
deny = [
  { crate = "openssl-sys", reason = "use rustls or SChannel" },
  { crate = "native-tls", reason = "use rustls" },
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []
```

Passed on the full skeleton tree on 10-11 and 10-20 [ran 10-11, ran 10-20]. When the speech crates land, check `ort` and `sherpa-onnx` and the native libraries they download: cargo-deny only sees Rust crates, so the license of a downloaded ONNX Runtime or OpenVINO DLL needs a separate check in the release job [inferred].

### 6.6 Tests

- cargo-nextest 0.9.148 runs the unit and integration tests; `cargo test --doc` runs the doc tests (nextest does not). `.config/nextest.toml`:

  ```toml
  [profile.default]
  slow-timeout = { period = "10s", terminate-after = 3 }   # a test over 30 s is a bug

  [profile.ci]
  fail-fast = false
  retries = 0
  failure-output = "immediate-final"
  slow-timeout = { period = "15s", terminate-after = 4 }

  [profile.ci.junit]
  path = "junit.xml"
  ```

- insta 1.49.0 snapshots pin every wire format and every tool input schema in `eileen-contracts` (`cargo insta review` after an intentional change).
- proptest 1.12.0 for parsers and the skill matcher.
- `eileen-mcp` gets an in-process round trip test: the real `jobs_list` tool served over a `tokio::io::duplex` pipe, so MCP is tested without spawning (written in ws-next, not yet run).

### 6.7 Profiles

As run on 10-20:

```toml
[profile.dev]
debug = "line-tables-only"        # useful backtraces at a fraction of full debuginfo's link time

[profile.dev.package."*"]
opt-level = 2                     # audio, ONNX and SQLite are unusable at opt-level 0

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "unwind"                  # the core must survive a panicking task
debug = "line-tables-only"
strip = "none"                    # MSVC keeps symbols in the .pdb; ship the .pdb to a symbol store
incremental = false

[profile.release-abort]           # for helper processes, measured separately
inherits = "release"
panic = "abort"
```

Proposed changes, not yet measured [inferred]:

1. **Split release in two.** `release` with `lto = "thin"` and the default codegen units for CI smoke tests and local checks; a `dist` profile (`inherits = "release"`, `lto = "fat"`, `codegen-units = 1`) built only for tagged releases. Fat LTO with one codegen unit is the slowest and most memory-hungry thing in the build, and it is the build that was running when the PC ran out of memory.
2. **No debug info for dependencies in dev**: add `debug = false` under `[profile.dev.package."*"]`. Smaller PDBs and less link memory; backtraces still resolve our own frames.
3. **Narrow `opt-level = 2` to the crates that need it** once the speech crates exist. Per the Cargo reference, a `package."*"` override takes precedence over `build-override`, so the current setting also builds every proc macro and `syn` at opt-level 2, and the `cc` crate builds SQLite's C source optimised. Both were among the slowest units above. Check the effect with `cargo build --timings` before and after.

### 6.8 CI

`.github/workflows/ci.yml` from ws-next has two jobs:

- `hygiene` on `ubuntu-latest` (10 min limit): `cargo fmt --check`, `cargo-deny-action`, `cargo machete` (installed by `taiki-e/install-action`, prebuilt), `crate-ci/typos`, and a grep firewall that fails the build if any crate source names a vendor credential path (`.credentials.json`, `.codex/auth.json`) or a user-facing string starts with "Claude Code ". Extend that grep to "Claude Agent", since workers are named by route only.
- `windows` on `windows-latest` (30 min limit): `dtolnay/rust-toolchain@1.99.0` with clippy, `Swatinem/rust-cache@v2` with a shared key, `taiki-e/install-action` for nextest, then clippy with `-D warnings`, `cargo nextest run --profile ci`, doc tests, `cargo doc` with `RUSTDOCFLAGS=-D warnings`, a release build of `eileen-cli`, a smoke test (`eileen --version` and an `mcp-probe` round trip against `eileen mcp-jobs`), and an upload of `eileen.exe` with its `.pdb`.

Global settings: `CARGO_INCREMENTAL=0`, `concurrency` cancels superseded runs, `permissions: contents: read`. The workflow has not run on GitHub yet; the repository's current CI is the Python scaffold's.

### 6.9 One command: `cargo xtask ci`

The earlier attempts used `just ci`, which worked, but `just` is one more binary to install and its Windows shell had to be changed from `pwsh` (not installed here) to `powershell.exe`. Recommend an `xtask` crate instead, with `.cargo/config.toml`:

```toml
[alias]
xtask = "run --package xtask --"
```

and subcommands [inferred, not built]:

- `cargo xtask ci`: fmt check, clippy `-D warnings`, nextest, doc tests, docs with `-D warnings`, `cargo deny check`, `cargo machete`; the same list CI runs, so "green locally" means green in CI.
- `cargo xtask pre-commit`: fmt check, clippy, nextest. The `.githooks/pre-commit` hook calls it.
- `cargo xtask setup`: install the tools prebuilt (cargo-binstall or release archives, seconds instead of 59 minutes) and run `git config core.hooksPath .githooks`.
- `cargo xtask build|check|test|clippy ...`: the same as plain cargo, but through the build guard of section 9.

`xtask` uses only std (std's `File::try_lock` is stable since Rust 1.89, enough for the guard), so it compiles in seconds.

## 7. Interop with Python and Node adapters

**The boundary is MCP over stdio, spawned by the supervisor.** The rmcp client side (`TokioChildProcess` over `process-wrap`) handled both Rust and Python servers; handshakes and tool calls worked on both dates. Tool calls cost 2.5 to 26 ms; the handshake is dominated by the child's own startup [ran 10-11, ran 10-20].

**Python.** The import chain of the official SDK is what costs: `mcp.server.mcpserver` imports the client package and the whole streamable-HTTP stack before a stdio server can answer, 35 to 42 s on the loaded PC and 2.4 to 14 s on 10-11. A 100-line stdlib-only adapter (JSON-RPC over stdin and stdout, `initialize`, `tools/list`, `tools/call`) answered the same Rust client in 1.7 to 2.9 s [ran 10-20]. Rules for Python adapters:

- Stdlib-only JSON-RPC shim, a small shared module copied into each adapter; no MCP SDK import at startup.
- Run with `python -I` from a uv venv pinned by the adapter's own lockfile; never a global install.
- Long-lived: started once by the supervisor, kept warm, restarted on failure. Bare interpreter startup alone took 4.5 to 6.9 s under load [ran 10-20].
- Version 2.x of the Python SDK renamed `FastMCP` to `MCPServer`; code written for 1.x fails at import [ran 10-11].
- For Resolve, the app-hooks report replaces Python with a Lua bridge under Resolve's own `fuscript.exe`; Python stays as the fallback.

**Node ACP adapters.** `claude-agent-acp` 0.89.1 initialised in 1.5 to 1.8 s and `codex-acp` 2.2.2 in 6.6 s [ran 10-11]. Two reasons not to use the Claude adapter in v0.1: it depends on `@anthropic-ai/claude-agent-sdk`, whose Windows native package is a 256 MB second copy of the Claude binary [primary, npm], and the harness report's native stream-json driver avoids the Node hop (about 99 MB idle and about 1 s per session). ACP stays the generic driver for routes that speak it natively (for example `grok agent stdio`) or through a vendor adapter.

**How to drive ACP from Rust on Windows.** The ACP crate's `process` feature brings `async-process`, `async-io` (a second reactor beside tokio), `rustix` and `windows-sys`, and its Windows path neither groups nor job-wraps the child, so killing a `node` wrapper can orphan the real agent [primary, source of 3.4.0]. Instead: build the command with `supervisor::rpc_command`, spawn it through `supervisor::supervised`, convert the child's stdin and stdout with `tokio_util::compat`, and pass them to `agent_client_protocol::ByteStreams::new(outgoing, incoming)`. Only `AcpAgent` and `AcpAgentConfig` sit behind the `process` feature, so nothing else is lost [primary]. This is written in ws-next and not yet compiled.

**Names.** Package names such as `claude-agent-acp` stay internal. The user sees route names only: Claude, Codex, Grok, DeepSeek.

## 8. HUD, notifications, secrets and config

**HUD.** The three prototypes in `hud-lab` were never built, so there is no measured comparison. Reasoning [inferred]:

- A raw Win32 overlay (`WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, drawn with GDI first and Direct2D and DirectWrite later, through the `windows` crate we already link) adds no dependencies and should cost a few MB. It never takes focus and lets clicks through, which a HUD over the user's work needs.
- egui 0.37 needs a GPU context and winit; fine for a richer panel, MSRV 1.95.
- WebView2 through wry is the most flexible for layout but runs separate `msedgewebview2.exe` processes, which is why `measure-idle.ps1` sums the whole process tree. Tauri's own claim is about binary size (a minimal app under 600 KB) and says nothing about memory [primary, Tauri docs].

Recommendation: v0.1 ships the Win32 overlay in `eileen-hud.exe`, fed by bus events (listening, transcript so far, current jobs and their progress, what it is watching). Keep the HUD's state model in `eileen-contracts` so the renderer can be swapped. Measure all three with `measure-idle.ps1` before anyone argues for a different one: build one at a time with `CARGO_BUILD_JOBS=4`, release profile, and record working set, private bytes and CPU while idle and while updating ten times a second.

**Notifications.** `tauri-winrt-notification` 0.8.1 and `notify-rust` 4.18.2 both compile against our tree; neither toast was shown during tests [ran 10-20]. For an unpackaged app, Windows ties toasts to an AppUserModelID, normally registered through a Start menu shortcut, so the installer must create that shortcut [primary, Microsoft docs, not re-read for this report]. The HUD covers in-the-moment status; toasts are for "a job finished while you were away".

**Secrets.** Owner direction allows exactly one API key (DeepSeek) plus, later, connector tokens (a Home Assistant long-lived token, phone pairing keys). Store them with DPAPI (`CryptProtectData`, user scope) in Eileen's own data directory, or in Windows Credential Manager through keyring-core 1.0.1 and windows-native-keyring-store 1.1.0; both work here (8 to 17 ms and 3 to 13 ms) [ran 10-20]. Eileen never reads vendor credential files; the CI grep enforces it.

**Config.** Use TOML (`toml` 1.1.8). ARCHITECTURE.md currently specifies YAML for `routing.yaml`, `announcements.yaml` and `permissions.yaml`. The Rust YAML options are weak: `serde_yml` is unsound and unmaintained (RUSTSEC-2025-0068), `serde_yaml` is deprecated, `unsafe-libyaml` and `yaml-rust` carry unmaintained advisories, and only `serde-saphyr` 2.0.0 is clean [primary, RustSec database]. Switching to TOML needs a decision record.

## 9. Parallel builds on this 16 GB PC

The build phase will run several engineer agents at once. The measurements say why this needs rules:

- Cargo defaults to one job per logical CPU: 22 here. One `rustc` was seen at 417 MB [ran 10-20]; a clean build ran about 9.4 units at once on average.
- One clean dev build, beside other agents doing non-build work and the Claude app's 4 GB, took free RAM down to 462 MB and ran 5.3 times slower than the same build on 10-11 [ran 10-11, ran 10-20].
- At 13:55 on 10-20, with a fat-LTO release build and other agents running, free RAM reached 0 MB and the session ended [ran 10-20]. An earlier attempt thrashed the PC the same way by running Rust builds and speech models together.
- Disk is tight too: 19.4 GB free, about 1 GB per worktree after one clean build and 3.3 GB after a week of use.

Recommendations [inferred from the above unless marked]:

1. **At most two agents compile at the same time. Only one if a release build or any speech-model process is running.** Other agents read, write docs and design while they wait. Two agents at 6 jobs means at most 12 compiler processes, roughly 4 to 5 GB at peak, which leaves room for the Claude app and the OS.
2. **`CARGO_BUILD_JOBS=6` for every agent**, set by the orchestrator in the agent's environment or in the owner's user-level `%USERPROFILE%\.cargo\config.toml` (`[build] jobs = 6`). Not in the repository's `.cargo/config.toml`, which would also throttle contributors with bigger machines and CI. An agent alone on the PC may use 8.
3. **Enforce it in `cargo xtask`**: before running cargo, take one of two slot locks (`%LOCALAPPDATA%\eileen\build-slot-0.lock` and `-1.lock`, held with `File::try_lock`, released by the OS if the process dies), wait if both are taken, refuse to start while free RAM is under 2 GB, set `CARGO_BUILD_JOBS` to 6 if unset, and lower its own priority class to below normal so the children inherit it and the voice loop and the Claude app stay responsive. Speech benchmark scripts take a slot too. Agents are told to build only through `cargo xtask`.
4. **One shared sccache for all worktrees.** `RUSTC_WRAPPER=sccache`, `SCCACHE_DIR=%LOCALAPPDATA%\sccache`, `SCCACHE_CACHE_SIZE=8G`, one local server for every worktree. Cargo compiles workspace members incrementally, and sccache does not cache incremental units, so sccache naturally caches third-party crates (most of the 215 units) and leaves our own crates to incremental compilation. Install sccache 0.18.0 prebuilt; it is not installed today. Its hit rate on this PC is unmeasured: check `sccache --show-stats` after the second worktree's first build. Whether it also caches the `cc` crate's C compile of SQLite is unverified.
5. **A target directory per worktree** (Cargo's default `<worktree>\target`), not one shared `CARGO_TARGET_DIR`. Cargo locks a build directory for the whole build, so a shared one serialises the agents anyway and turns waiting into blocked processes; sccache gives the cross-worktree reuse without the lock. Delete a worktree's target directory when its branch merges. Cap the number of live worktrees with build output at three while disk is under 25 GB free.
6. **Debug info**: keep `debug = "line-tables-only"` for our crates and add `debug = false` for dependencies (section 6.7). Keep incremental compilation on locally and off in CI (already set).
7. **Inner loop on `cargo xtask check` and `clippy`**, not `build`. Build only the binary you need (`-p eileen-desktop`); separate binaries make that cheap.
8. **No local `dist` (fat LTO) builds by agents.** Release artifacts come from CI. If a local release binary is needed, use the thin `release` profile with one slot and nothing else compiling.
9. **Never `cargo install` tools from source**: 59 minutes and four rustc trees for four tools [ran 10-11]. Use prebuilt binaries.
10. **Do not start a 20-minute build at the end of an agent's session.** A backgrounded build dies with the session that started it; the 10-20 release build and its measurements were lost that way [ran 10-20].
11. **Optional, the owner's call**: a Windows Dev Drive for target directories and the sccache directory speeds up builds by relaxing antivirus scanning, but needs at least 50 GB of free disk, which this PC does not have today [inferred]. Any Defender exclusion is a security setting for the owner to decide, not something an agent changes.

## 10. Pitfalls

1. **A hung cross-process COM call cannot be cancelled in-process.** UIA's `next_sibling` blocked 38.7 minutes despite the 20 s default transaction timeout [ran 10-20]. Isolate and watchdog; never call UIA from the core.
2. **Fat LTO plus one codegen unit on a loaded 16 GB PC** ran for over 12 minutes without finishing and coincided with the PC running out of memory [ran 10-20].
3. **`cargo install` from source** took 59 minutes for four tools [ran 10-11].
4. **Pinning `channel = "1.99.0"` when the installed toolchain is "stable" (also 1.99.0)** makes rustup download and install a second, identical toolchain the first time [ran 10-11]. Harmless, but it costs disk and minutes; do it once in `xtask setup`.
5. **`windows-core` 0.100.0 does not mix with `windows` 0.62.2.** Do not add the 0.100 focused crates while the umbrella crate is 0.62.2; take core types from `windows::core` [primary, app-hooks report].
6. **The ACP crate's `process` feature on Windows** does not job-wrap or group the child and adds a second async reactor (section 7) [primary].
7. **Cargo feature unification changes JSON key order.** ACP enables `serde_json/preserve_order`, which silently changed the key order of a schemars snapshot in `eileen-contracts`; the snapshot test failed until it was re-recorded [ran 10-11]. Snapshot tests must not depend on map order, or the order must be fixed explicitly.
8. **rmcp's `#[tool_handler]`** without `router = self.tool_router` calls `Self::tool_router()` and rebuilds the router on every request, and leaves the `tool_router` field looking unused (a `dead_code` warning, an error under `-D warnings`). Write `#[tool_handler(router = self.tool_router)]` [ran 10-11].
9. **The 10-11 lint table had three mistakes**: `module_name_repetitions` now sits in the restriction group, `format_push_string` was removed from the table (the notes do not record why), and `self_named_module_files` does the opposite of what its comment said (it bans `foo.rs` plus `foo/`, while `mod_module_files` bans `foo/mod.rs`). Clippy's `lints.json` endpoint now returns 404; check names against the lint index page [ran 10-20].
10. **`unnecessary_safety_comment` fires on the word "SAFETY:" in module documentation**, and `rustdoc::private_intra_doc_links` fires when public docs link a private `ffi` module. Write "safety comment" in prose and refer to the module without a link [ran 10-11].
11. **ulid 3** has `Ulid::generate()`, not `Ulid::new()`, and needs `features = ["std", "serde"]` with default features off [ran 10-11].
12. **keyring 4** renamed its features (`windows-native` no longer exists) and is now glue for its CLI. Depend on `keyring-core` plus `windows-native-keyring-store` [ran 10-20].
13. **Python `mcp` 2.x renamed `FastMCP` to `MCPServer`**; the SDK's import cost is the bigger problem (section 7) [ran 10-11, ran 10-20].
14. **`tray-icon` defaults to the Linux `libappindicator` feature.** Use `default-features = false` [primary].
15. **`just`'s Windows shell defaults assume `pwsh`**, which is not installed on this PC [ran 10-11].
16. **Load samplers built on `Get-CimInstance` fail with "Call cancelled" under heavy load** [ran 10-20]. Sample with `Get-Process` or performance counters when measuring a busy machine.
17. **The bundled SQLite C build** sits on the critical path of every clean build (666 s under load) [ran 10-20]. Keep it cached (sccache, or a target directory that is not cleaned needlessly).

## 11. Open risks

- **Release binary size, release idle footprint and fat-LTO build time and memory are unmeasured.** The first CI run on `windows-latest` will give the size and build time on a quiet 4-core runner; measure idle locally once a release binary exists.
- **ws-next has never compiled.** The supervisor, the `ByteStreams` ACP driver, the new clippy rules and the in-process MCP test are written, not run. The first engineering task should be to build it with one slot and fix what breaks.
- **The HUD choice is unmeasured** (section 8).
- **All 10-20 build times were taken under contention.** A quiet-machine baseline (one agent, nothing else running, `CARGO_BUILD_JOBS=8`) is needed before anyone tunes profiles.
- **sccache on Windows MSVC is untested here**, including whether the `cc` crate's C compile is cached. An option worth testing instead of the bundled SQLite build is linking the SQLite that ships with Windows (`winsqlite3`, a libsqlite3-sys feature); its version and feature set would need checking [inferred].
- **The root cause of the UIA hang is unknown.** No top-level window reported hung; the call blocked after the desktop window. The watchdog contains it, but the desktop process will restart now and then, and DesktopState must tolerate a gap.
- **The speech process's IPC latency is unmeasured.** Audio frames over a named pipe should cost well under a millisecond, but this needs measuring with real frame sizes before the split is final.
- **Process start is slow on a loaded PC** (bare Python 4.5 to 6.9 s; a 14 MB debug Rust binary 0.6 to 5.1 s to finish an MCP handshake). Adapters must be resident; per-request spawning would ruin the feel the owner asked for.
- **ort is still a release candidate** (2.0.0-rc.15), and native inference libraries are outside cargo-deny's view.
- **Protocol crates move weekly** (rmcp: six releases in six weeks). Pin exact versions in the workspace, update deliberately, and keep the probe subcommands as smoke tests.
- **ARCHITECTURE.md still describes YAML config files and LiteLLM for direct model calls.** Both conflict with this report (TOML) and with the owner's direction (Rust core, subscriptions, DeepSeek as the only key); they need decision records.
- **Disk**: 19.4 GB free cannot hold three worktrees at 3.3 GB each plus an 8 GB sccache and the existing scratch target directories for long.

## 12. Sources

Registries and release pages, read 2026-10-11 and 2026-10-20:

- crates.io API, per crate: `https://crates.io/api/v1/crates/<name>` (saved responses in `versions\`)
- windows-rs: https://github.com/microsoft/windows-rs (README on umbrella versus focused crates) and https://github.com/microsoft/windows-rs/releases
- rmcp: https://github.com/modelcontextprotocol/rust-sdk/releases
- Agent Client Protocol Rust SDK: https://github.com/agentclientprotocol/rust-sdk and https://github.com/agentclientprotocol/rust-sdk/releases
- global-hotkey changelog: https://github.com/tauri-apps/global-hotkey/blob/dev/CHANGELOG.md
- keyring 4.2.0: https://docs.rs/keyring/4.2.0/keyring/
- process-wrap: https://crates.io/crates/process-wrap
- PyPI `mcp`: https://pypi.org/project/mcp/ and the 2.x migration guide: https://py.sdk.modelcontextprotocol.io/v2/migration/#fastmcp-renamed-to-mcpserver
- npm `@agentclientprotocol/claude-agent-acp`: https://www.npmjs.com/package/@agentclientprotocol/claude-agent-acp
- GitHub runner images: https://github.com/actions/runner-images and https://github.com/actions/runner-images/blob/main/images/windows/Windows2025-Readme.md
- RustSec advisories: https://rustsec.org/advisories/RUSTSEC-2025-0068.html (serde_yml), https://rustsec.org/advisories/RUSTSEC-2024-0320.html (yaml-rust), https://rustsec.org/advisories/RUSTSEC-2024-0449.html (unsafe-libyaml)

Documentation:

- Cargo resolver and MSRV-aware resolution: https://doc.rust-lang.org/cargo/reference/resolver.html
- Cargo profiles and override precedence: https://doc.rust-lang.org/cargo/reference/profiles.html (not re-read for this report)
- Clippy lint index: https://rust-lang.github.io/rust-clippy/stable/index.html
- UIA transaction timeout: https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_transactiontimeout
- Toasts from unpackaged desktop apps: https://learn.microsoft.com/en-us/windows/apps/design/shell/tiles-and-notifications/send-local-toast-desktop-cpp-wrl
- Tauri size claim: https://v2.tauri.app/start/
- cargo-udeps needs nightly: https://github.com/est31/cargo-udeps
- sccache: https://github.com/mozilla/sccache
- cargo-nextest: https://nexte.st and cargo-deny: https://github.com/EmbarkStudios/cargo-deny

Source code read from the local Cargo registry: rmcp 3.5.1 and 3.6.0 (`transport/child_process.rs`, tests, features), agent-client-protocol 3.3.0 and 3.4.0 (`acp_agent.rs`, `jsonrpc.rs`, features), process-wrap 10.0.1, ulid 3.0.0, windows-sys 0.61.2, windows 0.62.2 (DPAPI signatures), tray-icon 0.26.1, muda 0.21.2, global-hotkey 0.8.0, windows-native-keyring-store 1.1.0, eframe 0.36.2.

## 13. Reproducing

All commands ran from Git Bash with `export PATH="$USERPROFILE/.cargo/bin:$PATH"` and an explicit `CARGO_TARGET_DIR`. Run them one at a time with nothing else compiling, and with `CARGO_BUILD_JOBS=6`.

- Gate: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo nextest run --workspace --locked`, `cargo test --workspace --doc --locked`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`, `cargo deny check`, `cargo machete`.
- Clean build with timings: `cargo build --locked --timings` into an empty target directory; the HTML report's `UNIT_DATA` gives per-unit times.
- Idle: `powershell -File scripts\measure-idle.ps1 -Exe <eileen.exe> -ArgList run,--seconds,90,--db,<file>,--muted -Warmup 5 -Samples 15`.
- MCP interop: `eileen mcp-probe --tool jobs_list -- <eileen.exe> mcp-jobs --db <file>`, and `eileen mcp-probe --tool resolve_status -- <venv python> -I <adapter.py>`.
- ACP: `eileen acp-probe -- <node.exe> <adapter>\dist\index.js`, with a throwaway HOME so no real sign-in is touched.
- UIA, DPAPI, keyring, toast and config probes: `compat.exe` (release).

The temporary scratch directory still holds several GB of target directories (`target-dev`, `target-m1`, `target-clean`, `target-compat`, and the partial `target-rel`). Delete them once nobody needs the build outputs.
