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
- Status: accepted; amended by D18 (API keys), D19 (worker names) and D77 (proposed, pending the owner: narrowing the "no reading `~/.claude`" consequence to the user's own Markdown notes and instruction files after a yes)
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

## D15. An earlier working name

- Date: 2026-09-19
- Status: superseded by D16 (scrubbed of the earlier name by owner answer 15 of 2026-10-20)
- Decision: The product used an earlier working name; the shipping name was to be picked later from the shortlist in [NAMING.md](NAMING.md).
- Why: The earlier name collided with a live, funded startup in the same category, belonged to a commercial book and film franchise, and sounded like a place name when spoken after "hey". GitHub renames repositories with redirects, so the choice did not block work.
- Consequences:
  - The working name was used in code and docs until D16; the README pointed newcomers to NAMING.md.
  - An organisation handle with a suffix was expected, since every candidate handle was already taken.
  - Friends were to weigh in through a GitHub discussion or the naming issue.
- Source: [section 8](RESEARCH.md#8-name)

## D16. The product is named Eileen

- Date: 2026-10-11
- Status: accepted
- Supersedes: D15
- Decision: The product is named Eileen. The owner chose the name on 2026-10-11, and the choice is final.
- Why: D15 kept an earlier working name only as a placeholder, because it collided with a funded startup in the same category and belongs to a commercial franchise. The owner has now picked the shipping name. Renaming before the first commit and before the repo goes public costs nothing.
- Consequences:
  - The Python package, the console script and the `python -m` entry point are all `eileen`.
  - Environment variables use the `EILEEN_` prefix, and the data directory is `%LOCALAPPDATA%\eileen`.
  - MCP server names and notification namespaces use `eileen`, for example `eileen-desktop` and `notifications/eileen/desktop_event`.
  - The repository is `github.com/Sixteen1-6/eileen`.
  - Earlier entries in this log now say Eileen where they named the product. D15 was later scrubbed of the earlier name as well (owner answer 15 of 2026-10-20).
  - The shortlist in RESEARCH.md section 8 is closed; Eileen was not on it. That section stays as the record of why the earlier name was dropped, without naming it.
- Source: owner decision, 2026-10-11; [section 8](RESEARCH.md#8-name) for the collisions of the earlier name.

## D17. Rust core with supervised sidecars

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D55
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
- Status: accepted (owner answer 1 of 2026-10-20)
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
- Status: accepted (owner answer 1 of 2026-10-20); amended by D41
- Supersedes: D3, and D4's worker-naming consequence
- Decision: Every worker route implements one `WorkerDriver` trait. v0.1 builds the native Claude driver over `claude -p` stream-json. The Codex app-server driver and a generic ACP driver (for Grok and others) are designed slots behind the same trait, not built in v0.1. Workers are named by route only (Claude, Codex, Grok, DeepSeek) in every UI string, log line and document.
- Why: The Claude and Codex ACP adapters are Node wrappers around the same native protocols, add about 99 MB and a second per session, and hide features Eileen needs (the permission-prompt tool, partial messages, interrupts, rate limits). Anthropic's branding rules forbid using its product name as part of another product's or feature's name.
- Consequences:
  - The ACP crate's `process` feature is never used (it does not job-wrap on Windows); every vendor process is spawned by the supervisor.
  - `Route::display_name` is the only place a worker's user-facing name is defined; a tidy check enforces the naming rule.
- Source: [2026-10-subscription-harnesses.md](research/2026-10-subscription-harnesses.md) sections 1, 5 and 7; [2026-10-claude-mods.md](research/2026-10-claude-mods.md) section 5.

## D20. The brain is a warm Claude session with a hot spare, behind a deterministic fast path

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D41 and D53
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
- Status: accepted (owner answer of 2026-10-20); amended by D59
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
- Status: accepted (owner answer 1 of 2026-10-20); amended by D59
- Amends: D7
- Decision: Known videos and pages open with `ShellExecuteW` in the user's own browser, and playback is controlled through Windows' system media controls from Rust. Page content goes through screenshots and foreground input in v0.1, or through the Playwright MCP extension when the user installs it. Claude in Chrome is used only inside a Claude worker, by user opt-in. YouTube is never searched headlessly.
- Why: Measured on the owner's PC: a `&t=930s` URL landed at 15:30 and SMTC pause and seek from Rust took effect, with no extension. YouTube's robots.txt disallows headless search, and yt-dlp took seconds idle and minutes under load.
- Consequences: Eileen never relaunches the user's browser profile with a debug port; after a load or seek it waits for the media session to return with the expected title (ads).
- Source: [2026-10-app-hooks.md](research/2026-10-app-hooks.md) sections 0 and 5.

## D23. App hooks are resident Rust; Resolve through a Lua bridge

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D48
- Amends: D8 (its ladder stands; its list of adapters changes)
- Decision: One resident Rust process, `eileen-apps`, holds every app adapter: SMTC with one long-lived manager, Office and Explorer over late-bound COM on per-target STA threads, the recent-work index, and DaVinci Resolve through a small Lua bridge run by Resolve's own `fuscript.exe`, supervised and gated on Resolve running. Absolute timecodes are computed in Rust. Adobe apps get Eileen-owned UXP plugins in v0.2. On Resolve 21.1 or later, Resolve's own MCP server is offered to workers and measured against the bridge.
- Why: Warm state beats fast calls: under load, spawns and fresh system-object requests degraded by orders of magnitude while resident handles stayed at about a microsecond. Resolve needs no Python of Eileen's own.
- Consequences: Python is not in the v0.1 build (D38).
- Source: [2026-10-app-hooks.md](research/2026-10-app-hooks.md) sections 1, 2, 6 and 8.

## D24. A local voice pipeline in Rust, CPU only

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D42, D53 and D58
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
- Status: accepted (owner answer 1 of 2026-10-20); amended by D42
- Decision: Dictation uses the already-loaded streaming recognizer, Rust cleanup rules (with SpeakoFlow's conformance cases), an optional Claude scribe for polish, and an insertion ladder in the desktop server (`EM_REPLACESEL`, then a port of Handy's clipboard transaction, then paced keys), behind the same input lock as computer use, with vocabulary learned from corrections.
- Why: Owner direction for dictation better than the paid tools; Eileen already knows the focused field, its app and what is on screen, which the open-source leaders do not.
- Consequences: Handy's and SpeakoFlow's code is ported with attribution in NOTICE; GPL and AGPL dictation projects are design references only.
- Source: [2026-10-dictation-and-voices.md](research/2026-10-dictation-and-voices.md) sections 1, 5 to 9.

## D27. The learning loop

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D44 and D77
- Decision: Record a redacted trajectory for every job at the dispatcher and from the worker stream; a low-priority Claude librarian distills successful jobs into native `SKILL.md` skills with optional recipes of Eileen tool calls with checks, writing only through a validating tool; workers load them with `--plugin-dir`; skills climb a promotion ladder on evidence (draft, trusted, compiled recipe with one user approval, spoken fast path) and fall on failure; replay runs through the same dispatcher and permission gate as a worker and hands off on the first failed check. Version history lives in `learn.sqlite`, not in a git repository per skill. The crate is `eileen-learn`; the matcher lives in `eileen-intent`.
- Why: Owner direction: this is the main point of the product. The native route was verified on the owner's PC. SQLite history avoids `gix`'s compile cost on this PC and any dependency on a git CLI, and makes "forget" one transaction.
- Consequences: A learned skill never raises a tier or skips a confirmation; screen text enters a skill only with the user's approval; auto memory and session persistence are off in every session Eileen starts.
- Source: [2026-10-learning-loop.md](research/2026-10-learning-loop.md) sections 1, 3 and 6; BUILD.md section 2.10.

## D28. A layered intent resolver

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D44 and D52
- Decision: Intent resolution is its own crate, `eileen-intent`, with four layers: a deterministic grammar plus learned templates (L1, under 1 ms), a local semantic matcher with a small on-device embedding model run in the speech process (L2, hints and confirmed paraphrases only), the brain with live context (L3), and a worker that figures out new capabilities which the learning loop turns into skills (L4). Unclear requests get one clarifying question, never an error. "What can you do here?" is answered by introspection of tools, app capabilities and skills.
- Why: Owner direction "intent, not commands"; the measured matcher is microseconds per utterance; lexical hints alone matched the wrong verb, so nothing below L3 acts on an unconfirmed similarity.
- Consequences: The embedding model is adopted only if it meets its latency budget on the owner's PC; otherwise L2 runs lexical only and says so.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [2026-10-learning-loop.md](research/2026-10-learning-loop.md) section 3.4; BUILD.md sections 2.2 and 3.3.10.

## D29. Settings by voice, in versioned TOML

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20)
- Supersedes: D13's YAML policy file and ARCHITECTURE.md's YAML files
- Decision: Configuration is TOML with comments in `%LOCALAPPDATA%\eileen\config`, generated from documented types. Every setting is reachable by voice and on the HUD through one flow (propose, check when a choice could misfire, confirm in one sentence, apply, version), with "undo that", profiles and hand edits picked up by a watcher. Safety bounds: tier 3 can never be relaxed; skipping a tier-2 confirmation needs an explicit, logged decision and never covers credentials, payments, messages sent as the user or deletes outside a repository.
- Why: Owner direction "customise anything by just saying it". The Rust YAML options are deprecated or unsound; TOML parsed in 0.4 to 5.1 ms here.
- Consequences: Renaming the assistant updates the addressee gate, the recognizer hints and the brain's self-reference; the product and repository stay Eileen.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) section 8; BUILD.md section 2.11.

## D30. Claude integration only through official extension points

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D43
- Decision: Workers get per-job `--settings` files whose HTTP hooks send `PreToolUse` to Eileen's tier guard (fail closed, never answering allow) and report notifications, failures and stops to the loopback bus. The owner may install one opt-in plugin named `eileen` (a handoff skill, Eileen's session tools, a display-only mod) through Claude's own plugin flow. Mods are never in Eileen's critical path. Nothing patches, wraps or replaces the Claude app or turns off its updater.
- Why: Owner direction to use Claude's official mods, plugins, skills and hooks while keeping updates. Hooks are documented and were verified to fire and to block on the owner's PC; the mod API is early access and changes between releases.
- Consequences: CI validates the plugin and fails if it gains a gating hook; learned skills are not loaded into the owner's own sessions in v0.1.
- Source: [2026-10-claude-mods.md](research/2026-10-claude-mods.md) sections 1 to 6.

## D31. A sans-IO core tested by deterministic simulation

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D45
- Decision: `eileen-core` is a state machine with one entry point, `Core::step(now, rng, input, effects)`, over closed `Input` and `Effect` enums, with time, randomness and IDs injected and every queue bounded. `eileen-sim` drives it on one thread with simulated speech, playback, brain, workers and sidecars, injects faults at `buggify!` points, checks named invariants after every step and replays any failure from a seed or a decoded input script. Pull requests run a fixed seed set; `main` and nightly run fresh seeds. End-to-end tests inject recorded or synthesized audio at the audio boundary and use a stub harness; the owner's PC is never a self-hosted runner for this public repository.
- Why: The best-tested systems (TigerBeetle, FoundationDB, Turso) make concurrency bugs reproducible this way; the owner asked for simulation of jobs, bus, scheduling and barge-in from a seed.
- Consequences: Core logic never reads the clock or OS randomness (clippy bans them); the event log format is shared by the flight recorder, the simulator, the end-to-end harness and the learning record.
- Source: [2026-10-engineering-practice.md](research/2026-10-engineering-practice.md) sections 1 to 3.

## D32. Conventions are enforced by CI with one local command

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D46
- Decision: Every way of working adopted from the exemplar repositories has an automated check in `cargo xtask lint-repo` (crate guidance, the code map matching the crates, Markdown lint, links, spelling, house style and naming, commit and pull request format, generated files current, unsafe only in `ffi` modules with safety comments, documented public items, decision record format, dependency policy, the credential boundary, the platform boundary, scenario privacy). `cargo xtask ci` runs locally exactly what the pull request lane runs. One aggregate check, `all-green`, gates merges.
- Why: Owner direction: conventions enforced by CI, not just written down. Guidance files rot; the checks are the guardrail.
- Consequences: The list of checks lives in BUILD.md section 4b.3, CONTRIBUTING.md and the root AGENTS.md; each check has a test that fails on a seeded violation.
- Source: [2026-10-engineering-practice.md](research/2026-10-engineering-practice.md) sections 4.9 and 6; BUILD.md section 4b.

## D33. Guidance form: one AGENTS.md, crate docs in the code, nested files only where rules differ

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D46
- Decision: The root `AGENTS.md` is the canonical guide for people and every coding tool; the root `CLAUDE.md` imports it with `@AGENTS.md` and adds a short Claude-only section. Each crate's guidance (what it owns and does not own, invariants, boundaries, how to test, traps, logs) is its crate doc at the top of `src/lib.rs` or `src/main.rs`, with required headings checked by CI. A nested `AGENTS.md` (paired with a one-line importing `CLAUDE.md`) exists only where a folder's rules genuinely differ. A short root `ARCHITECTURE.md` is the code map.
- Why: The engineering-practice survey found that no best-run repository keeps a guidance file in every crate; per-directory files appear only where a subsystem has its own contract; Codex reads only `AGENTS.md` and has no import syntax; symlinks break on Windows checkouts. The owner asked not to assume a CLAUDE.md in every folder.
- Consequences: The nested files at the start are listed in BUILD.md section 4b.4; adding one needs a stated reason. `docs/ARCHITECTURE.md` moves to the repository root in WP0.
- Source: [2026-10-engineering-practice.md](research/2026-10-engineering-practice.md) sections 2.3, 4 and 5.

## D34. Observability: request IDs, a flight recorder and replay

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20)
- Decision: Typed, prefixed ULIDs (`turn_` is the request ID) and a `TraceCtx` carried on every bus envelope, frame, tool call and worker environment; span names as constants with a checked parent table; user content only in `pii.` fields; JSON log files, an in-memory flight recorder with a replayable input lane, incidents on panics, crashes, watchdog trips and spoken reports; a guard process outside the Job Object that writes minidumps of crashed or hung processes; L1 replay of recordings in the simulator; a generated code map with an event map; a read-only `dev` toolset for Eileen's own coding workers.
- Why: Owner direction: one request ID traced end to end, a flight recorder, session replay and a code map, so a bug goes from symptom to code quickly for people and agents.
- Consequences: Bare `tokio::spawn` is banned in favour of a wrapper that keeps spans; the audio callback never calls `tracing`; a redaction canary test covers every sink.
- Source: [2026-10-observability.md](research/2026-10-observability.md) sections 1 to 8.

## D35. Real use becomes tests: the community scenario corpus and the test fleet

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D45 and D58
- Decision: Scenarios are data in a versioned schema (`scenarios/<area>/<id>/scenario.toml`). "Save that as a test" turns a real request, success or failure, into a scenario from the flight recorder; sharing generalises and redacts it with a preview the contributor approves, then submits it from the contributor's own GitHub account. CI validates every scenario and replays it in simulation on every pull request. Contributors may opt in to `eileen test --fleet`, which runs real-desktop scenarios on their PCs and uploads anonymised results; failures are clustered, flakes tracked, and a readable catalogue with a coverage view is generated.
- Why: Owner direction: the owner's friends test on their own PCs, and everything they try should feed the test suite so it grows from real use.
- Consequences: The schema, its checks and the catalogue generator land in WP0; the replay runner in WP1; capture in WP2; sharing and the fleet in WP9. Every package ships scenarios. The owner's voice and screen never enter the public repository.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md section 4.8.

## D36. The HUD is part of v0.1

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D47
- Decision: A raw Win32 overlay (layered, click-through, topmost, never activated, excluded from screen capture) plus a small clickable strip, in its own process, fed with full `HudState` snapshots at most ten times a second. It shows the mic state, what is heard, what Eileen says, jobs, approvals, the input lock, the target of the next click, suggestions and indicators.
- Why: The owner asked for mute one click away and a visual layer as part of the feel; a status surface is table stakes. The raw Win32 option adds no dependencies; the egui and WebView2 alternatives stay unmeasured in the lab.
- Consequences: The HUD's layout is platform-neutral and snapshot-tested; only the renderer is Windows code.
- Source: [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) section 8; BUILD.md section 2.9.

## D37. The PC first, then outward through connectors

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20)
- Amends: D11 (the phone channels move from v0.1 to v0.2)
- Decision: v0.1 connects fully to the PC. Everything outside it (the phone, Home Assistant and Matter, other devices on the user's network, earbuds as microphones) is a connector with a manifest of tools, tiers and events, sharing the permission tiers, job model and learning loop. Room-sensing connectors are opt-in per device and processed locally. Telegram and ntfy become the first outward connector in v0.2.
- Why: Owner direction of 2026-10-20.
- Consequences: The connector manifest is a contract from the first commit; D12 (OpenClaw only as an optional gateway) is unchanged.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md section 2.14.

## D38. Windows first, not Windows only; the Python scaffold is retired

- Date: 2026-10-20
- Status: accepted (owner answer 1 of 2026-10-20); amended by D55
- Supersedes: ROADMAP.md's 2026-09 non-goal "macOS and Linux", and the remaining Python consequences of D9
- Decision: Every OS-touching piece sits behind traits in `eileen-platform` and is implemented in a per-area Windows crate (`eileen-audio-win`, `eileen-desktop-win`, `eileen-apps-win`, `eileen-shell-win`); a Linux port adds `-linux` crates. Platform-neutral crates build and pass their tests on `ubuntu-latest` in CI from the first commit. The Python scaffold under `src/eileen` is retired; nothing in the v0.1 build is Python, and any later Python adapter is a stdlib-only JSON-RPC shim.
- Why: Owner direction of 2026-10-20. The Python MCP SDK's import alone took 35 to 42 s on the loaded PC, against 1.7 to 2.9 s for a stdlib-only shim.
- Consequences: The last commit with the scaffold is tagged `python-scaffold-final` before WP0 deletes it.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) section 7; BUILD.md sections 2.14 and 3.2.

## D39. Voices: the engine is chosen per voice, and Chatterbox is in v0.1

- Date: 2026-10-20
- Status: accepted (owner direction of 2026-10-20); amended by D58
- Amends: D25 (its consequence that Chatterbox waits for v0.2)
- Decision: Kokoro stays the default voice and Kyutai Pocket TTS the default cloner. Chatterbox (Turbo from Resemble's official ONNX graphs, Nano from a community ONNX export once its license checks out) sits behind the same `Synth` trait in v0.1, run through `ort` with no PyTorch and no Python. Each voice profile stores its engine. A head-to-head on the owner's PC (Pocket TTS against Chatterbox-Nano and Chatterbox-Turbo, cloning his own voice: time to first audio, real-time factor, memory, a listening test) is part of WP4's acceptance. An engine speaks live when its measured real-time factor allows it; otherwise it keeps announcements, summaries and read-alouds, rendering a whole line before playing it when it is slower than real time. No watermark by default on any engine; Perth stays off.
- Why: The owner's entry of 2026-10-20 makes Chatterbox selectable per voice and live when fast enough, and asks for the head-to-head; D25 was written from the earlier entry.
- Consequences: The first run's surprise path (D49) uses whichever cloner measured fast enough; `voice.toml` carries the engine and the consent record.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); [2026-10-dictation-and-voices.md](research/2026-10-dictation-and-voices.md) section 10; BUILD.md section 2.8.

## D40. What contains a worker in v0.1, and what waits for v0.2

- Date: 2026-10-20
- Status: accepted (owner answer of 2026-10-20); amended by D62, D71 and D77 (the `Notes` class; and, proposed, pending the owner: whether a session kind may drop the `--restricted` fence to keep native notes)
- Amends: D14 (its "sandbox by default" and "encrypt at rest" consequences)
- Decision: In v0.1 a Claude worker is contained by: its own repository or worktree; a path guard that tiers every read and write by canonicalised path (Eileen's data folder, credential stores and browser profiles are tier 3; anything outside the job's folder and toolchains is tier 2); a shell policy that parses commands and allows only a list of build, test and local git commands without asking; per-job taint that makes network egress tier 2 once a job has both seen untrusted content and read private data; the `PreToolUse` guard failing closed; per-app consent for desktop input; and its Job Object. The CLI's own `--restricted` mode (on 2.1.304 it drops code-running tools and WebFetch unless named, confines file tools to the working folders and ignores user and project settings) becomes a second fence for every session if WP3's test shows workers still work under it; other sandbox and Windows container options are measured in WP3 and recorded. A real sandbox and encryption at rest (a DPAPI-wrapped key with an AEAD cipher) are v0.2; v0.1 gives the learning folder a user-only ACL and keeps it out of Windows Search.
- Why: The safety review showed a worker with web tools and file reads could exfiltrate private data with nothing in the way. Encryption at rest adds a crypto dependency and key handling for little gain over the profile's own permissions on a single-user PC.
- Consequences: A simulator invariant checks the taint rule. The privacy note says plainly what is and is not encrypted.
- Source: BUILD.md sections 2.10 and 2.12; [2026-10-observability.md](research/2026-10-observability.md) section 5.5.

## D41. The brain may act through deterministic tools; workers bind to warm sessions

- Date: 2026-10-20
- Status: accepted (owner answer of 2026-10-20); amended by D52, D54 and D65
- Amends: D19 (the driver starts sessions, and jobs are bound to them) and D20 (the brain is no longer limited to answering and dispatching)
- Decision: The brain gets an `act` toolset of tier-0 and tier-1 tools that never move the mouse or type (launch an app, move or snap a window, volume, open a URL, URI or file, media control, recent work, Resolve navigation), only in turns that began with the user's own addressed input. Clicks, typing, drags and every edit stay with workers. A worker driver starts a session first and binds a job to it later; one computer-use session and one deep session are kept warm when memory allows, and a memory admission rule in the core decides what may start. Personality travels in each turn, so renames and style changes need no restart, and the brain is recycled only on context size, a skill change or a crash.
- Why: Anything outside the exact grammar otherwise waited for a cold worker (34.5 s to a usable session measured under load, 39 to 48 s for a cold brain), which broke "intent, not commands".
- Consequences: The contracts carry `SessionSpec` and `JobBinding`; hooks and the shim identify a session, not a job; a brain-chosen action that the user does not undo becomes a phrasing candidate for the learning loop.
- Source: BUILD.md sections 2.3 and 2.5; [2026-10-learning-loop.md](research/2026-10-learning-loop.md) section 4; [2026-10-brain-latency.md](research/2026-10-brain-latency.md).

## D42. Hearing: the system mix as the echo reference, a conversation window, Raw Input

- Date: 2026-10-20
- Status: accepted (owner answer of 2026-10-20); amended by D51
- Amends: D24 (the echo reference and the barge-in rule) and D26 (how the dictation key is read)
- Decision: Echo cancellation uses the WASAPI loopback capture of the default render endpoint as its far-end reference, so video and music the PC plays are cancelled as well as Eileen's own voice; it is bypassed when nothing plays. The addressee gate adds a conversation window that stays open while the user is engaged and is suspended while other audio plays or another app uses the microphone, plus a local score for unnamed requests about the screen when the user is alone, with the brain able to answer that an utterance was not for it. Barge-in pauses first and resumes after a backchannel. The dictation key and physical input are read through Raw Input, never a low-level keyboard hook in the core.
- Why: Without a system reference, a video playing beside Eileen triggered barge-in, opened the follow-up window and could even approve; without a conversation window, "no wake word" still needed the name in almost every sentence; a slow low-level hook is silently removed by Windows and makes every keystroke wait on Eileen.
- Consequences: The addressee acceptance has two numbers: room talk ignored at least 9 of 10 times, and unnamed requests in a conversation acted on at least 8 of 10 times with at most 1 false action in 10.
- Source: BUILD.md sections 2.2 and 2.8; [2026-10-voice-stack.md](research/2026-10-voice-stack.md) sections 3 and 12.

## D43. Approvals come only from the user, and the worker route is proven against the real CLI

- Date: 2026-10-20
- Status: accepted (owner answer of 2026-10-20); amended by D51
- Amends: D14 (approval sources) and D30 (how a worker's tier-2 call reaches the user)
- Decision: An approval is built only from the user's own input: a physical click on the HUD (synthetic input is ignored), the tray, the approval hotkey, or speech inside the voice window. For tier 2 by voice the user says the action word ("yes, push") or the assistant's name; voice approval is off while other audio plays, while another app uses the microphone, and during fleet and nightly runs. Other media pause while the question is open. An unanswered request waits on the HUD until the job's deadline. Grants cover the exact call that was shown, and an approval is bound to its target. The summary is built by Eileen's code, never by a worker. How a worker's tier-2 call reaches Eileen (the `PermissionRequest` hook held open, `--permission-prompt-tool`, or deny then steer) is decided by WP3's contract test against the real CLI before the harness merges, and re-checked by the weekly and nightly contract tests.
- Why: The `ask` path through the permission-prompt tool was never tested and the flag no longer appears in `claude --help` on 2.1.304; the earlier rule let any "yes" the microphone heard approve, including a video Eileen had opened itself.
- Consequences: The dispatcher accepts an `approve` call only for a pending ask it issued; tools a worker could use to slip past the guard (subagents) stay off until the contract test covers them.
- Source: BUILD.md section 2.5; [2026-10-claude-mods.md](research/2026-10-claude-mods.md) sections 1.2 and 4.

## D44. The learning loop, revised

- Date: 2026-10-20
- Status: accepted (owner answer of 2026-10-20); amended by D52 and D77
- Amends: D27 and D28
- Decision: The loop's decisions (the ladder, the linter, computed provenance, shadow agreement, the replay state machine) live in a pure crate, `eileen-skills`, that the core calls through a `SkillEngine` port, so learning runs in the simulator; the IO lives in `eileen-learn`. Each worker session gets its own learned-skill snapshot written from `learn.sqlite` with a manifest, and every use of a skill is attributed. Recipes may refer to earlier steps' outputs, have call, delegate and confirm steps, check only read-only ops, and retry only idempotent ones. The ladder is risk-weighted (light skills reach model-free replay after three clean replays, standard ones after ten), and success needs an objective check or praise. Provenance is computed, never taken from the librarian; tainted steps and steps that needed approval never become recipe steps. The librarian is capped by share of the plan, sees only jobs that already ran on Claude, and can be run on demand. Confirmed phrasings run directly only when their target has no slots.
- Why: The review found that the recipe format could not express the worked examples, skill use was never attributed, the ladder's numbers contradicted the demos, and the safety rules had no mechanism that could be computed.
- Consequences: BUILD.md section 2.10 has a table of what each benchmark should look like run by run, and B8 measures it as an A/B.
- Source: BUILD.md section 2.10; [2026-10-learning-loop.md](research/2026-10-learning-loop.md) sections 3 and 6.

## D45. Replay and scenarios: two modes, formats as contracts, shipped crates

- Date: 2026-10-20
- Status: accepted (owner answer of 2026-10-20); amended by D57
- Amends: D31 and D35
- Decision: The recording, checkpoint, stub-script, regression, expected-trace and fleet formats are contracts. Strict replay (for incidents) compares every event on the commit that recorded it; scenario replay (for the community corpus) starts a fresh core and serves model and tool responses reactively, remapping IDs, so shared scenarios survive code changes. Replay, scenario capture and sharing, and the fleet runner are shipped crates (`eileen-replay`, `eileen-scenarios`, `eileen-fleet`); the simulator stays development-only. Sharing ships with the first slice. The scenario catalogue and its results are rendered to GitHub Pages and never committed, so pull request checks stay hermetic.
- Why: Open-loop replay broke on any change that minted one more ID, friends' pull requests could not regenerate a committed catalogue, and shipped commands lived in development-only crates.
- Consequences: Fleet PCs run scenarios with no model by default (recorded mode), with a live mode on the contributor's own plan as an option.
- Source: BUILD.md sections 3.3.16 and 4.8; [2026-10-observability.md](research/2026-10-observability.md) section 5.

## D46. The build guard and the gates, revised

- Date: 2026-10-20
- Status: accepted (owner answer of 2026-10-20); amended by D54, D55 and D57
- Amends: D32 and D33
- Decision: `cargo xtask` admits a build slot by measured free memory per compile job, samples before admitting a second, runs cargo in a kill-on-close Job Object at below-normal priority, and records its holder; agents cannot run cargo directly. Native libraries appear only under Windows target tables, so neutral crates build and test on Linux. Pull requests lead with an Evidence section whose tests must exist, and fix pull requests are checked failing before and passing after where Linux can show it. The house-style denylist is stored as hashes so the names never enter the repository. Each crate's guidance stays its crate doc, with an added heading for what needs a decision record, and is also generated into path-scoped Claude rules. A check registry generates the documented list; each check is marked as a repository check or delegated to a tool.
- Why: The first guard admitted builds on a single 2 GB reading on a PC whose free memory sits near 1 to 2 GB; the native dependencies would have broken the Linux lane, Windows test binaries and cargo-deny; the owner's evidence-first rule had no check.
- Consequences: Thirty checks, listed in BUILD.md section 4b.3 and generated into `CONTRIBUTING.md`.
- Source: BUILD.md sections 4.1, 4b and 6.1; [2026-10-engineering-practice.md](research/2026-10-engineering-practice.md) sections 4 and 6.

## D47. The HUD draws with DirectWrite and counts only physical clicks

- Date: 2026-10-20
- Status: accepted (owner answer of 2026-10-20); amended by D51
- Amends: D36
- Decision: The HUD draws text with DirectWrite into Direct2D on a DirectComposition surface (GDI text in a per-pixel-alpha layered window is invisible), with a colour-keyed GDI fallback if WP8's first measurement prefers it, and probes capture exclusion on exactly that window type. Its strip accepts a click on Allow, Deny, the mic or a first-run choice only when Raw Input shows it came from a device. It adds a live DWM thumbnail of the window a job works in, a confirm panel for crops, a contact sheet, a settings panel and a drop target for voice clips. The tray and hotkeys move into the core's shell crate, built in WP2, so the first slice has a mute key.
- Why: A worker could otherwise click Eileen's own Allow button; the owner asked for the mini-view, the crop confirmation and settings reachable on the HUD.
- Consequences: `HudInput` lines carry a `physical` flag.
- Source: BUILD.md section 2.9; [2026-10-rust-architecture.md](research/2026-10-rust-architecture.md) section 8.

## D48. Resolve edits: Fusion first, versions as undo

- Date: 2026-10-20
- Status: accepted (owner answer of 2026-10-20); amended by D59
- Amends: D23
- Decision: The benchmark's edit tries, in order, a generated Fusion composition imported on the clip (an ellipse mask keyed from face boxes Eileen tracks itself, feeding a sharpen node), a generated DRX with a window if the format allows placing one, and computer use on the Color page last. Before any edit Eileen adds a color version, so undo loads the previous one, and shows the face crop for confirmation. The bridge also loads projects and timelines, polls readiness after a launch, opens the Edit page before moving the playhead, and reports render progress, which the core tracks as an app task. WP6 runs the spike that picks the route in W1, before anything depends on it.
- Why: The DRX route was unknown and replaced the clip's grade with no undo, computer use on the Color page could never become a model-free recipe, and nothing loaded a project after launch.
- Consequences: An edit op without a recorded version is tier 2.
- Source: BUILD.md section 2.7; [2026-10-app-hooks.md](research/2026-10-app-hooks.md) section 2.

## D49. The first run has a surprise path

- Date: 2026-10-20
- Status: accepted (owner direction of 2026-10-20); amended by D56
- Decision: The first run offers a tour, a skip, and "Show me something cool". With a truthful consent tick for the user's own voice, the surprise path asks for half a minute of free speech, uses it for setup and as a clone reference, runs a short live demo in the user's cloned voice, reveals it, and deletes everything at once if the user does not keep it. If the PC cannot clone fast enough, it runs the demo in the default voice or the tour, and offers the surprise later.
- Why: Owner direction: the moment that makes people understand what Eileen is.
- Consequences: A state machine in the core, the choices and checkbox on the HUD strip, the clone and its timing check in the speech process, and a scenario that asserts no file is left after a decline.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md sections 2.8 and E18.

## D50. A reusable, fully cited engineering playbook

- Date: 2026-10-20
- Status: accepted (owner direction of 2026-10-20); amended by D75
- Decision: `docs/PLAYBOOK.md` documents how this repository is built in a project-agnostic way, with Eileen as the worked example: the modular layout, everything pulled in and why, the CI/CD design, the testing tactics, code maps, guidance for people and agents, observability and replay, the learning loop, how parts stay swappable, and the working methods. Every section cites the research reports, the decision records or primary sources with links and dates. `templates/` holds files another project can copy (guidance files, CI workflows, the xtask checks, pull request and issue templates, the scenario schema), generated from the live files where they mirror them.
- Why: Owner direction: other projects using this style of development should be able to adopt the discoveries.
- Consequences: Check 29 enforces citations and links; check 30 makes a change to this decision log update the playbook in the same pull request. WP0 writes the first version; later changes arrive through amendment pull requests, and WP9 adds the measured results.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md section 4b.6.

## D51. Approvals and the conversation window, revised

- Date: 2026-10-20
- Status: accepted (architect, after the first verification round; the owner's standing rule applies); amended by D62 and D76
- Amends: D42 (who passes inside the conversation window, and what is never suspended), D43 (the forms of a tier-2 voice approval and its order), D47 (the tray is not an approval source)
- Decision: A tier-2 approval by voice needs the action word, and also a speaker match once the speaker check is adopted and the user enrolled; a bare "yes" or "Eileen, yes" answers only clarifying questions, confirm steps and tier-1 asks. The approve, deny and unmute hotkeys count only when Raw Input saw the chord come from a keyboard, and the desktop server refuses to press any of Eileen's hotkeys. The tray offers no approval. When an approval is asked, the core pauses media first, waits for the pause to be confirmed, then decides the voice mode and speaks; the voice window opens after the question has drained plus an echo tail. Inside the conversation window only commands and replies pass without the local addressee score; media-control and stop words are never suspended while other audio plays. Every resolution records its `ApprovalSource`.
- Why: `RegisterHotKey` fires on injected keystrokes, so a worker could press "approve"; "Eileen, yes" from any voice was weaker than the action word the other documents promised; a question could be heard as its own answer; the window answered side talk such as "want coffee?"; and the suspension blocked "pause" while a video played.
- Consequences: Simulator invariant 7 covers the order and the sources; WP1 acceptance 5 tests each case; ROADMAP and ARCHITECTURE no longer say "only the user's own voice" unless the speaker check is enrolled.
- Source: BUILD.md sections 2.5, 2.8 and 2.9; the first verification round (items ab65bc-3 and aedf46-2).

## D52. The learning loop, second revision

- Date: 2026-10-20
- Status: accepted (architect, after the first verification round; the owner's standing rule applies); amended by D61
- Amends: D44 (recipes, attribution, provenance, facts), D41 (how brain actions become phrasings), D28 (L2 always has an on-device model)
- Decision: Every op a recipe may use declares an output schema, and recipes point into outputs with typed operands; branching is one fallback construct with an `ask` step for questions; eligibility allows only vocabulary literals in non-read steps; a replay may call only the act, desktop and apps toolsets and `jobs_dispatch`; a level-3 match reaches the brain as a `[match]` block. Slot values of every skill use have a defined source order. Snapshots are verified and recycled when their skills change; the claim is detection, not prevention. Provenance defines "literal", traces to untainted worker steps too, and one set of observed sources drives both taint and recipe compilation; drafts from jobs that read free text from outside are held whole for review. Facts have a deterministic path from the user's own words, `FactDelta`, and `facts_used` on skills. Fast-path commands and brain act turns are `Act` jobs, and a phrasing serves exact repeats at once and paraphrases after confirmation. L2 uses a transformer model if it meets its budget and a static-embedding model otherwise.
- Why: The verification round found that pointers had nothing to point into, the worked recipes broke their own eligibility rule and needed branching the contract lacked, worker skill uses had no slot values, observed text was never captured for the main injection path, facts had only a name for their path, and the first-run repeat could not be served by the fast path.
- Consequences: BUILD.md section 2.10's three worked recipes lint clean; WP1 acceptance 12 and 13 and WP7 acceptance 5 to 10 test the rules.
- Source: BUILD.md sections 2.3, 2.10, 3.3.10 and 3.3.13; the first verification round (items a6a62e-1, -2, -3, -5, -6, -8 and aedf46-1).

## D53. Model names are aliases in config; the brain and the speech stages are plug-ins

- Date: 2026-10-20
- Status: accepted (owner direction of 2026-10-20, "stay at the frontier" and "the brain is swappable"); amended by D62 and D74
- Amends: D20 (model ids were full ids written into config), D24 (speech stages were wired directly)
- Decision: Config names models through `routing.models` aliases; the harness resolves each to a full id per CLI version, passes only full ids on command lines and records the resolved id beside every timing. `routing.brain`, `routing.deep` and `routing.fallback_brain` are typed config; `BrainBackend` has `route()` and a `brain_for` factory; benchmark B10 (`eileen bench brain`) runs before a backend switch and warns when the candidate is slower or less reliable; a stalled brain falls back to the fallback slot. Voice activity, recognition and turn detection sit behind traits like the voices, each reporting its languages and cost, with the engine per stage a setting.
- Why: The owner asked for aliases in config and for every component AI progress improves to be swappable; D20's reason (aliases drifted between CLI builds) is handled by re-resolving per CLI version and recording the resolved id.
- Consequences: Simulator invariant 15; WP3 acceptance 6; the later language support discovers languages from the components.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md sections 2.3, 2.8 and 3.3.8.

## D54. Memory admission and the build guard read available memory, commit headroom and paging

- Date: 2026-10-20
- Status: accepted (architect, after the first verification round; the owner's standing rule applies); amended by D64, D69 and D76
- Amends: D41 (admission and the warm pool), D46 (the build guard's numbers)
- Decision: Admission reads available memory (free plus standby), commit headroom and the paging rate. A job starts if commit headroom stays above 1 GB and either available memory stays above the reserve (10 percent of RAM, 768 MB to 2 GB), or paging has been low for 30 s, or the user says "start it anyway"; a queued job names the biggest process and asks again after ten minutes. The warm pool starts only by the reserve rule. A per-job memory limit is a Job Object limit that fails the job by name. The build guard admits its first slot by headroom and paging with two compile jobs under 2 GB available, its second only with real room, and waits then exits with code 75 when refused. Eileen's own coding workers run no language server in v0.1.
- Why: With 1 to 2 GB usually available on the owner's PC, the old rules would have admitted no worker and no build at all, contradicting the M6 demo and the owner's answer that one build slot runs on normal days.
- Consequences: WP9 records which rule admitted each M6 job and retunes the defaults by amendment; the documents say plainly that the warm pool and the hot spare are usually absent on this PC.
- Source: BUILD.md sections 2.5, 4.4 and 6.1; owner answer 8 of 2026-10-20.

## D55. The crate graph and the lint bans agree; secrets, volume and app hooks are platform traits

- Date: 2026-10-20
- Status: accepted (architect, after the first verification round; the owner's standing rule applies)
- Amends: D17 (only the supervisor spawns processes), D38 (what sits behind platform traits)
- Decision: Named threads and sleep live in `eileen-stdx`, so every crate, the `-win` crates included, obeys the thread and sleep bans; the `-win` crates may use `eileen-trace`; `eileen-runtime` may use `eileen-replay`; `tools/repo-tools` may use `eileen-trace`'s privacy module. Three listed exemptions may spawn processes: the build guard's cargo spawn, the guard's restart of a hung core, and the development tools, each with one `#[expect]` or its own `clippy.toml`, enforced by check 14. Credentials (`SecretStore`), system volume (`SystemVolume`) and app hooks (`AppHost`) are platform traits with Windows implementations in the `-win` crates.
- Why: The crates that need threads, sleeps, spans and spawns could not reach the wrappers the lints point to, and the owner's "Windows first, not Windows only" entry lists app hooks and credential storage among the things behind traits.
- Consequences: BUILD.md section 3.1's table and section 4b.2's exception list change together; ARCHITECTURE.md's code map follows.
- Source: BUILD.md sections 3.1, 3.4 and 4b.2; [OWNER-DIRECTION.md](OWNER-DIRECTION.md).

## D56. The first run, revised

- Date: 2026-10-20
- Status: accepted (owner direction of 2026-10-20); amended by D61 and D67
- Amends: D49 (the fallback and what the recording does)
- Decision: When cloning is too slow, the first run gives the normal tour and offers the surprise later, as the owner wrote; an unticked box runs the demo in the default voice and records nothing. The prompt asks the user to start with "Hi Eileen", which teaches the name without spoiling the surprise; echo cancellation is calibrated from Eileen's own prompt; the clip sets the mic level, measures the recognizer's confidence and, if adopted, enrols the speaker check; no model is adapted. The choices and consent work by voice. A declined clone deletes the clip and the clone; the listening calibration, which holds no audio, is kept, said aloud, and removed by "forget my voice". The "faster the second time" moment uses the exact-repeat rule of D52.
- Why: The earlier text promised name, accent and echo calibration from a clip that could not provide them, and its fallback did not match the owner's.
- Consequences: WP4 acceptance 12 and WP8 acceptance 5 test the calibration, the voice path and the decline.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md section 2.8 and E18.

## D57. The thin slice is gated on named pull requests; evidence checks from the first pull request

- Date: 2026-10-20
- Status: accepted (architect, after the first verification round; the owner's standing rule applies); amended by D63 and D64
- Amends: D45 (scenario replay rules), D46 (when the evidence checks start)
- Decision: WP2 starts as soon as WP1's minimal core merges, and slice S1 waits only for four named pull requests (`wp2/s1-live`, `wp3/worker-brain`, `wp4/s1-audio`, `wp4/s1-speech`). WP0 pins only the contracts on the S1 path; the others stay provisional until their owner's first pull request. `pr-meta` runs `cargo xtask pr-meta` from WP0's CI bootstrap with the title, template, filled-evidence, test-existence and fix-regression rules, and the before-and-after evidence job follows in phase B with defined mechanics. Scenario replay feeds every input family by one of three rules (scheduled, reactive, synthesized); `bless` belongs to WP2 and `stale` expires.
- Why: The owner's evidence rule applies during the build too, and the first live loop should not wait for the whole of WP3 and WP4 or for every contract to freeze.
- Consequences: BUILD.md section 0's waves, sections 6.1 and 6.2 and checks 8 and 19 change together.
- Source: BUILD.md sections 0, 3.3, 3.3.16, 4b.3 and 6; [OWNER-DIRECTION.md](OWNER-DIRECTION.md) ("Evidence first, then the change").

## D58. The owner's answers of 2026-10-20 applied

- Date: 2026-10-20
- Status: accepted (owner answers of 2026-10-20); amended by D60 and D74
- Amends: D35 (fleet identity and where results go), D24 (the speech build), D39 (the watermark is built as an opt-in)
- Decision: The fleet identifies a machine by a salted one-way hash of its Windows MachineGuid, the same in every result and scenario; results arrive as pull requests from contributors' own accounts to an orphan `devhub` branch (or relayed by the owner), where the results history and the public `RESULTS.md` table also live. The speech build contains no GPL code: sherpa-onnx is built without espeak-ng in this repository's CI and Kokoro gets an espeak-free front end. The opt-in watermark is built with AudioSeal for exported audio and recordings. The remaining answers are applied where BUILD.md section 7 lists them.
- Why: The owner answered every question section 7 asked; these four change contracts or the build.
- Consequences: `MachineHash` replaces the random install id; `native.yml` and `devhub.yml` join the workflows.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md), "Owner answers to BUILD.md's open questions".

## D59. App routes: discovery by Eileen's own tool, remembered per app, and shown without hijacking

- Date: 2026-10-20
- Status: accepted (owner direction of 2026-10-20, "the app's own AI route first"); amended by D62 and D66
- Amends: D21 (the page-structure rung and showing), D22 (route discovery), D48 (undo per Resolve route and ResolveMCP first)
- Decision: Route discovery is Eileen's own `routes_lookup`, which queries the official MCP registry from the apps sidecar, so no worker needs web tools for it; proposals are stored, shown on the HUD and accepted only as a tier-2 Safety setting. A `route_stats` table remembers which route worked per app and task and feeds workers and introspection, and a vision or UI Automation step is never compiled for an app with a higher declared route. The page-structure rung is on-demand UI Automation of a browser window's web content. The screen lease is always announced and handed back, and a window comes forward only when the user should look. Resolve undo follows the route (Eileen's composition deleted and the previous one loaded, or the previous color version), and Resolve's own MCP server serves each op it covers once the owner is on 21.1 or later.
- Why: Discovery was assigned to workers that had no web tools, "remembers the best route" had no store or reader, page structure depended on an optional extension, and color versions do not remove a Fusion composition.
- Consequences: WP5, WP6, WP7 and WP8 each own a part, with tests listed in BUILD.md section 6.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md); BUILD.md sections 2.5, 2.6 and 2.7.

## D60. The fleet's machine hash mixes the OS machine id with a random install id

- Date: 2026-10-20
- Status: accepted (owner answer 6 of 2026-10-20); amended by D74
- Amends: D58 (its consequence that the machine hash replaces the random install id)
- Decision: `MachineHash` is BLAKE3 over a fixed Eileen salt, the OS machine id read through a new `MachineIdentity` platform trait (Windows MachineGuid; Linux `/etc/machine-id`, then `/var/lib/dbus/machine-id`; macOS later, the platform UUID) and a random install id Eileen creates on first run; where the OS has no usable id, the salt and the install id alone.
- Why: The owner's answer asks for exactly this, so that cloned virtual machines and disk images that share an OS id still get distinct hashes and machines without an OS id still have one; D58 read the answer as replacing the install id, which was the opposite.
- Consequences: `fleet\machine.toml` keeps the install id; WP2 implements the trait in `eileen-shell-win`; WP9 tests two installs on one fake OS id and the install-id-only case.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md), owner answer 6; BUILD.md sections 3.3.1, 3.4 and 4.8.

## D61. The learning loop, third revision

- Date: 2026-10-20
- Status: accepted (architect, after the second verification round; the owner's standing rule applies); amended by D65
- Amends: D52 (recipe typing, facts, distillation events, provenance precedence), D56 (the first run's gates and its phrasing)
- Decision: Recipes type every operand, with `$len` for array lengths; a fallback step may point to earlier steps of its own fallback; `call` and `delegate` steps need postconditions while `ask` and `confirm` steps have implicit ones; an `ask` fills a declared, asked slot (kind `Pick` when its options come from an earlier step); a delegate's own check is the worker's report and the objective check is the next `call`; `apps_open_url` can reuse a session already playing the video. The fact patterns match only single declarative statements with a known subject, are said back with "Noted" and are undoable; `CoreCommand` and `FactCommand` are contracts; a correction retires facts by subject key; only `Corrected` and `Forgotten` retirements queue repairs. Which sources gave accepted answers is a statistics table fed by the user's own signals, not a fact kind. Phrasings record their source turn and job, forget plans include them, and first-run phrasings are ephemeral; no-learn never suppresses a phrasing. "Learn from that now" closes the regret window, and every distillation ends in one drafted (with its hold), rejected or skipped event. Provenance and the deny-scan follow one table by literal class. The first run checks setup's clone timing before recording and deletes the clip if rendering misses its limit afterwards.
- Why: The second verification round found the worked recipes broke the linter's own rules, ordinary requests would have been stored as facts, phrasings survived "forget that", `--wait` could time out on a held draft, the deny-scan contradicted provenance, and the first run's clip could outlive a fallback to the tour.
- Consequences: WP1 acceptance 12, WP7 acceptance 3, 5, 6, 8, 9 and 11, WP4 acceptance 12 and WP8 acceptance 5 test the rules.
- Source: BUILD.md sections 2.4, 2.8, 2.10, 3.3.10 and 3.3.13; the second verification round (items a6a62e-1, -3, -5, -6, -8, aedf46-1, aedf46-5, L1 and L8).

## D62. Containment, calls and the brain's slots, revised

- Date: 2026-10-20
- Status: accepted (architect, after the second verification round; owner answer 19 for calls); amended by D66 and D67
- Amends: D40 (git internals, path class order, egress after taint), D51 (the follow-up feature and calls), D53 (how the fallback brain is addressed), D59 (route proposals and installs)
- Decision: Writes to a repository's git directory and git configuration are tier 2 and HUD only, workers run with `core.fsmonitor` off, and hook-running git commands become tier 2 after taint in a repository with hooks. Path classes match most specific first, so a job's own folders and snapshot inside Eileen's data folder are not tier 3 for that job. Apps Eileen opens for the user are started by the shell, outside every Eileen job. One table decides which reads are observed and of which kind, covering the browser and mail trees, Claude in Chrome, third-party MCP servers, egress commands and `routes_lookup`. After taint and a private read, queries to vouched hosts, browser input and files a browser opens are tier 2, and hosts in a brain-dispatched task must pass the brain's URL rule. A route proposal must match a registry entry and is shown and spoken only from registry fields; an approved server is installed once at its exact version. During a call only name-led utterances are addressed and Eileen stays silent. A conversation-window follow-up counts only when it resolves against Eileen's last turn. Brain commands name their slot and brain events carry their process instance, so a stalled brain's late answer is dropped.
- Why: Each closes a gap the second verification round found: git hooks bypassed the taint rule, the data folder contradicted job folders inside it, launched apps inherited a kill-on-close job, poisoned pages read through UI Automation escaped the whole-draft hold, the brain could hand an attacker's URL to a worker, a worker could propose any server in Eileen's voice, calls contradicted owner answer 19, and the core could not tell two answers to one turn apart.
- Consequences: WP0 phase B builds `user_launch`; WP1 acceptance 6 and 12, WP2 acceptance 10, WP5 acceptance 11 and 12, WP6 acceptance 8, WP7 acceptance 9 and WP8 acceptance 7 test them; section 7 names the git hook limit.
- Source: BUILD.md sections 2.1, 2.3, 2.5, 2.8, 2.9, 2.12, 3.3.3 and 3.3.9; the second verification round (items ab65bc-1, -2, -5, -11, aedf46-2, aedf46-7, L3, L4 and L8).

## D63. Evidence has a grammar and its own required check; Dependabot updates cargo; scenario keys are stable

- Date: 2026-10-20
- Status: accepted (architect, after the second verification round; the owner's standing rule applies); amended by D68
- Amends: D57 (the evidence job's home and the bot rule; scenario replay keys and expiry)
- Decision: The Evidence section has field lines (`tests:`, `scenarios:`, `numbers:`, `replay:`, `incident:`, `owner-run:`, `override:`) and only `tests:` and `scenarios:` are read as ids; tests resolve in any workspace member, `xtask` and `repo-tools` included; `type:` sits under its own heading. The evidence job moves to `evidence.yml`, re-runs on description and label edits, and becomes a required check from phase B; cases Linux cannot show fail until the owner labels them reviewed. Dependabot opens the cargo and action update pull requests with a 14-day cooldown, and no workflow creates pull requests. Scenario replay keys reactive families without digesting composed prompts, feeds the brain's and workers' tool calls inside their turn's sequence, schedules environment events, and one shared function applies the stale and quarantine expiry in both checks.
- Why: Ids could not be parsed from prose, W0's own tests could not be named, an edited description kept a stale evidence result, a pull request opened with the workflow token never runs the required checks, and prompt rewording would have made every brain scenario stale.
- Consequences: WP0 acceptance 8 and 11, WP1 acceptance 13 and WP2 acceptance 11 test them; the pull request template follows the grammar.
- Source: BUILD.md sections 3.3.14, 3.3.16, 4.6 and 4b.3; the second verification round (items a184cd-2, a184cd-4, L6 and L8).

## D64. The slice in W1, and memory thresholds from this PC's own measurements

- Date: 2026-10-20
- Status: accepted (architect, after the second verification round; the owner's standing rule applies); amended by D68 and D69
- Amends: D54 (the thresholds and the pool), D57 (when WP4's slice pull requests run)
- Decision: WP4's two slice pull requests run in W1, sharing the desktop lock with WP6's spike, so S1 can pass while WP1 deepens the simulator. The build guard's thresholds come from a baseline day measured on the owner's PC in W0, its job count never falls as memory rises, and guard processes share their samples. Memory admission's paging limit defaults to `auto`, learned from the PC's own week of paging, and WP3's W1 record must show a cold worker admitted on a typical day. The deep pool session is kept first and may start on a quiet minute; deep answers that start cold are a named v0.1 limit with B6 targets. Benchmarks are stratified by how their jobs were admitted instead of excluded below a fixed free-memory figure, and the M6 demo must start its three agents without "start it anyway".
- Why: A live read with several agent sessions showed about 19,400 KB/s of paging and 275 MB available, under which the fixed defaults would refuse every build and admission, and the old exclusions would have discarded most benchmark runs on this PC.
- Consequences: WP0 acceptance 4, WP3 acceptance 3 and WP9 acceptance 2 and 6 carry the checks; section 7 lists the remaining risks.
- Source: BUILD.md sections 0, 2.5, 4.3, 4.6, 4.7 and 6.1; the second verification round (items aedf46-10, a1129a-1, a1129a-5 and a1129a-7).

## D65. The learning loop, fourth revision

- Date: 2026-10-21
- Status: accepted (architect, after the third verification round; the owner's standing rule applies); amended by D70 and D77
- Amends: D61 (shadow agreement, distillation events, provenance of recipe text, facts and phrasings), D41 (when the brain is recycled, and how a paraphrase may run a brain action without a model)
- Decision: Shadow agreement is one table-tested function with a rule per case: asked slots from the user's answer to the worker's question, unobserved reads, shadow checks the core issues for postconditions that call a check op, fallbacks taken where a replay would take them, a delegate's span of worker calls checked by the `call` after it, extra worker calls, and three verdicts. Delegate goals and ask and confirm questions are literals for provenance and the deny patterns, and are held for approval when they trace nowhere. Ops are named the same way whether a tool selects them through `op` or `action`. A job is distilled at most once, `--wait` reads the recorded outcome after subscribing, and every attempt ends in drafted, rejected, skipped (with a typed reason) or deferred. An `Act` job starts with the taint its brain turn's context held, and an approved third-party server's tool names count as provenance `Tool`. "X means Y" lives only in the phrasing store with a subject key, so a correction replaces every alias of that subject; feedback facts take their object as subject and the answer's topic as scope only when they refer to it; the statements-only tests read slot text; a complaint lexicon keeps complaints as requests. An L2 paraphrase of a confirmed phrasing runs only above a measured similarity and when its entity names match the target's. The brain is recycled only on its context threshold, a forget of something it saw, or a crash.
- Why: The third verification round found that shadow agreement could not decide a delegated or fallback run, worked recipes contradicted the eligibility rule, `--wait` could miss an outcome or time out without a cause, brain actions escaped taint, corrections left two live aliases, and a paraphrase could launch the wrong app.
- Consequences: WP7 acceptance 3, 5, 8, 9 and 11, WP1 acceptance 6 and 12 and WP9 acceptance 6 test the rules; WP1 issues shadow checks through `SkillEngine::shadow_checks`.
- Source: BUILD.md sections 2.4, 2.10, 2.12, 3.3.4, 3.3.9, 3.3.10, 3.3.13 and 3.3.15; the third verification round (items a6a62e-1, -3, -5, -8 and aedf46-1).

## D66. Containment and route proposals, third revision

- Date: 2026-10-21
- Status: accepted (architect, after the third verification round; the owner's standing rule applies); amended by D71
- Amends: D62 (git internals and the order of path classes, the observed-read table, the URL rule), D59 (how a route proposal names its publisher)
- Decision: Any `.git` path component is git internals, path classes are tried in one fixed order with credentials first and git internals before the job's folders, the global git options that choose another repository or configuration are tier 2, workers pin `safe.bareRepository=explicit`, and every git command carries the repository it will use and whether that repository has hooks; after taint a git command outside the job's own repository asks. One URL function vouches for URLs everywhere: built by Eileen's code, a host the user named with nothing added, or a full URL the user visited, never a host whose page is in the current turn's own context. A dispatched job whose task repeats ledger lines or out-of-scope facts starts as having read private data. The observed-read table covers window lists, waits, action verification hints, media status, recent work and the ledger, and a free-text op must declare a source or a listed reason. A route proposal names its publisher by the registry-verified namespace of the server name, says whether that namespace belongs to the app's maker by Eileen's own vendor table, never reads the registry's metadata as an endorsement and never says "official".
- Why: Nested repositories, rewritten worktree pointers and in-place `.git` folders escaped the git rules; data split across short path segments, or sent to the visited page that asked for it, passed the URL rule; several reads returning third-party text set no taint; and the registry has no endorsement flag, so the spoken "official" could have been a squatter's.
- Consequences: WP1 acceptance 6, WP2 acceptance 10, WP6 acceptance 8 and WP7 acceptance 9 test them; WP6 maintains `app-vendors.toml`; section 7 names the git exposure that remains for an untainted job.
- Source: BUILD.md sections 2.5, 2.12, 3.3.3, 3.3.9 and 3.3.13; the third verification round (items ab65bc-1, ab65bc-5, ab65bc-11 and a6a62e-5).

## D67. The follow-up rule and the speech level are contracts

- Date: 2026-10-21
- Status: accepted (architect, after the third verification round; the owner's standing rule applies)
- Amends: D62 (the conversation window's follow-up feature), D56 (what the first run's calibration feeds)
- Decision: The core fills a record of what its own last turn mentioned and asks the intent resolver whether an unnamed utterance follows up on it; WP7 owns that rule with golden cases, and the slice's resolver answers no until then. Every end of turn carries its speech level, the first run's calibration reaches the core as a typed record, and the addressee score is a defined function whose speech-level check rejects a voice much quieter than the user's reference; without a calibration the reference is the median level of name-led turns.
- Why: The follow-up rule named the slot resolvers, which the core cannot reach except through a port that carried none of the needed data, and the first run's level reference was written to disk with no path into the score.
- Consequences: WP1 acceptance 12, WP4 acceptance 12 and WP7 acceptance 3 test them; section 7 keeps the limit that an equally loud second voice is not greyed without the speaker check.
- Source: BUILD.md sections 2.8, 3.3.3, 3.3.10, 3.3.11 and 3.3.15; the third verification round (items aedf46-2, L1 and L8).

## D68. Evidence, scenario replay and the slice, revised

- Date: 2026-10-21
- Status: accepted (architect, after the third verification round; the owner's standing rule applies); amended by D72
- Amends: D63 (the type rule, the bot rule, the reviewed label, test paths, scenario replay rules and keys, the expiry contract), D64 (what slice S1 needs in W1)
- Decision: A pull request's type must fit its changed files, Dependabot's pull requests are recognised by their author, the `evidence-reviewed` label counts only when applied after the head commit arrived, and integration tests are named by their test target. Every brain and worker event has a replay rule, the replayer mints brain instance numbers, hooks are replayed inside the worker's sequence and a changed decision fails the replay, the expiry rule's types are defined with dates written into the scenario file, and reactive keys use transcript digests and occurrences instead of ordinals. The minimal core includes barge-in, slice S1 gets a minimal real resolver in WP2 that WP7 replaces, and the live run takes the desktop lock.
- Why: A code change typed as docs skipped evidence, the bot template could not exist, a label survived later pushes, several events had no replay rule, the expiry function could not be written from its signature, and S1's fast-path command and barge-in had no owner in W1.
- Consequences: WP0 acceptance 11, WP1 acceptance 13 and WP2 acceptance 1 and 11 test them.
- Source: BUILD.md sections 3.3.1, 3.3.14, 3.3.16, 3.5, 4.6, 4b.3, 6.1 and 6.2 to 6.4; the third verification round (items a184cd-2, a184cd-4, aedf46-10 and L8).

## D69. Memory thresholds are machine-wide

- Date: 2026-10-21
- Status: accepted (architect, after the third verification round; the owner's standing rule applies); amended by D73
- Amends: D64 (the build guard's re-check and floor, where the paging history lives, the clamp, when WP3's admission record is taken), D54 (the paging limit's source)
- Decision: The build guard's baseline ignores samples taken during its own builds, its re-check limit is the 95th percentile plus 25 percent and never under twice the admission limit, one headroom floor between 1 and 2 GB applies after the build's own estimated commit, and its re-check wait is bounded. Memory samples and the 30 s paging averages behind the `auto` paging limit live in a machine-wide folder with one defined format and writer, seeded from the guard's baseline, so a fresh data directory uses what the PC has learned. The clamp ceiling rises to 40,000 KB/s, and WP3's admission record is taken in W2 on the full runtime.
- Why: The re-check limits were undefined and could stall builds on this PC, the floor could admit a build with under 1 GB of headroom, and the M6 demo, benchmarks and tests would have used a 5,000 KB/s fallback against typical paging near 19,400 KB/s.
- Consequences: WP0 acceptance 4, WP3 acceptance 3 and WP9 acceptance 6 carry the checks.
- Source: BUILD.md sections 2.5, 2.15, 3.1, 4.1 and 6.1; the third verification round (items a1129a-1 and a1129a-7).

## D70. The learning loop, fifth revision

- Date: 2026-10-21
- Status: accepted (architect, after the fourth verification round; the owner's standing rule applies)
- Amends: D65 (delegate spans, question matching and call assignment in shadow agreement, distillation outcomes, what a tainted step keeps, fact retirement)
- Decision: A selector tool's recorded arguments keep the fields outside the selector. A delegate's span starts after the latest match of the steps before it in its own list and holds only calls no other rule accounts for; a question with no options agrees with any question the worker asked in the same interval; shadow agreement searches for one consistent assignment of worker calls to steps, within a stated bound. A rejected job is never distilled again, rows left by a stopped runtime or a crashed librarian become deferred outcomes with their event, jobs that never ran on Claude get a stated outcome, and benchmarks judge distillation outcomes by run index, expecting statistics-only or too-short skips after the drafting run. A tainted step keeps vocabulary literals, slots and pointers into fields no outside party wrote, plus match-only pointers, so the worked recipes compile from their own evidence; job status reads carry their jobs' taint; an approved server's tool list is read once after its install. Refinement words take their dimension as subject, and a statement retires its subject's facts in its own scope and in no scope (in every scope when it has none).
- Why: The fourth verification round found that selector steps lost their window or session, a delegate with no pointer had no span start, the ladder table sent drafts with held lines straight to level 1, two identical calls could be matched wrongly, rejected and orphaned jobs had contradictory or missing outcomes, benchmarks would have recorded expected skips as failures, the worked recipes could not compile from their own tainted evidence, and corrections either retired too much or left two contradicting facts active.
- Consequences: WP1 acceptance 6 and 12, WP7 acceptance 3, 5, 8 and 9 and WP9 acceptance 2 test the rules; WP7 records the assignment search's largest count (BUILD.md section 7, "Carried into packages").
- Source: BUILD.md sections 2.4, 2.5, 2.10, 2.12, 3.3.4, 3.3.9, 3.3.10, 3.3.13 and 4.7; the fourth verification round (items a6a62e-1, -3, -5 and -8).

## D71. Git internals, environment assignments and the URL rule's inputs

- Date: 2026-10-21
- Status: accepted (architect, after the fourth verification round; the owner's standing rule applies)
- Amends: D66 (where git internals apply, the URL rule's inputs, private reads in dispatched jobs), D40 (the shell policy)
- Decision: A `.git` path component is git internals only inside the job's own folders, besides the job's own git directories; anywhere else it keeps its class for reads and makes writes ask on the HUD, and the component is recognised the way git's own NTFS protection recognises it. Any environment assignment in a command line makes the command ask on the HUD, except for five variables that name no program, file or folder. The URL rule reads a fact the runtime attaches per URL (whether it is in the media history or recent-work index, and whether the user visited it, from the ledger's browser lines, which now keep the address-bar URL) and a context the core builds (including the address-bar hosts of the pages in the deciding context); an unreadable address bar makes the visited rule vouch for nothing. A dispatched job starts as having read private data whenever its turn held ledger lines.
- Why: The any-`.git` rule had made other repositories' configuration readable without asking, environment prefixes let a worker-written configuration or program run inside an allowed command, the URL function named inputs no contract carried, and the overlap test let a paraphrased ledger line through.
- Consequences: WP1 acceptance 6, WP2 acceptance 10 and WP5 acceptance 13 test them; WP2 and WP5 settle the two measured questions (BUILD.md section 7, "Carried into packages"); section 7 names the paraphrased-fact limit.
- Source: BUILD.md sections 2.6, 2.10, 2.12, 3.3.3 and 3.3.5; the fourth verification round (items ab65bc-1 and ab65bc-5).

## D72. Hook answers are part of a recording

- Date: 2026-10-21
- Status: accepted (architect, after the fourth verification round; the owner's standing rule applies)
- Amends: D68 (scenario replay of hooks, reactive keys, the expiry check)
- Decision: The recorder writes the core's answer to every gating hook as its own line beside the hook input, strict replay compares it, scenario replay fails on a changed answer, and trimming keeps it. A brain turn sent again with an unchanged transcript keeps its key and is told apart by its send ordinal. The expiry check fails a `since` date later than the commit that introduced it.
- Why: The hook-divergence rule compared against a recorded decision that no format stored, one acceptance line still described the old key, re-sends had no defined key, and the commit-date comparison had no pass condition.
- Consequences: WP0 acceptance 8 and WP1 acceptance 13 test them.
- Source: BUILD.md sections 3.3.14, 3.3.16 and 4b.3; the fourth verification round (item a184cd-4).

## D73. The machine-wide memory files have one module

- Date: 2026-10-21
- Status: accepted (architect, after the fourth verification round; the owner's standing rule applies); amended by D76
- Amends: D69 (the paging history's format and writer, the baseline's keys, the tests)
- Decision: `eileen_buildguard::samples` owns the memory samples, the 30 s paging windows (with a mark for windows a build slot held, which the learned limit leaves out, and a rule that keeps two runtimes from writing one window twice), the keys of the guard's baseline file and the computation of the learned paging limit, so the runtime still depends on that one module. Its root comes from `LOCALAPPDATA`, so tests run against a temporary folder, and a runtime with a fake memory source writes nothing there.
- Why: The runtime read a baseline file whose keys and reader were undefined, the paging history had no format, lock or build mark, and no test covered the fallback chain, the clamp or a test runtime writing into the PC's learned history.
- Consequences: WP0 acceptance 4 and WP2 acceptance 12 test them.
- Source: BUILD.md sections 2.5, 2.15, 3.1, 3.3.11 and 6.1; the fourth verification round (item a1129a-7).

## D74. The owner's answers applied in full

- Date: 2026-10-21
- Status: accepted (owner answers of 2026-10-20; the owner's standing rule for the four questions it settles)
- Amends: D60 (where the machine hash is computed and how its inputs are encoded), D58 (how fleet results reach the `devhub` branch, and the operating system named in a result), D53 (the brain and the workers report their languages too)
- Decision: The install id is created by `eileen setup` or the first `eileen run`, and the runtime recomputes the machine hash at every start through the platform layer: the `MachineIdentity` trait (Windows MachineGuid in `eileen-shell-win`; a neutral reader of Linux's machine-id files in `eileen-platform`, tested on Linux from v0.1; macOS later) and one derivation in `eileen_platform::machine`, which normalises the OS id and length-prefixes its inputs so every OS gets the same rule. Scenario saves, voice consent records, fleet results and benchmark lines all read that one value from `fleet\machine.toml`. Fleet results reach the `devhub` branch as a pull request from the contributor's own account, as a contributor's own commit pushed there (for those with write access, validated on push), or relayed by the owner from his account, and each result names its operating system and build. The `ml_project` job kind is reserved in the contracts and refused with one plain sentence until v0.2's Colab connector exists. The brain backend and the worker driver report the languages they support, like the speech stages, from the component itself, never from a list in Eileen's code. Resolve's own MCP server is the primary Resolve route once present, and the Lua bridge stays a tested fallback through a route switch on the Resolve doctor. The owner's nightly lane is unattended from its first night. The four questions section 7 still asked (the two release assets, the memory admission defaults, `evidence` as a third required check, the listening calibration after a declined clone) are settled by the standing rule as recommended, as D54, D56, D58 and D63 already describe. The dated records no longer name the earlier working names or the franchise.
- Why: Owner answers 6, 9, 10, 15 and 21, and the owner's entries after them. Scenario capture ships with WP2 in W1 and W2 and the consent record with WP4, so a hash that only WP9's fleet runner computed in W6 could not appear "in every result and scenario from that machine"; without normalisation, Windows' GUID form and Linux's plain form would follow two rules; a results channel that took only pull requests left out the owner's "commits" and his relays; and a job kind added in v0.2 would change a pinned contract that v0.1 can reserve now.
- Consequences: WP0 acceptance 12, WP1 acceptance 14, WP2 acceptance 8 and 13, WP3 acceptance 6, WP6 acceptance 1 and WP9 acceptance 4 test them; `eileen-runtime` may depend on `eileen-platform`, which its memory sampler already needed.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md), "Owner answers to BUILD.md's open questions" and the entries after it; BUILD.md sections 2.5, 2.7, 2.8, 2.15, 3.3.1, 3.3.2, 3.3.8, 3.3.14, 3.3.16, 3.4, 4.2, 4.6, 4.7, 4.8 and 7.

## D75. The playbook check accepts a stated reason

- Date: 2026-10-21
- Status: accepted (architect, from the suggested guidance changes of pull request #12; the owner's standing rule applies)
- Amends: D50 (its consequence that check 30 makes every change to this log update the playbook in the same pull request)
- Decision: Check 30 passes a pull request that changes this decision log when the pull request also changes `docs/PLAYBOOK.md`, or when its description says under "Playbook" why the playbook is unaffected, and in both cases only if the generated `templates/` copies are current (check 9). The rest of D50 stands.
- Why: Check 30 in BUILD.md section 4b.3 reads this way, and the playbook describes it so; D50's consequence named only the first way and left out the templates. A change that leaves everything the playbook says true, such as a status line that only points forward, can give a reason instead of an edit.
- Consequences: Nothing changes in the build: WP0 phase B implements check 30 as section 4b.3 writes it. The playbook cites this record beside D50.
- Source: BUILD.md section 4b.3, check 30; the description of pull request #12, "Suggested guidance changes"; [PLAYBOOK.md](PLAYBOOK.md) section 1.7.

## D76. Fit the machine, and gate on the speaker only above a measured bar

- Date: 2026-10-21
- Status: accepted (owner direction of 2026-10-21)
- Amends: D54 (the low-memory profile becomes one outcome of the machine profile's plan, chosen from the PC's usual free memory; admission's rules are unchanged), D73 (the `samples` module also owns the hourly memory summaries), D51 (a voice approval needs a speaker match only while the speaker gate is on, which now means the check cleared a measured bar and the user's own shadow period)
- Decision: One mechanism, the machine profile, decides how Eileen runs on the PC it is on. It reads the PC at setup, at every start and when Windows reports a change (memory and its usual free figure from 30 days of hourly summaries, CPU cores by efficiency class, GPU and NPU adapters and the execution providers, disk, the OS build, power, the audio endpoints), runs short benchmarks only at setup, on request and when a fact they depend on changes, and never benchmarks Claude. A pure function in the core turns that into a plan: the speech models, the deep pool session, the hot spare and the computer-use pool session filled in order of value from a memory budget, and the speech threads, the worker cap, build-heavy jobs, the L2 model, residency, disk caps and idle-time work set from the facts and measurements, each with a reason. Memory admission and the build guard keep their rules as the per-job gates; a live re-check lets optional residents go under pressure and brings them back after a hold, and a speech model is reloaded only after a stable new choice, at a quiet moment. Every choice is explained in sentences from Eileen's own templates, once after setup, on request and on a HUD panel, and every fitted setting is `auto` until the user overrides it, which the plan never undoes. A speaker check gates nothing (no utterance, follow-up or approval is refused for voice) unless WP4 measured it on the owner's consented, command-length speech in real room conditions with both the false-reject and the false-accept one-sided 95 percent upper bounds under 1 percent on at least 300 held-out trials each; above that bar it gates a user only after a shadow period shows the same for that user's own turns, and it turns itself off after 3 false rejects in 300 gated turns. Below the bar, the protections that never depended on it stay: echo cancellation, no approval from Eileen's own audio, physical clicks and the hotkey, and confirmation for consequential actions.
- Why: The owner's entries of 2026-10-21, "Run to fit the machine it is on" and "Speaker recognition only if it is really accurate; otherwise respect the command". The design adapted to memory in four places with separate rules and no way to say why, and read nothing else about the PC, so a friend's PC or a 32 GB upgrade would have run with the owner's tight-memory behaviour or needed hand tuning. "Adopted only if it measures well" named no bar, and a check that refuses the owner even rarely breaks the first rule, that Eileen respects the command it hears.
- Consequences: WP0 phase B adds the `machine` contracts, the new top-level variants, the `MachineProbe` platform trait with three fixture machines and the hourly summaries; WP1 builds the plan, the re-check and the speaker gate, with simulator invariant 16; WP2 the probe, the files and `eileen doctor machine`; WP4 the benchmarks, the endpoint listing, the command accuracy of each recognizer and the speaker measurement; WP7 the `auto` keys and the `MachineFit` check; WP8 the panel and chip; WP9 the plan class in results. WP0 acceptance 13, WP1 acceptance 15 and 16, WP2 acceptance 14, WP4 acceptance 13 and 14, WP7 acceptance 12, WP8 acceptance 11 and WP9 acceptance 9 test them. The build guard's job count is also capped at the logical core count.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md), entry of 2026-10-21; BUILD.md sections 1 (E19), 2.3, 2.5, 2.8, 2.15, 2.16, 3.3.17, 3.4, 4.2, 4.4, 4.7, 6.1 to 6.11 and 7.

## D77. Learn the person through the models' own memory, use each tool's native strengths, and adopt the ten learning ideas

- Date: 2026-10-25
- Status: accepted (owner direction of 2026-10-22, with the owner's standing rule for the ten ideas' phasing and the design's recommendations); two narrower parts are proposed until the owner answers: reading his own Claude notes and instruction files under the user's `.claude` folder, which D4 forbids, and letting a session kind drop `--restricted` to keep native notes, which D40 makes a second fence. Two recommendations put to the owner apply by the standing rule until he answers: local first with cloud-side memories opt-in per route, the recommendation put to him on 2026-10-22; and keeping Claude's own note-taking to coding jobs in v0.1, which narrows his ask (the alternative gives the brain and general workers note proposals, each held for Keep or Drop; BUILD.md section 7, "Waiting for the owner")
- Amends: D27 (its consequence that auto memory is off in every session Eileen starts: the claude CLI's own notes now hold the person, by the rules below, and session persistence stays off in v0.1 except for `continue_session`), D44 (the user model's storage: the text of `user`, `feedback`, `project` and `reference` facts lives in the CLI's native notes, and the `learn.sqlite` row becomes a derived index with no note text; the fact rules of D52, D61, D65 and D70 are unchanged), D65 (a forget of a note in the brain's starting index recycles it, make-before-break), D40 (the path guard: the notes folders under Eileen's data directory are a `Notes` class with their own read and write tiers, section 2.12); proposed, pending the owner: D4 (its "no reading `~/.claude`" consequence, narrowed to the user's own Markdown notes and instruction files after a yes; the credential rule stands) and D40 (a session kind may run without `--restricted` to keep native notes)
- Decision: Eileen learns the person through the memory the vendors ship in their official tools and builds on it, uses each route's native strengths wherever they serve the product, and builds itself only what no vendor provides: the fast path that needs no model, permissions and confirmations, and the rules that must hold the same way on every route. On the Claude route the person lives in the claude CLI's own auto memory notes (a `MEMORY.md` index and topic files, in the CLI's own format), kept in Eileen's data directory: one hub per user for `user`, `feedback` and `reference` notes, written only by Eileen's writer and never any session's `autoMemoryDirectory`, and one folder per repository for `project` notes. A coding session that writes natively gets its repository's folder through `autoMemoryDirectory` in its settings file, checked against `system/init`, so nothing lands in the user's own Claude folder. Eileen keeps only a derived index of those folders (the fact rows, with a file reference and hash and no note text). Sessions read the hub as follows: the brain through its system prompt at spawn plus a per-turn `[notes]` block of what changed since, coding sessions through a read-only prompt file, and the web and screen workers and the pool only as note lines the core picks for the job; `continue_session`, whose transcript persists under the user's own Claude folder where a forget cannot reach, gets none in v0.1 unless WP3 shows its fork runs without session persistence, and the librarian, the scribe and one-turn probes get none. The path guard lets a session read only the notes folders its own memory spec names, and a memory folder never goes into `--add-dir`. Only coding sessions write notes natively, with `Write` and `Edit` in their repository's folder, and only once WP3's probe shows every note write passes the `PreToolUse` guard and nothing writes beside it, WP7's note acceptance has passed, and `--restricted` either still allows it or the owner has accepted the trade; every other note is written by Eileen's writer with no model, from the user's own words, a `learned_correct` span, an approved held note or an approved import. The guard holds every model-written note from a tainted job, and from an untainted job unless it traces to the user's words in that job or is a project note about that repository that shares no run of eight tokens with any file text the job read; held notes wait on the HUD for Keep or Drop, and a change no Eileen session or writer made is restored and held the same way. Notes never set a tier, a grant or a vouched URL; reading any note sets `private_read`; a forget reaches every copy (topic file, index line, held copies, the trajectory steps and hook rows that wrote it, the turns that carried it, its source turn) and leaves a tombstone of keyed hashes; and file checkpointing is off, so the CLI keeps no copies of its own. A note-backed fact unconfirmed for 90 days retires as `Stale` and leaves the index the sessions load, keeping its file until the user tidies. Until the owner answers the open point, cloud-side vendor memories are opt-in per route (`privacy.vendor_cloud.<route>`, off by default), through each vendor's own manual export and import only, line by line; Eileen never signs in to a vendor app or drives one to read memory. Codex, Grok, DeepSeek and local routes each have a memory design for when they are built (Codex Memories with per-thread controls and never a global config change, Grok Build's memory set per child with its Claude compatibility switches off, DeepSeek's own notes folder, a local route that shares the hub only on a loopback endpoint), and one derived person file carries the notes the user lets travel to each, through its own per-session channel. Other native strengths join v0.1: `PostToolUse` and `InstructionsLoaded` hooks for receipts, `--fallback-model` for workers, the dynamic boundary in the brain's system prompt, a CLI version floor for the CLI's own note hardening, a measurement of native compaction against recycling the brain, a repository's own instruction files for coding jobs through `--add-dir`, and plugin evals in the owner's nightly lane. Ayham's ten ideas are adopted: receipts (1), typed templates induced from the user's phrasings (3), corrections kept at the step (4), fault-injected recipe checks (8) and re-proving skills after a change (9) in v0.1; the check-in (2), backtesting the ladder (5), undo seen by its effect (6), a rejected attempt paired with the retry that worked (7) and an attacker model for the linter (10) after v0.1.
- Why: The owner's entry of 2026-10-22: "we use how the models learn the person directly" and "we should use every advantage to respect the whole idea of Eileen". The claude CLI already saves the four kinds of notes Eileen's user model had borrowed from it, so a second model of the person rebuilt what the vendor ships, and the reason auto memory was turned off (nothing learned should hide in the user's own Claude folder) has a native answer in a pinned folder inside Eileen's data directory. The critics' review of the first design found that the CLI also recalls and extracts memory outside visible tool calls, that an untainted coding job can still read text asking it to save a note, that web-reading workers holding the whole index could leak it after a page told them to, that note text lands in more places than the forget plan reached, and that a fixed hub path would mix benchmark and test notes into the owner's own; hence the probe gate, the held writes, the narrower loading, the wider forget and a hub that follows `EILEEN_DATA_DIR`. Local first costs almost nothing, because no vendor's CLI can read its app's memory. The alternative the owner can choose, cloud memories as first-class sources (claude.ai synced skills in sessions, Codex cloud tasks, a monthly paste of each app's memory), would leave "what do you know about me" and "forget that" incomplete, put more personal data on vendor servers and need his own amendment of the 2026-10-20 "on the user's PC" line, for little gain. The two narrower parts conflict with D4 and D40, which the owner's words do not address, so by the rule to flag conflicts they wait for him. Keeping Claude's own note-taking to coding jobs in v0.1 follows from the safety findings (the brain's and the other workers' context always holds untrusted text, and recall would sit on the voice path), but it narrows what he asked for, so it is put to him as well, with held proposals from the brain and general workers as the alternative.
- Consequences: BUILD.md sections 0, 1 (E11 and the proposed-ideas tables), 2.3, 2.4, 2.5, 2.9, 2.10 (items 1, 2, 5, 6 and 9 to 13, and the new item 15), 2.11, 2.12, 2.15, 3.3.8's mapping table, 3.3.13's provisional shapes, 3.5, WP1, WP3, WP7 and WP8, section 7 and section 8 change, and ARCHITECTURE.md's state, crate and learning lines follow. WP3 keeps `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` in every session until its memory gate passes, `learning.vendor_memory.claude` stays off until WP7's note acceptance passes, and WP0, WP1, WP3, WP7 and WP8 apply the provisional contract shapes by amendment. WP3 acceptance 4 stands unless the owner approves the D4 part; then CONTRIBUTING.md's hard rule 2 and check 14 take the same narrowing by amendment.
- Source: [OWNER-DIRECTION.md](OWNER-DIRECTION.md#2026-10-22), entry of 2026-10-22, and Ayham's section "Proposed 2026-10-22"; [2026-10-learning-the-person.md](research/2026-10-learning-the-person.md); BUILD.md sections 2.3, 2.5, 2.10, 2.12, 2.15, 3.3.13, 6 and 7.

## D78. Three build-time zip advisories are ignored until the sherpa-onnx pin can move

- Date: 2026-10-25
- Status: accepted (WP0, from `wp0/stubs`; the owner's standing rule applies); amended by D80
- Decision: `deny.toml` ignores RUSTSEC-2026-0367, RUSTSEC-2026-0368 and RUSTSEC-2026-0369, each entry with its reason naming this record, and nothing else. All three are against `zip` 2.6.1, which `sherpa-onnx-sys` 1.13.8 (pinned exactly in BUILD.md section 4.1) takes as a build dependency. The only patched release is `zip` 9.0.0, which no 1.x release of `sherpa-onnx-sys` accepts. The ignores come out in the pull request that moves the pin to a `sherpa-onnx` release whose build script takes a patched `zip`, once that release is past the cooling-off period; WP4 owns the pin.
- Why: The three advisories are about extracting a hostile archive: symbolic links whose targets leave the destination, and permission bits applied through them. `sherpa-onnx-sys`'s build script calls `zip` only for the iOS `.xcframework.zip` archives. On Windows it unpacks a `.tar.bz2` with `tar` and `bzip2`, Linux never builds it (it sits under the Windows target table), and a build dependency links into no binary. So no Eileen build runs the vulnerable code. The alternatives were worse: leaving `sherpa-onnx` out of the stubs would hide native build failures until WP4's first pull request, which BUILD.md section 3.1 declares the native dependencies in W0 to prevent, and `sherpa-onnx-sys` 2.0.0 (2026-10-23) is a major release inside the cooling-off period that would change WP4's speech build.
- Consequences: `cargo deny check` passes on both targets with the `webpki-roots` license exception and these three ignores (WP0 acceptance 3). Any other advisory, against `zip` or any other crate, still fails the job. `deny.toml`'s `bans` section allows `zip` only as a direct dependency of `sherpa-onnx-sys`, so the ignores cannot cover a `zip` that arrives any other way (amendment of 2026-10-25).
- Source: BUILD.md sections 3.1, 4.1, 4b.3 and 6.2 and its revision log entry of 2026-10-25 from `wp0/stubs`; the three advisories in the RustSec database (read 2026-10-25); `build.rs` of `sherpa-onnx-sys` 1.13.8.

## D79. Keep later connectors, routines and devices possible

- Date: 2026-10-25
- Status: accepted (the owner, in the build session of 2026-10-25, approving the recommendations and asking for a speaker recognition option beside them)
- Decision: Five provisional reservations, written into BUILD.md sections 2.14, 2.12 and the code blocks of section 3.3 as comments, keep later connectors, routines and devices a new plug-in rather than a redesign, without naming any app or service: (1) a general way into the core, `Input::Connector` with `ConnectorEvent::Observed`, and manifests that can wrap an approved third-party MCP server; (2) data classes as open string ids on manifests, ops and taint, with `privacy.classes.<id>` rules for speaking, showing, routes and egress; (3) paired devices whose turns may be addressed, opt-in per device, and room for a device-signed approval with a nonce; (4) a `sink` beside the speech frames' `source` and a reply surface per turn; (5) routines with standing authorization (`TurnSource::Routine`, `Trigger { Time, Event }`) and a budget lane per job. Four conflicts with earlier entries are settled: a draft from a job that runs while nobody watches is a finished artifact, never an open approval; a disclosure rule decides by the audience state (alone, others present, in a call) and a private screen, and may carry an optional speaker condition that takes effect only while the speaker gate is `on`, after D76's measured bar; a model trained on the user's data stays on the PC, records the data it was trained on, and is disabled when any of that data is forgotten, until it is retrained; a continuous scout runs after v0.1 in its own capped budget lane. Third-party names that stood in for a generic capability are made generic where the text allows it now, and the pinned names (`JobSpec.chrome`, `ResolvePlayhead`) change when their modules next change.
- Why: The owner asked on 2026-10-25 that the architecture support capabilities that are not planned yet (personal-data sources linked locally, the phone and the car as ways in, triggers by place, stricter rules for sensitive data, agents that keep looking for better tools, new devices) without hard-coding the apps involved. A review of the design against those capabilities found five places that would force a redesign: `Input::Sidecar` and `ConnectorEvent` know only the built-in sidecars, taint has one bit of privacy, a connector's turns can only be ambient, a reply has no destination, and every job needs an addressed turn. It also found four conflicts. Owner answer 20 says "Unanswered approvals wait on the HUD until the job's 30-minute deadline", while work that runs overnight ends in things to approve in the morning; a draft that is not an approval keeps both. The owner's entry of 2026-10-21 says "Eileen does not need to recognise who is speaking. Mostly it should respect the command it hears", while a rule such as "this class answers only the owner" needs to know who asks; the audience state and the private screen decide without a voice check, and the owner's addition, a speaker condition, applies only once D76's bar is met. ROADMAP said "Eileen composes models and adapts no weights of its own", and ARCHITECTURE says that "forget" "removes the skill, its versions and its evidence from every store", while a private model trained on the user's data cannot have data removed from its weights; disabling it until it is retrained keeps forget true. The entry of 2026-10-20 makes the scout "A weekly routine" that comes "after the main product is built and shipped", and keeps anticipation "off by default, because it spends plan limits"; a scout that runs all the time keeps both only in its own capped lane.
- Consequences: Nothing changes in v0.1's build, its contracts or any acceptance list. The reservations touch the pinned `jobs`, `core_io` and `speech` modules, which change only through a code amendment by the package that owns their producer when it builds the first such connector; until then the prose in BUILD.md section 2.14 is the plan. The provisional `connectors`, `config` and `scenario` modules may take their fields when WP2 and WP7 pin them. ROADMAP's non-goal on training is narrowed to v0.1.
- Source: BUILD.md sections 2.5, 2.8, 2.12, 2.14, 3.3.2, 3.3.3, 3.3.7, 3.3.12 and 3.3.14; [OWNER-DIRECTION.md](OWNER-DIRECTION.md#2026-10-20), the entries of 2026-10-20 (connectors, the frontier scout, owner answer 20) and 2026-10-21 (speaker recognition); D11, D37 and D76; ROADMAP's non-goals; ARCHITECTURE's learning invariant.

## D80. Four build-time zstd advisories are ignored until zip takes zstd 0.14

- Date: 2026-10-26
- Status: accepted (WP10, from `wp10/deny-zstd-advisories`; the owner's standing rule applies)
- Amends: D78 (its "and nothing else": `deny.toml` also ignores the four advisories below, against crates on the same build-time path, and bans those crates anywhere else)
- Decision: `deny.toml` also ignores RUSTSEC-2026-0391 (against `zstd` 0.13.3), RUSTSEC-2026-0398 and RUSTSEC-2026-0403 (against `zstd-safe` 7.3.0) and RUSTSEC-2026-0410 (against `zstd-sys` 2.1.1+zstd.1.5.7), each entry with its reason naming this record, and nothing else beyond D78's three. The three crates reach the lock only through `zip` 2.6.1, the build dependency of `sherpa-onnx-sys` 1.13.8 that D78 covers. Its `bans` section allows `zstd` below 0.14.3 only as a direct dependency of `zip`, `zstd-safe` only of `zstd` and `zstd-sys` only of `zstd-safe`, so the ignores cannot cover these crates arriving any other way. The `zstd` ban stops at 0.14.3, the release RUSTSEC-2026-0391 lists as patched, because section 4.1 pins `zstd` 0.14.3 for Eileen's own store and blob modules. The four ignores come out in the pull request whose lock holds `zstd` 0.14.3, `zstd-safe` 8.2.0 and `zstd-sys` 2.1.2 or later on this path. That needs a `zip` that takes `zstd` 0.14 (9.0.2 or later, by the crates.io index), which `sherpa-onnx-sys` 1.13.8's `zip = "2"` excludes and the pin move D78 names allows (`sherpa-onnx-sys` 2.0.0 takes `zip = "9"`). RUSTSEC-2026-0410 alone can come out sooner: `zstd-sys` 2.1.2 is semver-compatible with `zstd-safe` 7.3.0, so section 4.1's security-patch rule admits a lock update to it.
- Why: The four advisories are dated 2026-10-16 and had their ids assigned in the RustSec database on 2026-10-26; from then `cargo deny --locked check` failed its advisories step on `main` and on every pull request. None reaches code an Eileen build runs. RUSTSEC-2026-0391 is in `zstd::stream::raw::Encoder::with_context` and `Decoder::with_context`, which `zip` never calls: it decodes with `zstd::Decoder::with_buffer` (`zstd::stream::read::Decoder`) and encodes with `zstd::stream::write::Encoder`. RUSTSEC-2026-0398 is in `zstd_safe::seekable`, behind the `seekable` feature, which nothing on the path enables. RUSTSEC-2026-0403 is in `DCtx::decompress_stream` when a legacy (v0.4 to v0.7) frame fails, reached through `zstd::stream::write::Decoder` and not through `read::Decoder`, `decode_all` or the bulk API; `zip` takes `zstd` with default features off and `zstd` takes `zstd-safe` with `std` only, so the `legacy` feature is off as well. RUSTSEC-2026-0410 affects wasm32 only, and the graph targets are x86_64 Windows and Linux. Around all four stands D78's reason: `sherpa-onnx-sys`'s build script calls `zip` only to read iOS `.xcframework.zip` archives, Linux never builds the crate, and a build dependency links into no binary. The patched `zstd` 0.14.3 and `zstd-safe` 8.2.0 are breaking releases that `zip` 2.6.1's `zstd = "0.13"` excludes, so no lock update reaches them before the pin moves; ignoring the four with bans this tight keeps every other vulnerability advisory failing the job.
- Consequences: `cargo deny --locked check` passes again on both targets, with D78's three ignores and these four. Any other vulnerability advisory, against these crates or any other, still fails the job, and so does a `zstd` below 0.14.3, a `zstd-safe` or a `zstd-sys` that arrives any other way. Ten informational `unsound` advisories from the same batch also cover `zstd-safe` 7.3.0 (RUSTSEC-2026-0395 to 0397, 0399 to 0402 and 0404 to 0406). They are not errors under the current `[advisories]` settings, which set no `unsound` scope, so cargo-deny's default reports unsound advisories only for workspace crates; they need no ignore and go away with `zstd-safe` 8.2.0, and whether `unsound` should cover dependencies too is the owner's choice. BUILD.md section 4b.3 names the seven ignores and the playbook's dependency-policy paragraph counts them; D78 (whose Status only notes this amendment), WP0's record and WP0 acceptance 3 describe the state of 2026-10-25 and stay as written. The pull request that moves the `sherpa-onnx` pin removes these four ignores and their bans with D78's once its lock holds the patched releases.
- Source: the four advisories in the RustSec database (read 2026-10-26); `Cargo.toml`, `src/compression.rs` and `src/write.rs` of `zip` 2.6.1; `Cargo.toml`, `src/lib.rs` and `src/stream/mod.rs` of `zstd` 0.13.3; `Cargo.toml` of `zstd-safe` 7.3.0; `build.rs` of `sherpa-onnx-sys` 1.13.8; the crates.io index entries of `zip`, `zstd`, `zstd-safe`, `zstd-sys` and `sherpa-onnx-sys` (read 2026-10-26); D78; BUILD.md sections 4.1 and 4b.3 and its revision log entry of 2026-10-26 from `wp10/deny-zstd-advisories`.
