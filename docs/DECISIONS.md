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
- Status: accepted
- Decision: Eileen is an Agent Client Protocol client, and every coding harness is reached through its ACP adapter.
- Why: Adapters exist for Claude Code (claude-agent-acp), Codex (codex-acp), Gemini CLI, OpenCode, Cursor and Droid. Goose, OpenClaw and OpenHands already consume them. One protocol gives one job model and one event mapping for every harness.
- Consequences:
  - v0.1 ships claude-agent-acp and codex-acp; Gemini CLI and OpenCode follow.
  - Harness hooks publish TaskStarted, Progressed, Blocked, ApprovalRequested and Completed to the event bus.
  - Parallelism comes from many sessions or cloud tasks, since Claude Code agent teams do not spawn in headless mode.
- Source: [section 6](RESEARCH.md#6-orchestration-and-routing)

## D4. Never handle vendor credentials

- Date: 2026-09-19
- Status: accepted
- Decision: Eileen spawns the unmodified official harness binaries in the user's own environment, never touches vendor credentials, and treats API keys as first-class.
- Why: Anthropic prohibits raw OAuth token use by third-party wrappers and enforced that three times in 2026. Driving the unmodified `claude` binary through the Agent SDK or `claude -p` is permitted today. The stance changed five times this year, so users need a switch that disables subscription-backed workers. OpenAI has no written rule, which is permission by silence, not by grant.
- Consequences:
  - No OAuth flows, no reading `~/.claude` or `~/.codex/auth.json`, no token proxying or pooling.
  - API keys, Bedrock, Vertex, Foundry and `CODEX_API_KEY` work from day one.
  - Workers are called "Claude Agent" or "Codex", never "Claude Code <anything>".
- Source: [section 3](RESEARCH.md#3-licensing-and-subscriptions-settled)

## D5. Cua Driver is the Windows action layer

- Date: 2026-09-19
- Status: accepted
- Decision: Cua Driver (trycua/cua) is the Windows action layer.
- Why: It is MIT, a Rust host daemon with Python and TypeScript SDKs and an MCP server with 57 tools. It runs UI Automation first with pixel fallback, and its synthetic cursor leaves the user's pointer alone. Commits land daily, and OpenClaw and Hermes both build their computer use on it.
- Consequences:
  - We do not write our own input injection or UIA action layer.
  - Chromium and Electron targets need foreground delivery, so the escalation ladder must handle that case.
  - Recording and replay come with the driver.
- Source: [section 1](RESEARCH.md#1-the-short-version), [section 2.2](RESEARCH.md#22-windows-action-and-grounding-components)

## D6. Our own DesktopState grounding service

- Date: 2026-09-19
- Status: accepted
- Decision: We write DesktopState, a small Windows service on Win32, DWM and UI Automation events, exposed over MCP.
- Why: Nothing exposes focused element, current selection, monitor model and focus history as one state object. Settings' "Identify" monitor numbers are not available from any public API, so we need our own ordinals. Everything required is public, stable Win32. Reference code: terminator, Windows-MCP, mcp-windows and UFO, all MIT.
- Consequences:
  - It is build item 1 of v0.1: `desktop.state`, the `window.*` verbs and a focus event stream.
  - Per-monitor-V2 DPI aware, every rect in physical pixels, UIA calls wrapped in timeouts.
  - Selection comes from TextPattern with a clipboard-sequence-number fallback.
- Source: [section 2.2](RESEARCH.md#22-windows-action-and-grounding-components), [section 9](RESEARCH.md#9-build-order-for-v01)

## D7. Playwright MCP with the browser extension

- Date: 2026-09-19
- Status: accepted
- Decision: Playwright MCP with the Playwright browser extension is the browser layer; Chrome DevTools MCP is the Chrome-only alternative.
- Why: It is the sanctioned way to drive real logged-in Chrome and Edge tabs, and it is Apache-2.0. Since Chrome 136, raw CDP against the default profile is refused unless the user opts in. Chrome DevTools MCP needs Chrome 144+ and does not support Edge.
- Consequences:
  - DOM-level grounding comes from the extension path; UI Automation is reserved for browser chrome.
  - Selection is a small JS snippet plus Ctrl+C, since no tool exposes it as a field.
  - Node 24 is a runtime dependency.
- Source: [section 2.3](RESEARCH.md#23-browser)

## D8. Deterministic first, vision last

- Date: 2026-09-19
- Status: accepted
- Decision: Actions climb a fixed ladder: App Actions, COM and app scripting, UI Automation, browser DOM, MCP servers, and only then screenshots and mouse.
- Why: Deterministic hooks are fast, testable and usually reversible. Multi-step vision runs take tens of minutes and the best open Windows result is 56.6 percent. Vision stays as the last rung, through Anthropic's computer toolset, for cases where no interface exists.
- Consequences:
  - Office COM, Explorer Shell, SMTC, Resolve and Vegas adapters are first-class modules under `apps/`.
  - The vision loop is Cua Driver's screenshot-plus-input path, not our own.
  - Later: register with the Windows On-Device Registry and prefer App Actions when an app publishes them.
- Source: [section 2.4](RESEARCH.md#24-app-hooks-that-beat-clicking), [section 2.5](RESEARCH.md#25-vision-fallback)

## D9. Python core, native sidecar, MCP between processes

- Date: 2026-09-19
- Status: accepted
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
- Status: accepted
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
- Status: accepted
- Decision: A YAML policy maps task kind to harness and model; a spoken override wins over it immediately.
- Why: Policies are defaults, not prisons. LiteLLM (MIT) supports per-request overrides for direct model calls. The conversational model parses "send this straight to Claude", "keep this local" or "have all three debate it" into a one-off override.
- Consequences:
  - The router in `core/` applies policy, then override, and records which one decided.
  - "Debate" fans one task out to several harnesses and collects the results.
- Source: [section 6](RESEARCH.md#6-orchestration-and-routing)

## D14. Permission tiers

- Date: 2026-09-19
- Status: accepted
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
