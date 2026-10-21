# Decisions

Architecture decision log for Eileen. One entry per decision, in the order they were made. Every entry points to the part of [RESEARCH.md](RESEARCH.md) that supports it. To change a decision, add a new entry that supersedes the old one; do not edit history.

## D1. Compose, do not fork

- Date: 2026-09-19
- Status: accepted
- Decision: Eileen is a small new core that integrates existing projects over MCP and ACP, not a fork of OpenClaw or Hermes.
- Why: OpenClaw is a 400k-line monorepo with a one-click RCE (CVE-2026-25253, 40k exposed instances) and nine CVEs in four days in March 2026. Hermes had a spring 2026 CVE cluster and two database-corruption hotfixes this month. A fork inherits that reputation and a moving target. Every layer we need exists as a separately licensed component.
- Consequences:
  - We write integration code, not another agent framework.
  - Designs are borrowed without the code: Hermes voice pipeline, Agent Zero containment, IronClaw credential boundary.
  - OpenClaw stays available as an optional gateway (see D12).
- Source: [section 1](RESEARCH.md#1-the-short-version), [section 2.1](RESEARCH.md#21-closest-projects), [section 7](RESEARCH.md#7-what-to-avoid-or-watch)

## D2. The voice loop lives outside every agent

- Date: 2026-09-19
- Status: accepted
- Decision: The voice loop runs in its own process, owns the microphone, and treats each agent session as an async job.
- Why: An MCP voice tool inside an agent cannot listen while a tool call runs. The one shipped example of the target UX, GPT-Live with Codex, keeps the two apart. claude-call and backtalk show the same shape at toy scale. So there are two loops: a fast conversational loop that never blocks, and agent loops dispatched as jobs.
- Consequences:
  - The job registry and event bus are prerequisites for every harness adapter.
  - The conversational model sees a recency-ordered jobs block, so "how's that simulation doing?" resolves to a job.
  - Announcements follow the policy: approval or blocked interrupts, completed waits for idle, progress stays silent unless asked.
- Source: [section 4](RESEARCH.md#4-voice)

## D3. Agent Client Protocol is the harness contract

- Date: 2026-09-19
- Status: superseded by D19
- Decision: Eileen is an Agent Client Protocol client, and every coding harness is reached through its ACP adapter.
- Why: Adapters exist for Claude Code (claude-agent-acp), Codex (codex-acp), Gemini CLI, OpenCode, Cursor and Droid. Goose, OpenClaw and OpenHands already consume them. One protocol gives one job model and one event mapping for every harness.
- Consequences:
  - v0.1 ships claude-agent-acp and codex-acp; Gemini CLI and OpenCode follow.
  - Harness hooks publish TaskStarted, Progressed, Blocked, ApprovalRequested and Completed to the event bus.
  - Parallelism comes from many sessions or cloud tasks, since Claude Code agent teams do not spawn in headless mode.
- Source: [section 6](RESEARCH.md#6-orchestration-and-routing)

## D4. Never handle vendor credentials

- Date: 2026-09-19
- Status: accepted; amended by D18 (API keys) and D19 (worker names)
- Decision: Eileen spawns the unmodified official harness binaries in the user's own environment, never touches vendor credentials, and treats API keys as first-class.
- Why: Anthropic prohibits raw OAuth token use by third-party wrappers and enforced that three times in 2026. Driving the unmodified `claude` binary through the Agent SDK or `claude -p` is permitted today. The stance changed five times this year, so users need a switch that disables subscription-backed workers. OpenAI has no written rule, which is permission by silence, not by grant.
- Consequences:
  - No OAuth flows, no reading `~/.claude` or `~/.codex/auth.json`, no token proxying or pooling.
  - API keys, Bedrock, Vertex, Foundry and `CODEX_API_KEY` work from day one.
  - Workers are called "Claude Agent" or "Codex", never "Claude Code <anything>".
- Source: [section 3](RESEARCH.md#3-licensing-and-subscriptions-settled)

## D5. Cua Driver is the Windows action layer

- Date: 2026-09-19
- Status: superseded by D21
- Decision: Cua Driver (trycua/cua) is the Windows action layer.
- Why: It is MIT, a Rust host daemon with Python and TypeScript SDKs and an MCP server with 57 tools. It runs UI Automation first with pixel fallback, and its synthetic cursor leaves the user's pointer alone. Commits land daily, and OpenClaw and Hermes both build their computer use on it.
- Consequences:
  - We do not write our own input injection or UIA action layer.
  - Chromium and Electron targets need foreground delivery, so the escalation ladder must handle that case.
  - Recording and replay come with the driver.
- Source: [section 1](RESEARCH.md#1-the-short-version), [section 2.2](RESEARCH.md#22-windows-action-and-grounding-components)

## D6. Our own DesktopState grounding service

- Date: 2026-09-19
- Status: accepted; amended by D21
- Decision: We write DesktopState, a small Windows service on Win32, DWM and UI Automation events, exposed over MCP.
- Why: Nothing exposes focused element, current selection, monitor model and focus history as one state object. Settings' "Identify" monitor numbers are not available from any public API, so we need our own ordinals. Everything required is public, stable Win32. Reference code: terminator, Windows-MCP, mcp-windows and UFO, all MIT.
- Consequences:
  - It is build item 1 of v0.1: `desktop.state`, the `window.*` verbs and a focus event stream.
  - Per-monitor-V2 DPI aware, every rect in physical pixels, UIA calls wrapped in timeouts.
  - Selection comes from TextPattern with a clipboard-sequence-number fallback.
- Source: [section 2.2](RESEARCH.md#22-windows-action-and-grounding-components), [section 9](RESEARCH.md#9-build-order-for-v01)

## D7. Playwright MCP with the browser extension

- Date: 2026-09-19
- Status: accepted; amended by D22
- Decision: Playwright MCP with the Playwright browser extension is the browser layer; Chrome DevTools MCP is the Chrome-only alternative.
- Why: It is the sanctioned way to drive real logged-in Chrome and Edge tabs, and it is Apache-2.0. Since Chrome 136, raw CDP against the default profile is refused unless the user opts in. Chrome DevTools MCP needs Chrome 144+ and does not support Edge.
- Consequences:
  - DOM-level grounding comes from the extension path; UI Automation is reserved for browser chrome.
  - Selection is a small JS snippet plus Ctrl+C, since no tool exposes it as a field.
  - Node 24 is a runtime dependency.
- Source: [section 2.3](RESEARCH.md#23-browser)

## D8. Deterministic first, vision last

- Date: 2026-09-19
- Status: accepted; amended by D23
- Decision: Actions climb a fixed ladder: App Actions, COM and app scripting, UI Automation, browser DOM, MCP servers, and only then screenshots and mouse.
- Why: Deterministic hooks are fast, testable and usually reversible. Multi-step vision runs take tens of minutes and the best open Windows result is 56.6 percent. Vision stays as the last rung, through Anthropic's computer toolset, for cases where no interface exists.
- Consequences:
  - Office COM, Explorer Shell, SMTC, Resolve and Vegas adapters are first-class modules under `apps/`.
  - The vision loop is Cua Driver's screenshot-plus-input path, not our own.
  - Later: register with the Windows On-Device Registry and prefer App Actions when an app publishes them.
- Source: [section 2.4](RESEARCH.md#24-app-hooks-that-beat-clicking), [section 2.5](RESEARCH.md#25-vision-fallback)

## D9. Python core, native sidecar, MCP between processes

- Date: 2026-09-19
- Status: superseded by D17 and D38
- Decision: The core is Python 3.12+ (package `eileen`, src layout, pydantic, pytest, ruff); the DesktopState sidecar is Rust or C#; processes talk over MCP.
- Why: Python matches the voice stack, LiteLLM and the Agent SDK. UIA events need an MTA thread and a native library. uiautomation-rs and FlaUI.UIA3 expose events; the Python uiautomation library does not, and pywinauto has had one release in six years. MCP is already the interface of Cua Driver, Playwright MCP and the On-Device Registry.
- Consequences:
  - The sidecar language is chosen by its first implementer; the Rust toolchain is not installed on the owner's machine today.
  - Every cross-process boundary is MCP, so any component can be swapped.
  - Node 24 stays for Playwright MCP.
- Source: [section 2.2](RESEARCH.md#22-windows-action-and-grounding-components), [section 9](RESEARCH.md#9-build-order-for-v01)

## D10. MIT license

- Date: 2026-09-19
- Status: accepted
- Decision: Eileen is released under the MIT license.
- Why: Every component we build on is MIT, Apache-2.0 or BSD-2, and MIT stays compatible with all of them. Anyone can run, modify and redistribute it. Copyleft and source-available tools (komorebi, GlazeWM, screenpipe, obs-websocket, AutoHotkey) are never bundled; they are spawned or reached over IPC.
- Consequences:
  - `LICENSE` at the repo root is MIT.
  - No GPL, AGPL or source-available code in the tree; optional user-installed backends only.
  - OmniParser, if ever used, takes the MIT YOLOv9-E weights, not the AGPL YOLOv8 ones.
- Source: [section 1](RESEARCH.md#1-the-short-version), [section 2.2](RESEARCH.md#22-windows-action-and-grounding-components), [section 7](RESEARCH.md#7-what-to-avoid-or-watch)

## D11. Phone reach and off-PC work

- Date: 2026-09-19
- Status: accepted; amended by D37 (phone channels move to v0.2)
- Decision: Phone reach is a Telegram bot, ntfy push and Tailscale; off-PC work uses Claude Code cloud sessions and Codex cloud before any self-hosted infrastructure.
- Why: Telegram is free, long polling works behind home NAT, and voice notes go both ways. ntfy is Apache-2.0 and self-hostable with action buttons; Tailscale Personal is free. Claude Code cloud sessions come with Pro and Max at no compute charge, and Codex cloud tasks are included in plans, so jobs survive the PC being off. WhatsApp needs a public webhook and a business portfolio, and iMessage has nothing programmable on Windows.
- Consequences:
  - Presence detection routes announcements to the phone when the user is away.
  - A Discord bot covers live two-way voice when wanted.
  - A VPS or OpenClaw cloud workers are later options, not day one.
- Source: [section 5](RESEARCH.md#5-phone-and-off-pc-execution)

## D12. OpenClaw only as an optional gateway, later

- Date: 2026-09-19
- Status: accepted
- Decision: OpenClaw Gateway is an optional later integration with Eileen as a node; it is never the core.
- Why: OpenClaw has 20+ channels, mobile apps, cron, sub-agents and cloud workers. But voice wake and talk mode are documented for macOS, iOS and Android only, the gateway is recommended on WSL2, Windows computer use is experimental, and the secret store is not encrypted at rest. A node integration gets the commodity layer without the monorepo.
- Consequences:
  - Build order item 6, after Telegram and ntfy already work.
  - Nothing in `core/` may depend on OpenClaw.
- Source: [section 1](RESEARCH.md#1-the-short-version), [section 2.1](RESEARCH.md#21-closest-projects)

## D13. The user is the supreme router

- Date: 2026-09-19
- Status: accepted; amended by D20 (no LiteLLM) and D29 (TOML, not YAML)
- Decision: A YAML policy maps task kind to harness and model; a spoken override wins over it immediately.
- Why: Policies are defaults, not prisons. LiteLLM (MIT) supports per-request overrides for direct model calls. The conversational model parses "send this straight to Claude", "keep this local" or "have all three debate it" into a one-off override.
- Consequences:
  - The router in `core/` applies policy, then override, and records which one decided.
  - "Debate" fans one task out to several harnesses and collects the results.
- Source: [section 6](RESEARCH.md#6-orchestration-and-routing)

## D14. Permission tiers

- Date: 2026-09-19
- Status: accepted; amended by D40 and D43
- Decision: Reversible actions execute immediately; consequential actions confirm first.
- Why: Moving a window, opening a site, running tests or drafting an email can be undone, so they need no prompt. Sending, deleting and spending money cannot, so they wait for a yes. OpenClaw's approval gates, Hermes's hard-blocked key combos and Cua Driver's foreground escalation ladder show the idea in production.
- Consequences:
  - Every tool declares its tier; an undeclared tool is treated as consequential.
  - ApprovalRequested is a first-class bus event that interrupts the voice loop, or goes to the phone when the user is away.
  - Sandbox by default, secrets never enter model-visible text, encrypt at rest.
- Source: [section 4](RESEARCH.md#4-voice), [section 6](RESEARCH.md#6-orchestration-and-routing)

## D15. "Eragon" is a working name

- Date: 2026-09-19
- Status: superseded by D16
- Decision: "Eragon" is the working name; the shipping name is picked later from the shortlist in [NAMING.md](NAMING.md).
- Why: eragon.ai is a live, funded startup in the same category. The name is Paolini's coinage in active multi-class commercial use, with rights held by Paolini International LLC and Disney. "Hey Eragon" is a near-homophone of "Oregon". GitHub renames repositories with redirects, so the choice does not block work.
- Consequences:
  - Use "Eragon" everywhere in code and docs; the README points newcomers to NAMING.md.
  - Expect an org handle like `<name>-ai`, since every candidate handle is already taken.
  - Friends weigh in through a GitHub discussion or the naming issue.
- Source: [section 8](RESEARCH.md#8-name)

## D16. The product is named Eileen

- Date: 2026-10-11
- Status: accepted
- Supersedes: D15
- Decision: The product is named Eileen. The owner chose the name on 2026-10-11, and the choice is final.
- Why: D15 kept "Eragon" as a working name only, because eragon.ai is a funded startup in the same category and the word belongs to the Paolini and Disney franchise. The owner has now picked the shipping name. Renaming before the first commit and before the repo goes public costs nothing.
- Consequences:
  - The Python package, the console script and the `python -m` entry point are all `eileen`.
  - Environment variables use the `EILEEN_` prefix, and the data directory is `%LOCALAPPDATA%\eileen`.
  - MCP server names and notification namespaces use `eileen`, for example `eileen-desktop` and `notifications/eileen/desktop_event`.
  - The repository is `github.com/Sixteen1-6/eileen`.
  - Earlier entries in this log now say Eileen where they named the product. D15 keeps its wording because the old name is its subject.
  - The shortlist in RESEARCH.md section 8 is closed; Eileen was not on it. That section stays as the record of why Eragon was dropped.
- Source: owner decision, 2026-10-11; [section 8](RESEARCH.md#8-name) for the Eragon collisions.

## D17. Rust core with supervised sidecars

- Date: 2026-10-20
- Status: accepted
- Supersedes: D9
- Decision: The always-on core is one Rust process (`eileen.exe`) that owns the microphone, mute, the event bus, the job registry, permissions, learning and a supervisor, and links nothing that can hang. Speech models, UI Automation, app COM, the HUD and every Claude session run in child processes started in kill-on-close Job Objects, with a deadline on every call and a heartbeat on every sidecar.
- Why: The owner asked for a fast Rust core. One UI Automation call on the owner's PC blocked for 38.7 minutes, past UIA's own 20 s timeout, and a blocked COM call cannot be cancelled in-process; only a process boundary and a kill recover. Rust's measured wins are start-up, idle footprint (the skeleton idled at 26.6 MB), hook and UIA latency and single binaries, not model latency.
- Consequences:
  - One Cargo workspace (resolver 3, Rust 1.99.0, edition 2024), crates and owners as in BUILD.md section 3.1.
  - `eileen_supervisor` is the only place a child process starts; clippy bans `Command::new` elsewhere.
  - The core's logic is a sans-IO state machine (D31).
- Source: [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) sections 1, 3 and 5; BUILD.md section 2.1.

## D18. Subscriptions only; Claude is the v0.1 route

- Date: 2026-10-20
- Status: accepted
- Amends: D4 (its credential rule stands; its API-key and naming consequences are replaced)
- Decision: Models run through each vendor's own signed-in CLI on the owner's plans. Claude, through the installed `claude` CLI, is the brain and every worker in v0.1. Codex is added when the owner installs it; Grok and DeepSeek later. A DeepSeek API key is the only API key Eileen may ever store. There is no Gemini route.
- Why: Owner direction of 2026-10-11 and 2026-10-20. Anthropic permits an end user to sign in to the unmodified binary when another product runs it, and counts that usage against the plan. Google's consumer terms exclude third-party agents, and the owner allows no Gemini key.
- Consequences:
  - Eileen never runs a login flow and never reads, copies or refreshes vendor credential files; `eileen doctor` checks `claude auth status` by exit code only.
  - Workers get a constructed environment with the parent's `CLAUDE*`, `ANTHROPIC*` and `MCP_*` variables removed.
  - A kill switch for subscription-backed workers stays in the config, because vendor policy has changed often.
- Source: [2026-10-subscription-harnesses.md](research/2026-10-subscription-harnesses.md) sections 3 and 8; [OWNER-DIRECTION.md](OWNER-DIRECTION.md).

## D19. Workers are named by route; native Claude driver; ACP as a slot

- Date: 2026-10-20
- Status: accepted; amended by D41
- Supersedes: D3, and D4's worker-naming consequence
- Decision: Every worker route implements one `WorkerDriver` trait. v0.1 builds the native Claude driver over `claude -p` stream-json. The Codex app-server driver and a generic ACP driver (for Grok and others) are designed slots behind the same trait, not built in v0.1. Workers are named by route only (Claude, Codex, Grok, DeepSeek) in every UI string, log line and document.
- Why: The Claude and Codex ACP adapters are Node wrappers around the same native protocols, add about 99 MB and a second per session, and hide features Eileen needs (the permission-prompt tool, partial messages, interrupts, rate limits). Anthropic's branding rules forbid using its product name as part of another product's or feature's name.
- Consequences:
  - The ACP crate's `process` feature is never used (it does not job-wrap on Windows); every vendor process is spawned by the supervisor.
  - `Route::display_name` is the only place a worker's user-facing name is defined; a tidy check enforces the naming rule.
- Source: [2026-10-subscription-harnesses.md](research/2026-10-subscription-harnesses.md) sections 1, 5 and 7; [2026-10-claude-mods.md](research/2026-10-claude-mods.md) section 5.

## D20. The brain is a warm Claude session with a hot spare, behind a deterministic fast path

- Date: 2026-10-20
- Status: accepted; amended by D41
- Supersedes: D13's use of LiteLLM and the YAML policy file; ARCHITECTURE.md's 2026-09 conversational model
- Decision: The conversational brain is a warm `claude -p` stream-json process on the owner's plan, started at boot with connectors and user MCP servers off, thinking off, a short system prompt and only Eileen's MCP tools, plus a hot spare. Common commands bypass it through the deterministic fast path of the intent resolver (D28). Depth tiers (instant, quick, considered, deep) run in two speeds: the brain answers at once and a deep worker runs in parallel.
- Why: Cold start measured 39 to 48 s on the loaded PC; a warm turn reached first text in 1.0 to 1.5 s with connectors off and two to four times slower with them on. A per-utterance spawn would ruin the feel.
- Consequences:
  - Model ids are full ids read once from `system/init`, never aliases.
  - The brain never drives the mouse; computer use is always a worker job.
  - Spoken overrides still beat policy (D13's principle stands), recorded on the job with the words that caused them.
- Source: [2026-10-brain-latency.md](research/2026-10-brain-latency.md); BUILD.md sections 2.3 and 2.4.

## D21. Eileen's own Rust desktop server is the Windows action layer

- Date: 2026-10-20
- Status: accepted (the owner may overturn it on review)
- Supersedes: D5
- Amends: D6 (DesktopState lives in this server; tool names are snake_case)
- Decision: The Windows action layer is Eileen's own Rust MCP server (`eileen-desktop`, rmcp plus the `windows` crate), ported from the research proof. Cua Driver stays an MIT reference for MSAA fallback, PostMessage delivery and Electron drags, not a dependency.
- Why: The proof covered grounding, UIA actions, capture, input and app launch with 12 tools in an 8.8 KB tool list and grounded faster than Cua Driver 0.37.0 (63 tools, a 216 KB list, about 95 MB of binaries, telemetry on by default, and no password-field or blocked-combo refusals). Owning the layer puts the safety rules, one input lock across parallel workers and step-level recording for learning in one place.
- Consequences:
  - Action order: UIA pattern, then value or clipboard for text, then `SendInput` with honest foreground escalation, then vision with marks.
  - Safety rules live in the server, so they apply to every worker, skill and connector.
  - Any third-party component Eileen runs has its telemetry off.
- Source: [2026-10-computer-use.md](research/2026-10-computer-use.md) sections 1, 2, 4, 7 and 9.

## D22. Browser and media: URL launch and SMTC first

- Date: 2026-10-20
- Status: accepted
- Amends: D7
- Decision: Known videos and pages open with `ShellExecuteW` in the user's own browser, and playback is controlled through Windows' system media controls from Rust. Page content goes through screenshots and foreground input in v0.1, or through the Playwright MCP extension when the user installs it. Claude in Chrome is used only inside a Claude worker, by user opt-in. YouTube is never searched headlessly.
- Why: Measured on the owner's PC: a `&t=930s` URL landed at 15:30 and SMTC pause and seek from Rust took effect, with no extension. YouTube's robots.txt disallows headless search, and yt-dlp took seconds idle and minutes under load.
- Consequences: Eileen never relaunches the user's browser profile with a debug port; after a load or seek it waits for the media session to return with the expected title (ads).
- Source: [2026-10-app-hooks.md](research/2026-10-app-hooks.md) sections 0 and 5.

## D23. App hooks are resident Rust; Resolve through a Lua bridge

- Date: 2026-10-20
- Status: accepted; amended by D48
- Amends: D8 (its ladder stands; its list of adapters changes)
- Decision: One resident Rust process, `eileen-apps`, holds every app adapter: SMTC with one long-lived manager, Office and Explorer over late-bound COM on per-target STA threads, the recent-work index, and DaVinci Resolve through a small Lua bridge run by Resolve's own `fuscript.exe`, supervised and gated on Resolve running. Absolute timecodes are computed in Rust. Adobe apps get Eileen-owned UXP plugins in v0.2. On Resolve 21.1 or later, Resolve's own MCP server is offered to workers and measured against the bridge.
- Why: Warm state beats fast calls: under load, spawns and fresh system-object requests degraded by orders of magnitude while resident handles stayed at about a microsecond. Resolve needs no Python of Eileen's own.
- Consequences: Python is not in the v0.1 build (D38).
- Source: [2026-10-app-hooks.md](research/2026-10-app-hooks.md) sections 1, 2, 6 and 8.

## D24. A local voice pipeline in Rust, CPU only

- Date: 2026-10-20
- Status: accepted; amended by D42
- Decision: cpal on WASAPI with sonora echo cancellation and a mute that stops capture, in the core; Silero VAD v6.2, Nemotron streaming 0.6B (int8, 560 ms), Smart Turn v3.2 fused with transcript cues and a silence cap, and Kokoro v1.0 fp32 with clause chunking and a phrase cache, in `eileen-speech` through sherpa-onnx 1.13.8 (shared) and `ort` 2.0.0-rc.13 on one shipped `onnxruntime.dll`. Barge-in ducks at the first VAD window and stops by the third. Every Eileen process opts out of EcoQoS at start.
- Why: Measured on the owner's PC: idle listening 5.8 percent of one core, Nemotron WER 2.35 percent with the final 0.4 s after speech, barge-in about 70 to 110 ms from measured parts, and a 10-fold slowdown without the EcoQoS opt-out. The iGPU, NPU and DirectML gave no usable path for Kokoro.
- Consequences: Static sherpa-onnx is not used (it fails to link on MSVC 14.42); the native DLLs ship beside the binaries, never from `PATH`.
- Source: [2026-10-voice-stack.md](research/2026-10-voice-stack.md) sections 1, 3 to 9.

## D25. Voices: Kokoro default, Pocket TTS for cloning, no watermark by default

- Date: 2026-10-20
- Status: accepted (owner decision of 2026-10-20); amended by D39 (its Chatterbox consequence predates the owner's later entry)
- Decision: Kokoro is the default voice. Kyutai Pocket TTS, through sherpa-onnx, is the cloning engine. Chatterbox is an optional pre-render extra (v0.2). No watermark is applied by default on any engine, Chatterbox's Perth included; watermarking saved or exported audio is an opt-in setting. Cloning requires a live consent enrollment by the voice's owner, checked by a speaker-embedding comparison and stored with the profile on the PC only.
- Why: Owner decision recorded in OWNER-DIRECTION.md, which overrides the dictation-and-voices report where they differ. Pocket TTS is permissively licensed, streams and already runs through Eileen's speech runtime; Chatterbox measured far heavier on CPU.
- Consequences: When cloning is too slow for live conversation, Eileen says so and uses the cloned voice where latency matters less; a watchdog falls back to Kokoro for the next reply.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [2026-10-dictation-and-voices.md](research/2026-10-dictation-and-voices.md) sections 10 and 11.

## D26. Dictation is built into Eileen

- Date: 2026-10-20
- Status: accepted; amended by D42
- Decision: Dictation uses the already-loaded streaming recognizer, Rust cleanup rules (with SpeakoFlow's conformance cases), an optional Claude scribe for polish, and an insertion ladder in the desktop server (`EM_REPLACESEL`, then a port of Handy's clipboard transaction, then paced keys), behind the same input lock as computer use, with vocabulary learned from corrections.
- Why: Owner direction for dictation better than the paid tools; Eileen already knows the focused field, its app and what is on screen, which the open-source leaders do not.
- Consequences: Handy's and SpeakoFlow's code is ported with attribution in NOTICE; GPL and AGPL dictation projects are design references only.
- Source: [2026-10-dictation-and-voices.md](research/2026-10-dictation-and-voices.md) sections 1, 5 to 9.

## D27. The learning loop

- Date: 2026-10-20
- Status: accepted; amended by D44
- Decision: Record a redacted trajectory for every job at the dispatcher and from the worker stream; a low-priority Claude librarian distills successful jobs into native `SKILL.md` skills with optional recipes of Eileen tool calls with checks, writing only through a validating tool; workers load them with `--plugin-dir`; skills climb a promotion ladder on evidence (draft, trusted, compiled recipe with one user approval, spoken fast path) and fall on failure; replay runs through the same dispatcher and permission gate as a worker and hands off on the first failed check. Version history lives in `learn.sqlite`, not in a git repository per skill. The crate is `eileen-learn`; the matcher lives in `eileen-intent`.
- Why: Owner direction: this is the main point of the product. The native route was verified on the owner's PC. SQLite history avoids `gix`'s compile cost on this PC and any dependency on a git CLI, and makes "forget" one transaction.
- Consequences: A learned skill never raises a tier or skips a confirmation; screen text enters a skill only with the user's approval; auto memory and session persistence are off in every session Eileen starts.
- Source: [2026-10-learning-loop.md](research/2026-10-learning-loop.md) sections 1, 3 and 6; BUILD.md section 2.10.

## D28. A layered intent resolver

- Date: 2026-10-20
- Status: accepted; amended by D44
- Decision: Intent resolution is its own crate, `eileen-intent`, with four layers: a deterministic grammar plus learned templates (L1, under 1 ms), a local semantic matcher with a small on-device embedding model run in the speech process (L2, hints and confirmed paraphrases only), the brain with live context (L3), and a worker that figures out new capabilities which the learning loop turns into skills (L4). Unclear requests get one clarifying question, never an error. "What can you do here?" is answered by introspection of tools, app capabilities and skills.
- Why: Owner direction "intent, not commands"; the measured matcher is microseconds per utterance; lexical hints alone matched the wrong verb, so nothing below L3 acts on an unconfirmed similarity.
- Consequences: The embedding model is adopted only if it meets its latency budget on the owner's PC; otherwise L2 runs lexical only and says so.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [2026-10-learning-loop.md](research/2026-10-learning-loop.md) section 3.4; BUILD.md sections 2.2 and 3.3.10.

## D29. Settings by voice, in versioned TOML

- Date: 2026-10-20
- Status: accepted
- Supersedes: D13's YAML policy file and ARCHITECTURE.md's YAML files
- Decision: Configuration is TOML with comments in `%LOCALAPPDATA%\eileen\config`, generated from documented types. Every setting is reachable by voice and on the HUD through one flow (propose, check when a choice could misfire, confirm in one sentence, apply, version), with "undo that", profiles and hand edits picked up by a watcher. Safety bounds: tier 3 can never be relaxed; skipping a tier-2 confirmation needs an explicit, logged decision and never covers credentials, payments, messages sent as the user or deletes outside a repository.
- Why: Owner direction "customise anything by just saying it". The Rust YAML options are deprecated or unsound; TOML parsed in 0.4 to 5.1 ms here.
- Consequences: Renaming the assistant updates the addressee gate, the recognizer hints and the brain's self-reference; the product and repository stay Eileen.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) section 8; BUILD.md section 2.11.

## D30. Claude integration only through official extension points

- Date: 2026-10-20
- Status: accepted; amended by D43
- Decision: Workers get per-job `--settings` files whose HTTP hooks send `PreToolUse` to Eileen's tier guard (fail closed, never answering allow) and report notifications, failures and stops to the loopback bus. The owner may install one opt-in plugin named `eileen` (a handoff skill, Eileen's session tools, a display-only mod) through Claude's own plugin flow. Mods are never in Eileen's critical path. Nothing patches, wraps or replaces the Claude app or turns off its updater.
- Why: Owner direction to use Claude's official mods, plugins, skills and hooks while keeping updates. Hooks are documented and were verified to fire and to block on the owner's PC; the mod API is early access and changes between releases.
- Consequences: CI validates the plugin and fails if it gains a gating hook; learned skills are not loaded into the owner's own sessions in v0.1.
- Source: [2026-10-claude-mods.md](research/2026-10-claude-mods.md) sections 1 to 6.

## D31. A sans-IO core tested by deterministic simulation

- Date: 2026-10-20
- Status: accepted; amended by D45
- Decision: `eileen-core` is a state machine with one entry point, `Core::step(now, rng, input, effects)`, over closed `Input` and `Effect` enums, with time, randomness and IDs injected and every queue bounded. `eileen-sim` drives it on one thread with simulated speech, playback, brain, workers and sidecars, injects faults at `buggify!` points, checks named invariants after every step and replays any failure from a seed or a decoded input script. Pull requests run a fixed seed set; `main` and nightly run fresh seeds. End-to-end tests inject recorded or synthesized audio at the audio boundary and use a stub harness; the owner's PC is never a self-hosted runner for this public repository.
- Why: The best-tested systems (TigerBeetle, FoundationDB, Turso) make concurrency bugs reproducible this way; the owner asked for simulation of jobs, bus, scheduling and barge-in from a seed.
- Consequences: Core logic never reads the clock or OS randomness (clippy bans them); the event log format is shared by the flight recorder, the simulator, the end-to-end harness and the learning record.
- Source: [2026-10-engineering-practice.md](research/2026-10-engineering-practice.md) sections 1 to 3.

## D32. Conventions are enforced by CI with one local command

- Date: 2026-10-20
- Status: accepted; amended by D46
- Decision: Every way of working adopted from the exemplar repositories has an automated check in `cargo xtask lint-repo` (crate guidance, the code map matching the crates, Markdown lint, links, spelling, house style and naming, commit and pull request format, generated files current, unsafe only in `ffi` modules with safety comments, documented public items, decision record format, dependency policy, the credential boundary, the platform boundary, scenario privacy). `cargo xtask ci` runs locally exactly what the pull request lane runs. One aggregate check, `all-green`, gates merges.
- Why: Owner direction: conventions enforced by CI, not just written down. Guidance files rot; the checks are the guardrail.
- Consequences: The list of checks lives in BUILD.md section 4b.3, CONTRIBUTING.md and the root AGENTS.md; each check has a test that fails on a seeded violation.
- Source: [2026-10-engineering-practice.md](research/2026-10-engineering-practice.md) sections 4.9 and 6; BUILD.md section 4b.

## D33. Guidance form: one AGENTS.md, crate docs in the code, nested files only where rules differ

- Date: 2026-10-20
- Status: accepted; amended by D46
- Decision: The root `AGENTS.md` is the canonical guide for people and every coding tool; the root `CLAUDE.md` imports it with `@AGENTS.md` and adds a short Claude-only section. Each crate's guidance (what it owns and does not own, invariants, boundaries, how to test, traps, logs) is its crate doc at the top of `src/lib.rs` or `src/main.rs`, with required headings checked by CI. A nested `AGENTS.md` (paired with a one-line importing `CLAUDE.md`) exists only where a folder's rules genuinely differ. A short root `ARCHITECTURE.md` is the code map.
- Why: The engineering-practice survey found that no best-run repository keeps a guidance file in every crate; per-directory files appear only where a subsystem has its own contract; Codex reads only `AGENTS.md` and has no import syntax; symlinks break on Windows checkouts. The owner asked not to assume a CLAUDE.md in every folder.
- Consequences: The nested files at the start are listed in BUILD.md section 4b.4; adding one needs a stated reason. `docs/ARCHITECTURE.md` moves to the repository root in WP0.
- Source: [2026-10-engineering-practice.md](research/2026-10-engineering-practice.md) sections 2.3, 4 and 5.

## D34. Observability: request IDs, a flight recorder and replay

- Date: 2026-10-20
- Status: accepted
- Decision: Typed, prefixed ULIDs (`turn_` is the request ID) and a `TraceCtx` carried on every bus envelope, frame, tool call and worker environment; span names as constants with a checked parent table; user content only in `pii.` fields; JSON log files, an in-memory flight recorder with a replayable input lane, incidents on panics, crashes, watchdog trips and spoken reports; a guard process outside the Job Object that writes minidumps of crashed or hung processes; L1 replay of recordings in the simulator; a generated code map with an event map; a read-only `dev` toolset for Eileen's own coding workers.
- Why: Owner direction: one request ID traced end to end, a flight recorder, session replay and a code map, so a bug goes from symptom to code quickly for people and agents.
- Consequences: Bare `tokio::spawn` is banned in favour of a wrapper that keeps spans; the audio callback never calls `tracing`; a redaction canary test covers every sink.
- Source: [2026-10-observability.md](research/2026-10-observability.md) sections 1 to 8.

## D35. Real use becomes tests: the community scenario corpus and the test fleet

- Date: 2026-10-20
- Status: accepted; amended by D45
- Decision: Scenarios are data in a versioned schema (`scenarios/<area>/<id>/scenario.toml`). "Save that as a test" turns a real request, success or failure, into a scenario from the flight recorder; sharing generalises and redacts it with a preview the contributor approves, then submits it from the contributor's own GitHub account. CI validates every scenario and replays it in simulation on every pull request. Contributors may opt in to `eileen test --fleet`, which runs real-desktop scenarios on their PCs and uploads anonymised results; failures are clustered, flakes tracked, and a readable catalogue with a coverage view is generated.
- Why: Owner direction: the owner's friends test on their own PCs, and everything they try should feed the test suite so it grows from real use.
- Consequences: The schema, its checks and the catalogue generator land in WP0; the replay runner in WP1; capture in WP2; sharing and the fleet in WP9. Every package ships scenarios. The owner's voice and screen never enter the public repository.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md section 4.8.

## D36. The HUD is part of v0.1

- Date: 2026-10-20
- Status: accepted; amended by D47
- Decision: A raw Win32 overlay (layered, click-through, topmost, never activated, excluded from screen capture) plus a small clickable strip, in its own process, fed with full `HudState` snapshots at most ten times a second. It shows the mic state, what is heard, what Eileen says, jobs, approvals, the input lock, the target of the next click, suggestions and indicators.
- Why: The owner asked for mute one click away and a visual layer as part of the feel; a status surface is table stakes. The raw Win32 option adds no dependencies; the egui and WebView2 alternatives stay unmeasured in the lab.
- Consequences: The HUD's layout is platform-neutral and snapshot-tested; only the renderer is Windows code.
- Source: [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) section 8; BUILD.md section 2.9.

## D37. The PC first, then outward through connectors

- Date: 2026-10-20
- Status: accepted
- Amends: D11 (the phone channels move from v0.1 to v0.2)
- Decision: v0.1 connects fully to the PC. Everything outside it (the phone, Home Assistant and Matter, other devices on the user's network, earbuds as microphones) is a connector with a manifest of tools, tiers and events, sharing the permission tiers, job model and learning loop. Room-sensing connectors are opt-in per device and processed locally. Telegram and ntfy become the first outward connector in v0.2.
- Why: Owner direction of 2026-10-20.
- Consequences: The connector manifest is a contract from the first commit; D12 (OpenClaw only as an optional gateway) is unchanged.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md section 2.14.

## D38. Windows first, not Windows only; the Python scaffold is retired

- Date: 2026-10-20
- Status: accepted
- Supersedes: ROADMAP.md's 2026-09 non-goal "macOS and Linux", and the remaining Python consequences of D9
- Decision: Every OS-touching piece sits behind traits in `eileen-platform` and is implemented in a per-area Windows crate (`eileen-audio-win`, `eileen-desktop-win`, `eileen-apps-win`, `eileen-shell-win`); a Linux port adds `-linux` crates. Platform-neutral crates build and pass their tests on `ubuntu-latest` in CI from the first commit. The Python scaffold under `src/eileen` is retired; nothing in the v0.1 build is Python, and any later Python adapter is a stdlib-only JSON-RPC shim.
- Why: Owner direction of 2026-10-20. The Python MCP SDK's import alone took 35 to 42 s on the loaded PC, against 1.7 to 2.9 s for a stdlib-only shim.
- Consequences: The last commit with the scaffold is tagged `python-scaffold-final` before WP0 deletes it.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) section 7; BUILD.md sections 2.14 and 3.2.

## D39. Voices: the engine is chosen per voice, and Chatterbox is in v0.1

- Date: 2026-10-20
- Status: accepted (owner direction of 2026-10-20)
- Amends: D25 (its consequence that Chatterbox waits for v0.2)
- Decision: Kokoro stays the default voice and Kyutai Pocket TTS the default cloner. Chatterbox (Turbo from Resemble's official ONNX graphs, Nano from a community ONNX export once its license checks out) sits behind the same `Synth` trait in v0.1, run through `ort` with no PyTorch and no Python. Each voice profile stores its engine. A head-to-head on the owner's PC (Pocket TTS against Chatterbox-Nano and Chatterbox-Turbo, cloning his own voice: time to first audio, real-time factor, memory, a listening test) is part of WP4's acceptance. An engine speaks live when its measured real-time factor allows it; otherwise it keeps announcements, summaries and read-alouds, rendering a whole line before playing it when it is slower than real time. No watermark by default on any engine; Perth stays off.
- Why: The owner's entry of 2026-10-20 makes Chatterbox selectable per voice and live when fast enough, and asks for the head-to-head; D25 was written from the earlier entry.
- Consequences: The first run's surprise path (D49) uses whichever cloner measured fast enough; `voice.toml` carries the engine and the consent record.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [2026-10-dictation-and-voices.md](research/2026-10-dictation-and-voices.md) section 10; BUILD.md section 2.8.

## D40. What contains a worker in v0.1, and what waits for v0.2

- Date: 2026-10-20
- Status: proposed (architect, after the safety review; the owner confirms or overturns)
- Amends: D14 (its "sandbox by default" and "encrypt at rest" consequences)
- Decision: In v0.1 a Claude worker is contained by: its own repository or worktree; a path guard that tiers every read and write by canonicalised path (Eileen's data folder, credential stores and browser profiles are tier 3; anything outside the job's folder and toolchains is tier 2); a shell policy that parses commands and allows only a list of build, test and local git commands without asking; per-job taint that makes network egress tier 2 once a job has both seen untrusted content and read private data; the `PreToolUse` guard failing closed; per-app consent for desktop input; and its Job Object. The CLI's own `--restricted` mode (on 2.1.304 it drops code-running tools and WebFetch unless named, confines file tools to the working folders and ignores user and project settings) becomes a second fence for every session if WP3's test shows workers still work under it; other sandbox and Windows container options are measured in WP3 and recorded. A real sandbox and encryption at rest (a DPAPI-wrapped key with an AEAD cipher) are v0.2; v0.1 gives the learning folder a user-only ACL and keeps it out of Windows Search.
- Why: The safety review showed a worker with web tools and file reads could exfiltrate private data with nothing in the way. Encryption at rest adds a crypto dependency and key handling for little gain over the profile's own permissions on a single-user PC.
- Consequences: A simulator invariant checks the taint rule. The privacy note says plainly what is and is not encrypted.
- Source: BUILD.md sections 2.10 and 2.12; [2026-10-observability.md](research/2026-10-observability.md) section 5.5.

## D41. The brain may act through deterministic tools; workers bind to warm sessions

- Date: 2026-10-20
- Status: proposed (architect, after the feasibility and owner-asks reviews)
- Amends: D19 (the driver starts sessions, and jobs are bound to them) and D20 (the brain is no longer limited to answering and dispatching)
- Decision: The brain gets an `act` toolset of tier-0 and tier-1 tools that never move the mouse or type (launch an app, move or snap a window, volume, open a URL, URI or file, media control, recent work, Resolve navigation), only in turns that began with the user's own addressed input. Clicks, typing, drags and every edit stay with workers. A worker driver starts a session first and binds a job to it later; one computer-use session and one deep session are kept warm when memory allows, and a memory admission rule in the core decides what may start. Personality travels in each turn, so renames and style changes need no restart, and the brain is recycled only on context size, a skill change or a crash.
- Why: Anything outside the exact grammar otherwise waited for a cold worker (34.5 s to a usable session measured under load, 39 to 48 s for a cold brain), which broke "intent, not commands".
- Consequences: The contracts carry `SessionSpec` and `JobBinding`; hooks and the shim identify a session, not a job; a brain-chosen action that the user does not undo becomes a phrasing candidate for the learning loop.
- Source: BUILD.md sections 2.3 and 2.5; [2026-10-learning-loop.md](research/2026-10-learning-loop.md) section 4; [2026-10-brain-latency.md](research/2026-10-brain-latency.md).

## D42. Hearing: the system mix as the echo reference, a conversation window, Raw Input

- Date: 2026-10-20
- Status: proposed (architect, after the owner-asks and safety reviews)
- Amends: D24 (the echo reference and the barge-in rule) and D26 (how the dictation key is read)
- Decision: Echo cancellation uses the WASAPI loopback capture of the default render endpoint as its far-end reference, so video and music the PC plays are cancelled as well as Eileen's own voice; it is bypassed when nothing plays. The addressee gate adds a conversation window that stays open while the user is engaged and is suspended while other audio plays or another app uses the microphone, plus a local score for unnamed requests about the screen when the user is alone, with the brain able to answer that an utterance was not for it. Barge-in pauses first and resumes after a backchannel. The dictation key and physical input are read through Raw Input, never a low-level keyboard hook in the core.
- Why: Without a system reference, a video playing beside Eileen triggered barge-in, opened the follow-up window and could even approve; without a conversation window, "no wake word" still needed the name in almost every sentence; a slow low-level hook is silently removed by Windows and makes every keystroke wait on Eileen.
- Consequences: The addressee acceptance has two numbers: room talk ignored at least 9 of 10 times, and unnamed requests in a conversation acted on at least 8 of 10 times with at most 1 false action in 10.
- Source: BUILD.md sections 2.2 and 2.8; [2026-10-voice-stack.md](research/2026-10-voice-stack.md) sections 3 and 12.

## D43. Approvals come only from the user, and the worker route is proven against the real CLI

- Date: 2026-10-20
- Status: proposed (architect, after the safety review)
- Amends: D14 (approval sources) and D30 (how a worker's tier-2 call reaches the user)
- Decision: An approval is built only from the user's own input: a physical click on the HUD (synthetic input is ignored), the tray, the approval hotkey, or speech inside the voice window. For tier 2 by voice the user says the action word ("yes, push") or the assistant's name; voice approval is off while other audio plays, while another app uses the microphone, and during fleet and nightly runs. Other media pause while the question is open. An unanswered request waits on the HUD until the job's deadline. Grants cover the exact call that was shown, and an approval is bound to its target. The summary is built by Eileen's code, never by a worker. How a worker's tier-2 call reaches Eileen (the `PermissionRequest` hook held open, `--permission-prompt-tool`, or deny then steer) is decided by WP3's contract test against the real CLI before the harness merges, and re-checked by the weekly and nightly contract tests.
- Why: The `ask` path through the permission-prompt tool was never tested and the flag no longer appears in `claude --help` on 2.1.304; the earlier rule let any "yes" the microphone heard approve, including a video Eileen had opened itself.
- Consequences: The dispatcher accepts an `approve` call only for a pending ask it issued; tools a worker could use to slip past the guard (subagents) stay off until the contract test covers them.
- Source: BUILD.md section 2.5; [2026-10-claude-mods.md](research/2026-10-claude-mods.md) sections 1.2 and 4.

## D44. The learning loop, revised

- Date: 2026-10-20
- Status: proposed (architect, after the learning-loop review)
- Amends: D27 and D28
- Decision: The loop's decisions (the ladder, the linter, computed provenance, shadow agreement, the replay state machine) live in a pure crate, `eileen-skills`, that the core calls through a `SkillEngine` port, so learning runs in the simulator; the IO lives in `eileen-learn`. Each worker session gets its own learned-skill snapshot written from `learn.sqlite` with a manifest, and every use of a skill is attributed. Recipes may refer to earlier steps' outputs, have call, delegate and confirm steps, check only read-only ops, and retry only idempotent ones. The ladder is risk-weighted (light skills reach model-free replay after three clean replays, standard ones after ten), and success needs an objective check or praise. Provenance is computed, never taken from the librarian; tainted steps and steps that needed approval never become recipe steps. The librarian is capped by share of the plan, sees only jobs that already ran on Claude, and can be run on demand. Confirmed phrasings run directly only when their target has no slots.
- Why: The review found that the recipe format could not express the worked examples, skill use was never attributed, the ladder's numbers contradicted the demos, and the safety rules had no mechanism that could be computed.
- Consequences: BUILD.md section 2.10 has a table of what each benchmark should look like run by run, and B8 measures it as an A/B.
- Source: BUILD.md section 2.10; [2026-10-learning-loop.md](research/2026-10-learning-loop.md) sections 3 and 6.

## D45. Replay and scenarios: two modes, formats as contracts, shipped crates

- Date: 2026-10-20
- Status: proposed (architect, after the engineering review)
- Amends: D31 and D35
- Decision: The recording, checkpoint, stub-script, regression, expected-trace and fleet formats are contracts. Strict replay (for incidents) compares every event on the commit that recorded it; scenario replay (for the community corpus) starts a fresh core and serves model and tool responses reactively, remapping IDs, so shared scenarios survive code changes. Replay, scenario capture and sharing, and the fleet runner are shipped crates (`eileen-replay`, `eileen-scenarios`, `eileen-fleet`); the simulator stays development-only. Sharing ships with the first slice. The scenario catalogue and its results are rendered to GitHub Pages and never committed, so pull request checks stay hermetic.
- Why: Open-loop replay broke on any change that minted one more ID, friends' pull requests could not regenerate a committed catalogue, and shipped commands lived in development-only crates.
- Consequences: Fleet PCs run scenarios with no model by default (recorded mode), with a live mode on the contributor's own plan as an option.
- Source: BUILD.md sections 3.3.16 and 4.8; [2026-10-observability.md](research/2026-10-observability.md) section 5.

## D46. The build guard and the gates, revised

- Date: 2026-10-20
- Status: proposed (architect, after the feasibility and engineering reviews)
- Amends: D32 and D33
- Decision: `cargo xtask` admits a build slot by measured free memory per compile job, samples before admitting a second, runs cargo in a kill-on-close Job Object at below-normal priority, and records its holder; agents cannot run cargo directly. Native libraries appear only under Windows target tables, so neutral crates build and test on Linux. Pull requests lead with an Evidence section whose tests must exist, and fix pull requests are checked failing before and passing after where Linux can show it. The house-style denylist is stored as hashes so the names never enter the repository. Each crate's guidance stays its crate doc, with an added heading for what needs a decision record, and is also generated into path-scoped Claude rules. A check registry generates the documented list; each check is marked as a repository check or delegated to a tool.
- Why: The first guard admitted builds on a single 2 GB reading on a PC whose free memory sits near 1 to 2 GB; the native dependencies would have broken the Linux lane, Windows test binaries and cargo-deny; the owner's evidence-first rule had no check.
- Consequences: Thirty checks, listed in BUILD.md section 4b.3 and generated into `CONTRIBUTING.md`.
- Source: BUILD.md sections 4.1, 4b and 6.1; [2026-10-engineering-practice.md](research/2026-10-engineering-practice.md) sections 4 and 6.

## D47. The HUD draws with DirectWrite and counts only physical clicks

- Date: 2026-10-20
- Status: proposed (architect, after the feasibility and safety reviews)
- Amends: D36
- Decision: The HUD draws text with DirectWrite into Direct2D on a DirectComposition surface (GDI text in a per-pixel-alpha layered window is invisible), with a colour-keyed GDI fallback if WP8's first measurement prefers it, and probes capture exclusion on exactly that window type. Its strip accepts a click on Allow, Deny, the mic or a first-run choice only when Raw Input shows it came from a device. It adds a live DWM thumbnail of the window a job works in, a confirm panel for crops, a contact sheet, a settings panel and a drop target for voice clips. The tray and hotkeys move into the core's shell crate, built in WP2, so the first slice has a mute key.
- Why: A worker could otherwise click Eileen's own Allow button; the owner asked for the mini-view, the crop confirmation and settings reachable on the HUD.
- Consequences: `HudInput` lines carry a `physical` flag.
- Source: BUILD.md section 2.9; [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) section 8.

## D48. Resolve edits: Fusion first, versions as undo

- Date: 2026-10-20
- Status: proposed (architect, after the owner-asks review)
- Amends: D23
- Decision: The benchmark's edit tries, in order, a generated Fusion composition imported on the clip (an ellipse mask keyed from face boxes Eileen tracks itself, feeding a sharpen node), a generated DRX with a window if the format allows placing one, and computer use on the Color page last. Before any edit Eileen adds a color version, so undo loads the previous one, and shows the face crop for confirmation. The bridge also loads projects and timelines, polls readiness after a launch, opens the Edit page before moving the playhead, and reports render progress, which the core tracks as an app task. WP6 runs the spike that picks the route in W1, before anything depends on it.
- Why: The DRX route was unknown and replaced the clip's grade with no undo, computer use on the Color page could never become a model-free recipe, and nothing loaded a project after launch.
- Consequences: An edit op without a recorded version is tier 2.
- Source: BUILD.md section 2.7; [2026-10-app-hooks.md](research/2026-10-app-hooks.md) section 2.

## D49. The first run has a surprise path

- Date: 2026-10-20
- Status: accepted (owner direction of 2026-10-20)
- Decision: The first run offers a tour, a skip, and "Show me something cool". With a truthful consent tick for the user's own voice, the surprise path asks for half a minute of free speech, uses it for setup and as a clone reference, runs a short live demo in the user's cloned voice, reveals it, and deletes everything at once if the user does not keep it. If the PC cannot clone fast enough, it runs the demo in the default voice or the tour, and offers the surprise later.
- Why: Owner direction: the moment that makes people understand what Eileen is.
- Consequences: A state machine in the core, the choices and checkbox on the HUD strip, the clone and its timing check in the speech process, and a scenario that asserts no file is left after a decline.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md sections 2.8 and E18.

## D50. A reusable, fully cited engineering playbook

- Date: 2026-10-20
- Status: accepted (owner direction of 2026-10-20)
- Decision: `docs/PLAYBOOK.md` documents how this repository is built in a project-agnostic way, with Eileen as the worked example: the modular layout, everything pulled in and why, the CI/CD design, the testing tactics, code maps, guidance for people and agents, observability and replay, the learning loop, how parts stay swappable, and the working methods. Every section cites the research reports, the decision records or primary sources with links and dates. `templates/` holds files another project can copy (guidance files, CI workflows, the xtask checks, pull request and issue templates, the scenario schema), generated from the live files where they mirror them.
- Why: Owner direction: other projects using this style of development should be able to adopt the discoveries.
- Consequences: Check 29 enforces citations and links; check 30 makes a change to this decision log update the playbook in the same pull request. WP0 writes the first version; later changes arrive through amendment pull requests, and WP9 adds the measured results.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md section 4b.6.
