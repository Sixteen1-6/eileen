# Eileen

An open-source, voice-first AI operating layer for Windows 11: it sees your desktop, acts on it, and runs long jobs through coding agents while you keep talking.

## What it is

You leave it running and talk to it. It knows which monitor you are looking at, which window has focus, what you have selected and which browser tab is open. Window moves and copies happen at once. Coding, research and simulation work goes to an agent as a background job. That job reports back when you pause, or to your phone if you have walked away. Things you can say:

- "Put the terminal on the left monitor and the browser on the right."
- "Copy the second paragraph on this page into the email I am drafting."
- "Have Codex fix the failing tests, and tell me when it is done."
- "How's that simulation doing?"

A chatbot with a microphone cannot see your screen. Alexa cannot move your windows or run your tests. A computer-use agent can, but it goes quiet while it works and you wait. Eileen keeps the conversation and the work on separate loops. The voice loop owns the microphone and never blocks. Every agent session is a job you can watch, interrupt or reroute by voice, and you can send the same task to three agents and have them debate it.

## Why now

Nobody has built this. Voice loops, Windows UI Automation drivers, browser control, coding-agent protocols and phone bridges all exist, and some are very good. No open-source project combines them. The only shipped example of the always-on voice experience is OpenAI's closed GPT-Live and Codex integration in the ChatGPT desktop app. The pieces are now available under MIT, Apache-2.0 and BSD licenses, so the work is composition rather than invention. The full survey, with sources, is in [docs/RESEARCH.md](docs/RESEARCH.md).

## Principles

1. **You are the supreme router.** Automatic routing exists, but a spoken override ("no, send this straight to Claude", "keep this local") wins immediately. Policies are defaults, not prisons.
2. **Conversation never blocks on work.** The voice loop runs outside every agent, owns the microphone, and treats each agent session as an async job.
3. **Deterministic tools before vision.** App Actions, COM and app scripting, UI Automation, browser DOM and MCP servers come first; screenshots and mouse only when nothing better exists.
4. **Permission tiers.** Reversible actions (move a window, open a site, run tests, draft an email) run at once; consequential ones (send, delete, spend money) confirm first.
5. **Wrap existing harnesses, never rebuild them.** Eileen never handles vendor credentials; you sign in to the unmodified official binaries yourself, and API keys are first-class.
6. **Compose, do not fork.** Secrets never enter model-visible text, data is encrypted at rest, and sandboxing is the default.

## Architecture at a glance

```
                   You (speak, hear, or message from your phone)
                                         |
                                         v
+---------------------------------------------------------------------------------+
|  Voice loop (always on)                                                         |
|  Owns the microphone. Never blocks on work. Every agent session is a job.       |
+---------------------------------------------------------------------------------+
                                         |
                                         v
+---------------------------------------------------------------------------------+
|  Coordinator                                                                    |
|  Job registry, event bus, router, policy, permission tiers                      |
+---------------------------------------------------------------------------------+
         |                    |                    |                    |
         v                    v                    v                    v
+------------------+ +------------------+ +------------------+ +------------------+
| DesktopState     | | Action layer     | | Harness adapters | | Channels         |
| grounding        | | Cua Driver       | | over ACP         | | Telegram         |
| service          | | Playwright MCP   | | Claude Agent     | | ntfy             |
| Win32, DWM,      | | COM / app hooks  | | Codex, Gemini    | |                  |
| UI Automation    | |                  | | CLI, OpenCode    | |                  |
+------------------+ +------------------+ +------------------+ +------------------+
```

Jobs run async. The conversation continues while they work. Agent hooks publish started, progressed, blocked, approval-requested and completed events to the bus, and the voice loop and phone bridge subscribe. Approval requests interrupt you, completions wait for a pause, progress stays silent unless asked, and everything routes to your phone when you are away.

## What we build on

| Layer | Project | License | Why |
|---|---|---|---|
| Windows action layer | Cua Driver (trycua/cua) | MIT | Rust daemon with Python and TypeScript SDKs and an MCP server; UI Automation first, pixel fallback, synthetic cursor |
| Desktop grounding | DesktopState, our own service on Win32, DWM and UI Automation events | MIT (ours) | Nothing exposes focused element, selection, monitors and focus history as one state object; reference code in terminator, Windows-MCP, mcp-windows and UFO |
| Browser | Playwright MCP with its browser extension | Apache-2.0 | Drives your real logged-in Chrome or Edge tabs; Chrome DevTools MCP is the Chrome-only alternative |
| Voice | Pipecat, or a custom asyncio loop on RealtimeSTT and RealtimeTTS; Silero VAD, Smart Turn v3, sherpa-onnx or Moonshine v2, Kokoro | BSD-2, MIT, Apache-2.0 | Fully local, runs on Windows, and its async tool calls map cleanly onto jobs |
| Conversational model | Claude Haiku 4.5 to start; a local 4B to 8B model as the free option | API terms / open weights | Fast enough to hold a conversation while agents work |
| Harnesses | Agent Client Protocol client with claude-agent-acp and codex-acp; Gemini CLI and OpenCode later | Vendor terms apply | One protocol for every coding agent; unmodified vendor binaries, no credential handling |
| Routing | YAML policy plus LiteLLM | MIT | Configurable defaults with a per-request override |
| Phone | Telegram bot, ntfy, Tailscale; Discord for live voice | Free | Zero cost, works behind home NAT, voice notes both ways |
| Off-PC work | Claude Code cloud sessions and Codex cloud | Included in plans | Coding jobs survive the PC being off with no infrastructure to run |
| Later, optional | OpenClaw Gateway with Eileen as a node; Windows On-Device Registry; App Actions | MIT / Windows | Channels, cron and mobile apps at scale; native connector registration |

The core is Python 3.12+ (package `eileen`, src layout, pydantic models, pytest, ruff). The DesktopState sidecar will be Rust or C#, decided by its first implementer, and talks to the core over MCP. Alternatives considered and the reasons for each pick are in [docs/RESEARCH.md](docs/RESEARCH.md).

## Status

| Item | State |
|---|---|
| Research | Done, 2026-09-19 |
| Repo scaffold | In place |
| v0.1 nucleus | In progress |

Milestones and build order are in [docs/ROADMAP.md](docs/ROADMAP.md).

## The v0.1 milestone

The smallest useful version does five things. You talk to it. It knows what is on your Windows desktop: monitors, windows, the focused element, your selection. It moves and arranges windows and apps. It hands a coding task to one or more harnesses. And you can interrupt or reroute those jobs while they run. We build it in that order: the DesktopState grounding service over MCP; then the action layer (Cua Driver, Playwright MCP, COM adapters for Office and Explorer, SMTC for media); then the voice loop with the jobs block and DesktopState block in context; then the ACP harness adapters with the job registry, event bus, spoken overrides and permission tiers; then Telegram and ntfy with presence-aware announcements. Cloud sessions, the OpenClaw gateway, Vegas and Resolve extensions, ODR registration and a HUD overlay come only after that.

## Get involved

The repo is private for now and shared with friends; it goes public later. Read [CONTRIBUTING.md](CONTRIBUTING.md) for how we work and [docs/DEV-SETUP.md](docs/DEV-SETUP.md) to get a machine ready. Tasks are tracked as GitHub issues labeled by milestone, so pick one from v0.1 and say so on the issue. Maintainer: Shubhanshu Pokharel ([Sixteen1-6](https://github.com/Sixteen1-6)).

## A note on the name

The project is named Eileen. The owner chose the name on 2026-10-11, and it is final. Until then the project ran under the working name Eragon, which was never going to ship: eragon.ai is a funded startup in the same category, and the Paolini and Disney franchise owns the word in commerce. The history is in [docs/NAMING.md](docs/NAMING.md) and decision D16 in [docs/DECISIONS.md](docs/DECISIONS.md).

## Docs

- [docs/RESEARCH.md](docs/RESEARCH.md): what exists, what to build on, licenses, prices, sources
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): components, interfaces and data flow
- [docs/ROADMAP.md](docs/ROADMAP.md): milestones and build order
- [docs/DECISIONS.md](docs/DECISIONS.md): decision records
- [docs/NAMING.md](docs/NAMING.md): the name and how it was chosen
- [docs/DEV-SETUP.md](docs/DEV-SETUP.md): getting a Windows machine ready to work on it

## License

MIT.
