# Prior art and novelty

Research pass, 2026-10-11. Question: has anyone already shipped what Eileen is? That is, an always-on voice layer for the whole Windows PC, grounded in desktop state, that drives any app, and that dispatches parallel coding and computer-use jobs to the user's own subscription harnesses while the conversation continues.

This re-checks every row of [RESEARCH.md section 2.1](../RESEARCH.md#21-closest-projects) (written 2026-09-19) and sweeps for new entrants. It is written for contributors who need to know what exists, what we must match, and where Eileen can be clearly better.

## How to read this

Every claim carries a tag saying how it was checked:

- **[ran]**: run or measured on the owner's PC (Windows 11 Home 25H2, build 26200.9457, Core Ultra 7 155H, 16 GB RAM) on 2026-10-11.
- **[primary]**: read from the vendor's own docs, blog, release notes, repository README or the GitHub API.
- **[secondary]**: press, reviews or third-party write-ups. Treat dates and plan details as soft.
- **[inferred]**: our reasoning from the above.

GitHub star counts, licenses, last-push dates and latest releases were pulled from the GitHub API on 2026-10-11 [primary]. Stars move daily; use them as orders of magnitude.

What could not be checked: X and Reddit are not directly searchable from this environment, so social coverage came through web search and Hacker News (Algolia API). Several vendor pages (openai.com, perplexity.ai blog, techrepublic) returned HTTP 403 to automated fetches; for those, coverage of the same announcement was used and the claim is tagged secondary.

## 1. Verdict

No one ships the whole thing. As of 2026-10-11 there is no product, open or closed, that combines all five of: an always-on microphone on Windows with no wake word required, a grounded model of the desktop (focus, selection, monitors), background control of arbitrary Windows apps, parallel jobs on the user's own vendor harnesses across more than one vendor, and a conversation that keeps going while those jobs run.

But the gap is narrower than RESEARCH.md said three weeks ago, and the README sentence "Nobody has built this" now overstates it. Each half is shipped by someone:

1. **The closed, single-vendor version exists on Windows.** The ChatGPT desktop app has GPT-Live voice that can direct several Codex and ChatGPT Work tasks at once while you talk, plus Computer Use on Windows ([OpenAI, 2026-07-23](https://community.openai.com/t/chatgpt-voice-is-now-in-the-desktop-app/1388031)) [primary]. It is OpenAI-only, session-based (you start and stop voice chat, one at a time), its screen context is macOS-only, and on Windows its computer use takes over the foreground ([learn.chatgpt.com](https://learn.chatgpt.com/docs/computer-use)) [primary]. The Claude desktop app is converging on the same shape for Claude only.
2. **"Voice plus parallel subscription harnesses" is now open source and common.** Paseo (20k stars, Apache-2.0, Windows installer) runs Claude Code, Codex, Copilot, OpenCode, Pi, Antigravity and Muse Code in parallel worktrees, with a local-first voice mode whose brain is a hidden session on the user's own installed harness [primary]. Happy, Happier and Alibaba's qwen-audio-agent do variations of this. None of them control the desktop.
3. **"Voice plus Windows computer use" is open source too.** Hermes Agent (252k stars, MIT) ships a Windows desktop app with a "hey hermes" wake word, continuous voice with barge-in, and background UI Automation control through cua-driver [primary]. It is its own harness rather than a router to vendor harnesses, it handles vendor OAuth itself, and its wake flow takes one utterance at a time.
4. **The closest single design is on macOS.** Jarhead (MIT, 26 stars, created 2026-09-11) is a voice-first Mac assistant with GPT-Live voice, Codex and Claude Code as interchangeable brains, Accessibility-first control of the Mac and up to three parallel threads [primary]. It bills voice per minute through the OpenAI API and does not run on Windows.

So Eileen's novelty is the integration on Windows, done to a higher standard than any half: always-on local speech at near-zero idle cost, a desktop state object no competitor exposes, background-first deterministic control with real adapters for pro apps (Resolve, Photoshop, Premiere, Office), and route-named workers on five vendors' unmodified harnesses with strict credential hygiene. That is a composition moat, not a technology moat. Paseo or qwen-audio-agent could bolt on cua-driver in weeks. Eileen has to win on quality, latency, grounding and trust, and say so honestly.

## 2. What changed since 2026-09-19

Rows from RESEARCH.md 2.1, re-checked.

| Project | 2026-09-19 said | 2026-10-11 finding | Tag |
|---|---|---|---|
| OpenClaw | 390k stars; voice wake and Talk mode macOS/iOS/Android only; gateway recommended on WSL2; Windows computer use experimental | 391,668 stars, release v2026.10.1 on 2026-10-10. Native Windows gateway is documented and supported (WSL2 described as the most Linux-compatible option). The Windows Hub proposal to add Talk Mode (issue #1124) was closed as **not planned** on 2026-09-26; Windows keeps push-to-talk and a one-shot wake flow. Windows computer use is still labeled experimental (cua SDK in-process, primary display only, no held keys). Windows node runs `system.run` inside Microsoft Execution Containers when available. Microsoft announced a "native OpenClaw gateway" for Windows mini desktops on 2026-10-07. | primary |
| Hermes Agent | 247k stars; push-to-talk voice; UIA computer use in background | 252,680 stars, v0.21.6 on 2026-10-08 (roll-up of ~2,100 PRs; four dashboard auth fixes reported by Tenable). Voice now has a local wake word (openWakeWord "hey hermes" on Windows x64), continuous mode, barge-in, streaming STT, and an optional GPT-Live mode billed by OpenAI at $0.05 per minute including idle time. Computer use ships by default on Windows via cua-driver (UIA, `SendInput` plus `PostMessage`, no focus steal). | primary |
| Open Interpreter | Codex fork for open models; 01 dormant | Unchanged. Repo renamed to `openinterpreter/openinterpreter` ("a coding agent for open models like Kimi K3 and GLM 5.3", Rust, Apache-2.0, 68.5k stars). `OpenInterpreter/01` (AGPL-3.0) last pushed 2024-11-01. | primary |
| Leon, Khoj, Agent Zero | ideas only / avoid / borrow containment | Leon 17.6k active; Khoj AGPL-3.0, last push 2026-08-02; Agent Zero active (GitHub no longer auto-detects its license; the file still begins "MIT License", so check before borrowing). | primary |
| NanoClaw, ZeroClaw, IronClaw, Nanobot, QwenPaw, OpenFang | reference designs; OpenFang stale | All active except OpenFang (last push 2026-07-02). Nanobot 49k, QwenPaw 35.6k, ZeroClaw 33k, NanoClaw 30.9k, IronClaw 12.6k. | primary |
| Microsoft UFO / UFO3 | design reference | v3.0.10 on 2026-09-22. Galaxy multi-device DAG orchestration over UFO2 Windows agents. No voice. API keys. | primary |
| Perplexity Computer, Claude Cowork, Microsoft Scout, ChatGPT agent | competitors; ChatGPT agent removed Aug 2026 | ChatGPT agent was superseded by **ChatGPT Work** (2026-07-09) inside a merged ChatGPT desktop app that now contains Codex; Atlas browser targeted for deprecation on 2026-08-09. **Claude Cowork was folded into ordinary Claude chat** on 2026-09-16 (Pro and Max first). Perplexity shipped **Personal Computer for Windows** (2026-07-28) and **Portable Computer** for Windows (2026-09-14, needs an NVIDIA RTX GPU with 24 GB VRAM). Scout is still a Frontier private preview. | secondary (primary where linked below) |

New since 2026-09-19 or missed then, and material to the novelty question: Paseo, qwen-audio-agent, Happy and Happy Agent, Happier, Jarhead, Skales, VoiceOS, Fluent, Ski, the Gemini desktop app for Windows (2026-09-10), Meta Muse for Windows (announced 2026-10-07), Microsoft's local Copilot actions (announced 2026-10-07), DeepSeek Harness, and Grok Build.

## 3. The landscape

Columns used throughout: **Win** (runs natively on Windows), **Voice** (how listening works), **Subs** (uses the user's existing vendor subscriptions through that vendor's own harness), **Parallel** (several jobs at once while you keep talking), **Any app** (can operate arbitrary Windows applications, and how).

### 3.1 Vendor desktop products (all closed)

| Product | What it does | Win | Voice | Subs | Parallel | Any app | Activity |
|---|---|---|---|---|---|---|---|
| ChatGPT desktop app (ChatGPT, Work, Codex) | One app with chat, the Work agent and Codex. GPT-Live voice "can speak, listen, and coordinate work in the app at the same time"; it directs multiple Work or Codex agents, answers their input requests, and is reachable from iOS. | Yes | Session based. You start and stop a voice chat (hotkey available); one active voice chat across the app; no wake word documented. | OpenAI only (it is the vendor) | Yes, several Codex and Work tasks | Yes, Computer Use plugin. On Windows it "runs on the active desktop" and takes over the foreground; OpenAI suggests a VM or second device. Not available in the EEA, UK or Switzerland at Windows launch. Screen context ("appshots") is macOS-only. | Voice on desktop 2026-07-23; Windows computer use in Codex app 26.527 (2026-05-29) |
| Claude desktop app (chat with former Cowork capabilities, Claude Code desktop, Claude in Chrome, Dispatch) | Claude decides how much work a request needs and runs it, including long tasks that continue after the laptop closes. Computer use (beta, Pro and Max) clicks and types in your apps. Claude in Chrome is GA for paid plans. Dispatch steers the desktop from the phone. | Yes. The former Cowork VM needs full Hyper-V, absent on Windows Home ([ran]: no `vmms` service on this PC) | Voice mode is hands-free by default and works "in any conversation in the new Claude experience, including ones where Claude is working on a task". In Claude Code only dictation exists. A request to add voice mode to Cowork and Dispatch (issue #100893, 2026-10-09) was auto-labeled duplicate. | Claude only | Yes, several Claude Code sessions and background tasks | Beta. macOS 15 gets background windows; Anthropic does not document the same for Windows, so treat it as active-desktop control. Per-app approval. | Windows computer use announced 2026-04-02; beta update 2026-09-02; Cowork merged 2026-09-16 |
| Microsoft Copilot on Windows | "Hey Copilot" wake word (opt-in, PC unlocked), Copilot Vision screen sharing, Copilot Actions in an isolated agent workspace under a separate agent account. | Yes | Wake word plus voice session ending on "goodbye" | No harness dispatch | No | Copilot Actions is still an experimental, admin-enabled Insider feature (support article last updated 2025-12-05). Local context, local actions and local models were announced 2026-10-07 for **Copilot+ PCs only**, "in the coming months". Ask Copilot on the taskbar was paused on 2026-10-07. | Active, mostly preview |
| Gemini app for Windows | Alt+Space overlay over the active window, Google app access, hand-off to Gemini Spark (a cloud agent on Google VMs). | Yes (2026-09-10) | Not mentioned for Windows | Google only | Spark tasks run in the cloud | No local control on Windows. Project Mariner was shut down 2026-05-04. | New |
| Perplexity Personal Computer and Portable Computer | "Local agent harness" orchestrating agents over local files, 400+ connectors and the web; Portable runs model, orchestrator and scheduler on device. | Yes. Portable needs an RTX GPU with 24 GB VRAM. | Local dictation (Nemotron ASR) | Perplexity credits and plans, 15+ cloud models behind its own billing | Yes (scheduler, durable queue) | Approved folders, connectors, local MCP servers. Not general GUI control. | Windows 2026-07-28 and 2026-09-14 |
| Manus Desktop "My Computer" (Meta) | Runs Manus's agent locally through a CLI in your terminal; launches and controls local apps with per-command approval. | Yes | Not documented | Manus plans | Not documented | Terminal-driven | Launched 2026-03 |
| Meta Muse | Personal agent; Mac app acts in native macOS apps (2026-09-17). | Announced "coming soon" as a native app with MXC (2026-10-07) | Unknown | Meta | Unknown | Unknown on Windows | Pre-release |
| Microsoft Scout | Always-on work agent built on OpenClaw; desktop app reads and writes files, runs shell commands, drives a browser with Playwright. | Yes | None documented | Microsoft 365 | Background work | Browser and shell, not general GUI | Frontier private preview |
| Grok Bot (xAI) | Named agents with their own cloud computer. | Windows client | Not documented | SuperGrok | Yes, in the cloud | No local control | Beta 2026-08 |

Tags for this table. ChatGPT: voice, Work, Codex and Windows foreground behavior are primary (OpenAI community announcement, learn.chatgpt.com); the Codex 26.527 date and the EEA, UK and Switzerland exclusion are secondary. Claude: voice-mode behavior and the 2026-04-02 Windows announcement are primary (Help Center, Anthropic on X); the Cowork merge, the September beta, Windows background behavior and Claude in Chrome GA are secondary; the missing Hyper-V service is ran. Copilot: the 2026-10-07 announcements and the agentic-features status are primary (Windows blog, Microsoft Support); "Hey Copilot" GA and the Ask Copilot pause are secondary. Gemini: primary (Google blog); Mariner and Spark details secondary. Perplexity, Manus, Scout and Grok Bot: secondary (Perplexity's blog returned 403). Muse for Windows: primary (Windows blog).

Takeaway: every vendor is building its own always-available agent, each locked to its own models, and none of them listens continuously on Windows or knows what you have selected without a screenshot. The only Windows product that already lets you talk while parallel agents work is ChatGPT, for OpenAI agents only.

### 3.2 Open-source personal agents

| Project | What it does | Win | Voice | Subs | Parallel | Any app | License, activity |
|---|---|---|---|---|---|---|---|
| Hermes Agent (Nous Research) | Self-improving agent with desktop app, TUI, cron, subagents, messaging gateways, skills. | Yes; desktop app also in the Microsoft Store | Wake word ("hey hermes", openWakeWord on Windows x64) opens a session that records **one utterance**, replies, then returns to wake listening. Continuous mode restarts recording after each reply and stops after three silent recordings. Barge-in supported. GPT-Live mode is $0.05 per minute. | Partly. Its OpenAI Codex provider runs its own ChatGPT device-code OAuth, stores tokens in `~/.hermes/auth.json` and "can import existing Codex CLI credentials from `~/.codex/auth.json`". Anthropic OAuth is supported with reported HTTP 400 "extra usage" errors. It does not drive the vendor harnesses. | Subagents, each with its own cua-driver cursor | Yes. cua-driver over MCP: UIA tree, background input, no focus steal. Cannot drive elevated windows (UIPI) or Session 0. | MIT, 252,680 stars, v0.21.6 2026-10-08 |
| OpenClaw | Gateway with 20+ channels, nodes, cron, sub-agents, ACP sessions, a Codex harness plugin, memory; Windows Hub companion app. | Yes (native gateway; Windows Hub v2026.9.9-alpha.4 signed with Azure Artifact Signing) | Push-to-talk and one-shot wake on Windows; continuous Talk Mode only on macOS, iOS, Android, and the Windows Talk Mode proposal was closed as not planned. | Yes, through ACP sessions and the Codex harness | Yes | Experimental on Windows via the cua SDK in-process; primary display only | MIT, 391,668 stars, v2026.10.1 2026-10-10 |
| Open Interpreter | Now a Codex-derived coding agent for open models. The 01 voice device is dormant. | Yes | 01 only, dormant | No | No | No | Apache-2.0 / 01 AGPL-3.0 |
| Leon, Khoj, Agent Zero, NanoClaw and siblings | Unchanged from RESEARCH.md. None combines voice, Windows computer use and harness routing. | Mixed | Mixed | No | Some | Agent Zero via a host connector | See section 2 |

Tags: Hermes and OpenClaw rows are primary (official docs, READMEs, release notes, GitHub API), except the Anthropic "extra usage" errors in Hermes, which come from a third-party write-up (secondary).

### 3.3 Voice plus coding-harness orchestrators (the half that is no longer novel)

| Project | What it does | Win | Voice | Subs | Parallel | Any app | License, activity |
|---|---|---|---|---|---|---|---|
| **Paseo** | One interface (desktop, mobile, web, CLI) over Claude Code, Codex, Copilot, OpenCode, Pi, Antigravity and Muse Code; a local daemon manages agents. | Yes, `Paseo-Setup-0.11.2-x64.exe` (2026-10-09) | Dictation and a voice mode. Local-first: Parakeet TDT 0.6B INT8 STT and Kokoro TTS on ONNX/CPU through sherpa-onnx, Silero VAD. The voice brain is "a hidden agent session using your configured provider (claude, codex, or opencode)", with an MCP bridge for voice tools that launch and control agents. | Yes, the installed and signed-in CLIs | Yes, one worktree per agent | No (coding agents and a built-in browser) | README says Apache-2.0 (GitHub detects "Other"; check the file); 20,471 stars |
| **qwen-audio-agent** (Alibaba Qwen team) | "A realtime voice runtime that keeps Agents talking, working, and present." Frontend conversation and background tasks run in parallel; results return to the conversation. | Yes, desktop floating orb on Windows, macOS, Linux | Full-duplex realtime voice with interruption; wake word. Frontends are mostly cloud realtime APIs with API keys (Qwen Audio, GPT-Live/OpenAI Realtime, Gemini Live, StepFun, Doubao); a local Hugging Face speech-to-speech option exists. | Yes. Backends over ACP, "reusing its model configuration, tools, MCP, Skills, and authentication": Qwen Code, OpenCode, OpenClaw, Kimi Code, Hermes, Codex and Claude Code (external ACP adapters), DeepSeek Harness | Yes, multiple asynchronous tasks with status tracking | Only through whatever the backend agent can do | Apache-2.0, 2,888 stars, v2.0.1 2026-09-26; tech report on arXiv |
| Happy (slopus) | Mobile, web and desktop app plus Happy Agent, a "universal agent harness". | Yes (happy-desktop) | Voice assistant in the mobile/web app that follows sessions, relays messages and answers permission prompts; hosted voice with 20 free minutes then a paid tier. | Partly. Claude through the Agent SDK. Codex is **reimplemented** inside Happy Agent rather than wrapping the CLI. For Grok it "reads Grok's scoped auth store on every request ... persists rotated refresh tokens". | Yes, subagents across vendors | No | MIT, 24,086 stars |
| Happier | Desktop, web and mobile client and orchestrator for Codex, Claude Code, OpenCode, Gemini CLI, Grok, Antigravity, Kimi and more. | Yes | Voice assistant that follows sessions and answers permission requests; ElevenLabs key or local pipeline. | Yes, "your existing subscriptions and coding agents" | Yes | No | MIT, 1,921 stars |
| Omnara (YC S25) | Was a Claude Code wrapper with mobile voice; repo now describes "the open-source managed agent platform". | Web and mobile | Two-way voice in the app | Mixed | Yes | No | Apache-2.0, 2,905 stars |
| VoiceOS (YC) | Dictation plus an Agent Mode that acts through connected services (Slack, Gmail, Notion, Linear, GitHub); beta Claude Code and Codex integrations in 0.1.21 (2026-08-08). | Yes (screen pointing is Mac-first) | Hotkey dictation and agent sessions | Beta integrations | Background tasks reported | Connectors, not GUI | Closed |
| Ski | Floating pill to talk to Claude Code, Cursor, Codex or Gemini CLI and hear answers; on-device STT and TTS; mute toggle; interrupt mid-sentence. | Yes | Open mic with mute | Uses the agent you run | Several repos bound at once | No | Closed, free |
| voicemode (mbailey), claude-call, backtalk, jarvis-line and dozens of 2026 hobby repos | Voice in and out for a single Claude Code or Codex session. | Mostly | Push-to-talk or converse tool | Yes, one harness | No | No | MIT or AGPL, small |

Tags: Paseo, qwen-audio-agent, Happy, Happier and Omnara rows are primary (READMEs, docs in the repos, code search, release assets, GitHub API); whether Paseo's voice mode stays open between turns is inferred from its VAD code, not tested. VoiceOS is secondary (review sites citing its changelog). Ski is primary (its site). The voicemode row is primary.

What this means for Eileen: M4 (harness adapters, parallel jobs, spoken status) is table stakes now, not a differentiator. Paseo in particular proves two design choices Eileen already planned: local Parakeet plus Kokoro plus Silero on CPU is good enough for coding voice, and the conversational brain can be a warm session on a subscription harness instead of an API key. That second point matters because the owner's new rule allows no API keys except DeepSeek (see section 8).

### 3.4 Windows computer-use components

| Project | Role | Status on 2026-10-11 | License |
|---|---|---|---|
| Cua Driver (trycua/cua) | Background computer-use driver for macOS, Windows, Linux. Windows: UIA with cache requests, MSAA fallback for VCL/SAL apps, `PostMessage` background input, refuses to steal foreground unless asked (`dispatch:"foreground"` uses `SendInput`), `PrintWindow` then Windows.Graphics.Capture for pixels, a separately painted agent cursor, a named-pipe daemon so Session 0 callers reach the desktop. Chromium, GTK, VCL accelerators and WPF drags may need foreground. | cua-driver-rs v0.34.1 (2026-10-09); 29,333 stars. Hermes ships it by default; OpenClaw uses its SDK. The repo now also holds Cua Spaces under FSL-1.1-MIT and an optional `cua-perception` extension that is AGPL. | Driver MIT; Spaces FSL; perception AGPL |
| CursorTouch/Windows-MCP | Python MCP server, UIA plus optional DOM mode; 0.2 to 0.5 s between actions; claims 2M+ users through Claude Desktop extensions. | v0.8.8 (2026-10-10), 8,645 stars | MIT |
| Microsoft UFO3 | UIA plus vision hybrid; Galaxy multi-device DAG orchestration. No voice. | v3.0.10 (2026-09-22) | MIT |
| Agent S3 / Sai (Simular) | Open framework; Agent S3 72.6 percent on OSWorld (best-of-N), 56.6 on Windows Agent Arena. Hosted Sai reports 73 percent on OSWorld 2.0 versus 62.57 for GPT-5.6 Sol (vendor-reported, 2026-08-28). Simular API launched 2026-10-01. | Framework release v0.3.2 is from 2025-12; README active | Apache-2.0 |
| mediar-ai/terminator | Rust selector-based UIA. | Last commit 2026-06-02, last release 2026-04-05. Idle. | MIT |
| sbroenne/mcp-windows | C# multi-monitor, DPI-aware window tools. | Active, 111 stars | MIT |
| pi-computer-use, QwenLM/open-computer-use, nuphus-mcp, DioxusLabs/accessibility-cli | 2026 accessibility-API computer-use servers for other agents. | Active | MIT / Apache-2.0 |

Tags: primary throughout (Cua's own Windows write-up of 2026-05-27, READMEs, GitHub API). Benchmark percentages are the vendors' own reports, not reproduced here.

Nothing here is a voice product. These are the parts Eileen composes or learns from (D5, D6).

### 3.5 Voice input tools

| Product | What it is | Relevance |
|---|---|---|
| Talon Voice | Free with a Patreon tier, macOS, Windows, Linux X11, under its own license agreement [primary site]; no public source for the engine [inferred]. Command grammar for hands-free computer use. No LLM features on its site; community `talon-ai-tools` adds LLM queries [primary repo]. | The gold standard for low-latency, deterministic voice commands. Eileen should match its "say it, it happens instantly" feel for simple verbs rather than routing every utterance through a model. |
| Wispr Flow | Closed dictation for Mac, Windows, iPhone, Android [primary]; Command Mode rewrites selected text (Pro) [secondary]. One review measured the Windows Electron app at about 800 MB RAM and 8 percent CPU idle [secondary]. | Sets the bar for dictation polish (filler removal, self-corrections). Its idle footprint is the kind of number Eileen should beat by an order of magnitude. |

### 3.6 Screen-context assistants

| Product | Finding |
|---|---|
| Highlight (Medal) | Closed Mac and Windows assistant that attaches the current screen and meeting transcripts to questions. Memory and recall, not control [secondary]. |
| Glass (pickle-com) | `github.com/pickle-com/glass` returns 404 on 2026-10-11 [ran, GitHub API]. Do not depend on it. |
| Clicky | Open-source macOS voice companion that points at the button to click; no official Windows build as of 2026-08-30 [secondary]. |
| Fluent | Closed Windows voice computer-use app that reads the accessibility tree "instead of sending screenshots to a vision model"; agent runs metered at $20, $50 or $99 per month [primary site]. |

### 3.7 2026 projects that try to combine everything

GitHub search (2026-10-11) for voice plus computer use plus harness routing turned up dozens of repos. The ones that matter:

| Project | Platform | What it combines | What it lacks | License, size |
|---|---|---|---|---|
| **Jarhead** | macOS only | Wake word behind Touch ID, GPT-Live-1 voice, brain is Codex (resident `codex app-server` thread on the ChatGPT login), Claude Code (headless Agent SDK), an API or a local model; 71 AX-first tools; up to three parallel threads; Console into every coding-agent session; append-only ledger; publishes its own measured latencies (first audible word 1.35 s median for typed input, 2.2 s to "looking" for a delegated spoken request, 2026-10-06). | Windows; voice is billed per minute through the OpenAI API (shows a dollar meter); one model family for voice. | MIT, 26 stars, created 2026-09-11 |
| **Skales** | Windows, macOS, Linux, mobile | Always-listening "Iris" voice with a wake word trained on your own voice and matched locally, barge-in; screenshot computer use; its own coding agent with parallel sub-agents; ChatGPT subscription sign-in inside the app; 26 BYOK providers. | Not open source (EULA); no vendor harnesses; screenshot-based control; in-app OAuth. | EULA, 1,956 stars |
| vierisid/jarvis | Daemon on server or WSL2, Go sidecars incl. Windows (Win32, UIA, SendKeys) | Always-on daemon, openWakeWord, screen capture every 5 to 10 s, 9 specialist agents. | API keys only; source-available; screenshot polling. | RSALv2, 670 stars |
| Orkas | Windows, macOS, Linux | Commander LLM directing sub-agents, plugs in Claude Code, Codex, OpenCode, OpenClaw and Hermes as local sessions. | No voice loop found; API keys. | MIT, 2,166 stars |
| open-cowork | Windows, macOS | Open re-implementation of Claude Cowork: wraps Claude Code and other models, WSL2 VM isolation, computer use. | No always-on voice. | MIT, 2,198 stars |
| Monaw | Windows | Desktop assistant with computer use, browser automation, ChatGPT sign-in through the Codex SDK. | No voice loop. | Apache-2.0, 81 stars |
| ctskool/agentic-os-starter-v2, samonti86/jarvis, seainml/voiceagent, ijoshi129/vision, AndyTrust/Jarvis-Windows, Ayushmaniar/jev-voice-computer-use | Windows or cross-platform | Voice plus Claude Code or Codex, sometimes a desktop tool layer. | Starter kits and solo projects, 0 to 24 stars. | Mixed |

Tags: all rows primary (READMEs and GitHub API). Jarhead's latency figures are its author's own measurements, not ours.

Hacker News 2026 stories in this space (Algolia API, 2026-10-11): Fluent (voice computer use for Windows, 2026-07-31), Ski (2026-07-30), dashvox (voice control of coding agents from a watch, 2026-06-01), WinClaw (2026-02-23), Omnara launch (147 points, 2026-02-12). None reached the front page with the full combination.

### 3.8 Harness routes the owner named

These are not competitors but they decide what "uses the subscriptions you already pay for" can mean in October 2026.

| Route | Native harness | Subscription sign-in | Notes | Tag |
|---|---|---|---|---|
| Claude | Claude Code (`claude`), Agent SDK, `claude-agent-acp` (v0.89.1, 2026-10-10) | Yes, by the user in the unmodified binary | Anthropic's support article 15036540 says third-party apps authenticated through the Agent SDK draw from plan limits. | primary |
| Codex | Codex CLI (Rust, Apache-2.0, rust-v0.162.1 2026-10-09), `codex app-server`, `codex-acp` | Yes, ChatGPT login | `codex-acp` moved to `agentclientprotocol/codex-acp`; the Zed-org repo was archived 2026-07-22. | primary |
| Gemini | **Antigravity CLI (`agy`)**, not Gemini CLI | Google AI Pro/Ultra via `agy` | Gemini CLI stopped serving personal Google logins on 2026-06-18. The `google-antigravity/antigravity-cli` repo has **no license file** (GitHub API reports none), release 1.3.3 on 2026-10-10. Community reports that `agy -p` prints nothing when stdout is not a terminal (issue #76); community ACP adapters work around it with ConPTY. | primary (repo), secondary (cutoff, bug) |
| Grok | Grok Build (`xai-org/grok-build`, Apache-2.0, 27k stars, created 2026-07-14) | `grok login` | Happy Agent drives it by reading Grok's auth store, which Eileen must not do. | primary |
| DeepSeek | DeepSeek Harness (`deepseek-ai/deepseek-harness`, MIT, 247k stars, created 2026-08-13, developer preview) plus the DeepSeek API key | API key (the one key the owner allows) | qwen-audio-agent lists "DeepSeek Harness native ACP". | primary |

## 4. Capability matrix

Y = yes, P = partial, N = no. "Voice keeps going" means you can keep talking to it while its jobs run.

| | Windows native | Mic always on, no wake word needed | Voice keeps going while jobs run | Desktop state (focus, selection, monitors) | Background control of any Windows app | Parallel jobs | Several vendors' own harnesses | Never touches vendor credentials | Open license |
|---|---|---|---|---|---|---|---|---|---|
| ChatGPT desktop | Y | N (session, hotkey) | Y | N on Windows (macOS appshots) | P (foreground takeover) | Y | N (OpenAI) | n/a (vendor) | N |
| Claude desktop | Y | N (hands-free session) | Y (per support doc) | N | P (beta; Windows background undocumented) | Y | N (Claude) | n/a | N |
| Copilot on Windows | Y | N (wake word) | n/a | P (Vision on request) | P (experimental agent workspace) | N | N | n/a | N |
| Perplexity Personal/Portable | Y | N (dictation) | P | N | P (folders, MCP) | Y | N (own billing) | n/a | N |
| Hermes Agent | Y | P (wake word, continuous mode) | not documented | N | Y (cua-driver) | Y | N (own harness) | N (own OAuth, imports Codex auth file) | Y |
| OpenClaw | Y | N on Windows (PTT, one-shot wake) | Y | N | P (experimental) | Y | Y (ACP, Codex harness) | P | Y |
| Paseo | Y | P (voice mode with VAD in the app) | Y | N | N | Y | Y (7 harnesses) | Y | Y |
| qwen-audio-agent | Y | P (wake word, full duplex) | Y | N | N | Y | Y (ACP backends) | Y | Y |
| Happy / Happy Agent | Y | P (app voice, hosted) | Y | N | N | Y | P (reimplements Codex) | N (reads Grok auth store) | Y |
| Jarhead | N (macOS) | P (wake word) | Y | P (AX focused text, screenshots) | Y on Mac | Y (3 threads) | P (Codex, Claude) | Y | Y |
| Skales | Y | Y (local wake word, always listening) | Y | N | P (screenshots) | Y | N (own agent, in-app ChatGPT OAuth) | N | N |
| UFO3 | Y | N | N | P | Y (UIA plus vision) | Y (DAG) | N (API keys) | Y | Y |
| **Eileen target** | Y | Y (mute toggle) | Y | Y | Y, background first, deterministic adapters first | Y | Y (Claude, Codex, Gemini via agy, Grok Build, DeepSeek Harness) | Y | Y (MIT) |

The matrix is a summary of sections 3.1 to 3.7 and inherits their tags; the single-letter grades are our judgment [inferred].

No row other than Eileen's target has a Y in every column. The closest open-source rows are Hermes (control, voice) and Paseo or qwen-audio-agent (voice, harnesses, parallel). The closest overall product is ChatGPT desktop, limited to one vendor and to the foreground on Windows.

## 5. The exact gap Eileen fills

On Windows, for one person, an open and auditable layer that:

1. **Listens whenever the mic is unmuted**, with local VAD and STT, and decides by itself whether speech is addressed to it, so there is no wake word and no per-minute bill. Every competitor either needs a wake word or a hotkey (Copilot, Hermes, ChatGPT, Claude, OpenClaw), bills voice by the minute (Hermes GPT-Live mode, Jarhead, Happy after 20 minutes), or sends audio to a cloud API by default (qwen-audio-agent).
2. **Knows the desktop without taking a screenshot**: foreground window and process, focused element, selected text, monitors with stable ordinals, focus history. No competitor exposes this as state. ChatGPT's appshots are macOS-only; Copilot Vision and Claude computer use read pixels on request.
3. **Acts in the background first and deterministically first**: app scripting and COM before UI Automation before pixels, with real adapters for the apps the owner uses (Resolve's scripting API, Photoshop COM and UXP, Premiere UXP, Office COM, Explorer, SMTC). ChatGPT on Windows takes over the foreground; Claude's Windows background behavior is undocumented; Hermes and OpenClaw have UIA through cua but no app-level adapters.
4. **Routes work to the vendor harness that is best at it, under the vendor's own name** ("Claude", "Codex", "Gemini", "Grok", "DeepSeek"), several at once, with spoken overrides, and never reads, refreshes or proxies a vendor credential. Paseo and Happier route across harnesses but cannot see or drive the desktop; Hermes and Happy handle credentials themselves.
5. **Keeps talking while the work runs**, and reports through a fixed announcement policy, locally or on the phone.
6. **Costs almost nothing while idle**, as a Rust runtime, on ordinary hardware: Windows Home, no NVIDIA GPU, no Copilot+ NPU. Microsoft's local Copilot actions need a Copilot+ PC (40+ NPU TOPS); the owner's Core Ultra 7 155H is below that bar [secondary, inferred]. Perplexity Portable needs a 24 GB RTX card. The former Cowork VM needs Hyper-V, which Windows Home lacks [ran].

## 6. Differentiators that would make Eileen clearly best in class

Each item names the ROADMAP milestone it lands in.

1. **Always-on, local, cheap listening (M3).** A Rust audio engine owning capture and playback, AEC, Silero VAD and Smart Turn on CPU, streaming STT through sherpa-onnx, Kokoro TTS. Publish idle CPU and RAM numbers on the owner's PC next to the competitors' (section 9 has the first baselines). Target: idle under 2 percent of one core and under 150 MB resident excluding model weights [inferred target, to be measured].
2. **Addressee detection that is measured, not hoped for (M3).** Jarhead publishes room-talk trials; Happy's voice prompt calls a `skip_turn` tool. Eileen should ship a scripted room-talk test (TV, second person) and publish the false-reply rate, which ROADMAP M3 already demands (9 of 10).
3. **DesktopState as a first-class, cheap context block (M1).** Resolve "this", "that window", "the selection", "screen two" from state, not pixels. Then extend it into an episodic desktop memory: a local ledger of focus changes, window titles and document paths gathered from the same event hooks, so "the image where I was editing the girl at the beach" can be answered from Photoshop's recent documents plus a local image index, without screen recording. No competitor does recall this cheaply on Windows (Microsoft Recall is Copilot+ only) [inferred].
4. **Background-first control with honest escalation (M2).** UIA patterns and `PostMessage` first, a visible agent cursor, explicit foreground escalation only for targets that need it (Chromium, WPF drags), and the user's own input always wins (Jarhead holds its hands for 1.5 s after any user input; copy that rule). This beats ChatGPT on Windows outright.
5. **Pro-app adapters that make the owner's examples real (M2, v0.2).** "Open Resolve, go to 15:30" is `SetCurrentTimecode` through the scripting API that is already proven on this PC (`smauto/resolve_render.py`), not clicking. "Make the girl on the right's face less blurry" is a creative operation no competitor demonstrates: locate the face (vision on one frame), then apply a correction through the most deterministic path available (a DRX grade preset through the scripting API where possible, UI steps with a vision check where not), and confirm before committing. Photoshop gets the same treatment through COM or UXP. Shipping two such end-to-end demos would separate Eileen from every generic computer-use agent [inferred].
6. **Route-named workers across five vendors on unmodified harnesses (M4).** Claude via `claude` or the Agent SDK, Codex via `codex app-server`, Gemini via `agy`, Grok via Grok Build, DeepSeek via DeepSeek Harness. Warm, resident sessions per route (Jarhead keeps a resident `codex app-server` thread) so dispatch does not pay process start-up. "Launch parallel agents for X" fans out across worktrees and routes, and the registry names jobs by route and topic.
7. **A credential boundary you can audit (M4).** No OAuth, no reading `~/.claude`, `~/.codex/auth.json`, Grok or Antigravity stores, no token refresh. Preflight each route with the vendor's own status command and tell the user to sign in themselves. This is a real difference from Hermes, Happy Agent and Skales, and it is what keeps Eileen inside Anthropic's written rules.
8. **Rust where it buys something measurable.** Model latency dominates any job that calls a frontier model, and Rust does not change that. Rust buys: start-up in tens of milliseconds instead of seconds (section 9: the npm `claude` shim takes about 1.4 s just to print its version, the native binary about 60 ms), steady-state RAM and CPU while always listening, hook-to-event latency for focus and selection, UIA and capture speed, and a single signed binary instead of an Electron app plus a Python runtime. Say exactly this in the README.
9. **Containment that matches the platform (M4, v0.2).** Run coding jobs in worktrees, and use Microsoft Execution Containers when available (GA on Windows 11 since 2026-10-07; Codex and OpenClaw support it, Claude Code support announced). Check whether MXC works on Windows Home before promising it.
10. **Published, reproducible evaluation (M6, v0.3).** A Windows task suite with pass rates and latencies, measured the way Jarhead reports its numbers. No Windows competitor publishes this.

## 7. Table stakes: what competitors have that Eileen must match

If v0.1 lacks these, it will look weaker than tools people already use.

| Capability | Who has it | Eileen milestone |
|---|---|---|
| Barge-in: "stop" cuts speech mid-sentence and stops the current action | ChatGPT, Hermes, Jarhead, Skales, qwen-audio-agent, Ski | M3 |
| Ignore room talk and other people | Jarhead, Happy (`skip_turn`), isair/jarvis | M3 |
| A visible status surface with mute, listening state and running jobs | Jarhead notch island, qwen orb, Skales Iris, Ski pill, OpenClaw tray | Move the v0.2 HUD into v0.1 as a minimal tray plus strip; the owner asked for a mute toggle |
| Per-app permission with "always allow" lists | ChatGPT computer use (`always_allowed_app_ids`), Claude per-app approval | M2 plus D14 |
| Watch any job live and answer its Allow/Deny | Jarhead Console, Paseo, Happy, Claude Code desktop | M4 |
| One worktree per parallel coding job, diff review | Paseo, vibe-kanban, Claude Code desktop | M4 |
| Steer from the phone | Claude Dispatch, Codex remote from iOS, Paseo and Happy mobile apps, OpenClaw apps | M5 (Telegram is weaker than a native app; make it end-to-end private and fast) |
| Usage visibility before a plan hits its limit | Hermes usage chip, Jarhead meter | M4 |
| Scheduled and recurring tasks | Hermes, OpenClaw, Perplexity, Skales, qwen reminders | v0.2 |
| Cross-session memory | Hermes, OpenClaw, qwen-audio-agent, Skales | v0.2 (start with the episodic desktop ledger) |
| Background computer use without stealing focus | Hermes (cua), Codex on macOS | M2 |
| Local models as an option | Hermes, Perplexity Portable, Jarhead | M3 (optional, never required) |
| Signed installer and auto-update | OpenClaw Windows Hub, Hermes (Microsoft Store), Paseo | M6 |
| Multilingual STT | Paseo (Parakeet v3, 25 languages), Hermes | M3 (config switch) |
| Crash recovery and an audit ledger | Jarhead, OpenClaw doctor | M4 |

## 8. Pitfalls found during this pass

- **"Subscriptions only" conflicts with parts of the current plan.** ROADMAP M3 uses Claude Haiku 4.5 through LiteLLM as the conversational model, and D4 makes API keys first-class. The owner now allows only a DeepSeek API key. Paseo shows the alternative: run the voice brain as a warm, resident session on a subscription harness. Latency needs measuring; a cold harness start is seconds (section 9).
- **Harness auth state is per binary and per environment.** This PC has two Claude Code binaries: the npm global `claude` 2.1.175 on PATH and the Claude desktop app's bundled `claude.exe` 2.1.295. Run from a plain shell, the first reported "Not logged in" and the second "OAuth session expired and could not be refreshed" after 19.7 s, while the desktop app's own sessions work [ran]. Eileen must discover which binary to use, preflight its auth with the vendor's status command, fail fast with "please sign in to Claude yourself", and never try to repair auth.
- **Gemini means Antigravity now**, and `agy` has no license file and a known non-TTY output bug on Windows. Treat it as experimental and spawn it under ConPTY.
- **Windows foreground limits are real.** ChatGPT and Codex take over the foreground on Windows; cua-driver needs foreground for some Chromium, GTK, VCL and WPF interactions; UIPI blocks driving elevated windows from a normal-integrity process; Session 0 (SSH, services) has no desktop.
- **License traps in the parts bin.** Cua Spaces is FSL and `cua-perception` is AGPL (the driver itself is MIT). Skales is an EULA, vierisid/jarvis is RSALv2, screenpipe is a commercial license, Antigravity CLI has none. GitHub shows "Other" for Paseo and Agent Zero although their files read Apache-2.0 and MIT; read the files before copying code.
- **Dead or idle dependencies.** `pickle-com/glass` is gone, terminator has been idle since June, Project Mariner is shut down, Gemini CLI no longer serves consumer logins, the Zed-org `codex-acp` is archived.
- **The Windows Talk Mode gap in OpenClaw is a decision, not a delay** (closed as not planned). Do not plan around OpenClaw adding continuous voice on Windows.
- **Platform features that skip this hardware.** Microsoft's local Copilot actions are Copilot+ only; Recall is Copilot+ only; `odr.exe` (the MCP On-Device Registry tool) is not present on this retail build [ran]. Do not make ODR registration a v0.1 dependency.

## 9. What was run and measured on this PC

All on 2026-10-11, owner's PC. Scratch work lives in the session scratchpad under `prior-art/`.

| What | Result | Tag |
|---|---|---|
| OS | Windows 11 Home 25H2, build 26200.9457, EditionID Core | ran |
| Hyper-V management service (`Get-Service vmms`) | Not present, so the former Cowork VM sandbox cannot run here | ran |
| `odr.exe` | Not on PATH and not in System32 | ran |
| Installed vendor apps | Microsoft.Copilot 155.0.4283.45; Claude desktop (MSIX) 2.31226.1.0 bundling Claude Code 2.1.295 (`claude.exe`, 247.5 MB); npm global Claude Code 2.1.175; no ChatGPT, Codex, Gemini, Grok or Ollama | ran |
| Claude desktop app memory while in active use | 17 processes, 1,288 MB working set, 2,765 MB private bytes | ran |
| Claude Code CLI processes (this research workflow's agents) | 4 processes, 990 MB working set, 2,560 MB private | ran |
| `claude --version` through the npm shim, 5 runs | 1,508 / 2,263 / 1,383 / 1,383 / 1,406 ms (median 1,406 ms) | ran |
| Native `claude.exe --version`, 5 runs | 234 / 60 / 80 / 59 / 66 ms | ran |
| `claude -p` tiny prompt, npm 2.1.175, 3 runs | "Not logged in" after 2.8 to 3.5 s internal, 6.4 to 8.5 s wall; no model call happened | ran |
| `claude -p` tiny prompt, bundled 2.1.295, 1 run | "OAuth session expired and could not be refreshed" after 19.7 s internal, 27.4 s wall; no model call happened. No further attempts and no login were made. | ran |
| GitHub metadata for ~60 repos (stars, license, last push, releases) | Snapshot in this report | ran (API) |
| GitHub and Hacker News searches for new entrants | Sections 3.3 and 3.7 | ran (API) |

What the numbers say: the per-call cost of spawning a vendor harness cold is seconds before any model work starts, and a Node shim alone adds about 1.3 s. A voice layer that spawns a harness per utterance will feel slow no matter what language it is written in. Keep resident sessions per route and measure warm round-trips in M4. The Electron-class footprint of an always-available assistant on this 16 GB machine is on the order of a gigabyte; that is the comparison point for Eileen's idle budget.

What was not measured: no model round-trip latency (both Claude binaries refused auth from a plain shell), no competitor was installed (Hermes, Paseo, ChatGPT and cua-driver would each need installers or account sign-in), and no OSWorld-style task was run.

## 10. Suggested follow-ups to other docs

Not applied here; these need the owner's yes.

- README "Why now": replace "Nobody has built this" with a sentence that names the halves (ChatGPT desktop for one vendor, Paseo and qwen-audio-agent for voice plus harnesses, Hermes for voice plus Windows control) and states the combination Eileen adds.
- RESEARCH.md 2.1: add Paseo, qwen-audio-agent, Happy, Jarhead and Skales; update the OpenClaw, Hermes and vendor rows per section 2.
- DECISIONS: a new entry for the subscriptions-only rule that supersedes the "API keys first-class" consequence of D4 for everything except DeepSeek, and an entry naming Antigravity CLI as the Gemini route.
- ROADMAP M3: replace "Haiku 4.5 through LiteLLM" with "a resident subscription-harness session or a local model", and pull a minimal tray with mute into v0.1.
- ROADMAP M4: point `codex-acp` at `agentclientprotocol/codex-acp`, add Grok Build and DeepSeek Harness routes, and rename workers to "Claude", "Codex", "Gemini", "Grok", "DeepSeek".

## 11. Sources

All accessed 2026-10-11 unless a date is given.

Vendor products

- OpenAI, "ChatGPT Voice is now in the desktop app", 2026-07-23: https://community.openai.com/t/chatgpt-voice-is-now-in-the-desktop-app/1388031
- OpenAI, Computer Use docs: https://learn.chatgpt.com/docs/computer-use
- OpenAI, Codex voice docs: https://learn.chatgpt.com/codex/features/voice
- VentureBeat, GPT-Live voice control for Codex and ChatGPT desktop, 2026-07-23: https://venturebeat.com/orchestration/agentic-coding-goes-hands-free-as-openai-brings-gpt-lives-full-duplex-voice-control-to-codex-and-chatgpt-on-the-desktop
- TechCrunch, voice mode in ChatGPT desktop, 2026-07-24: https://techcrunch.com/2026/07/24/openais-new-voice-mode-makes-it-to-the-chatgpt-desktop-app/
- Engadget, ChatGPT Work, 2026-07-09: https://www.engadget.com/2211869/openai-releases-chatgpt-work-tool-macos-windows-web-plans/
- Codex computer use on Windows (third-party), 2026-05-30: https://codex.danielvaughan.com/2026/05/30/codex-computer-use-windows-desktop-automation-qa-testing-workflows/
- Anthropic on X, computer use on Windows, 2026-04-02: https://x.com/claudeai/status/2039836891508261106
- Claude Help Center, voice mode: https://support.claude.com/en/articles/11101966-use-voice-mode
- Claude Code issue #100893 (voice mode in Cowork and Dispatch), 2026-10-09: https://github.com/anthropics/claude-code/issues/100893
- SiliconANGLE, Cowork inside Claude chat, 2026-09-16: https://siliconangle.com/2026/09/16/anthropic-brings-cowork-directly-inside-claudes-chat-interface/
- 9to5Mac, Cowork merge, 2026-09-16: https://9to5mac.com/2026/09/16/anthropic-merging-claude-cowork-with-chat/
- Background computer use in Cowork and Claude Code, 2026-09-05: https://aicatchup.com/news/claude-background-computer-use-cowork-code
- Claude desktop control beta on macOS and Windows, 2026-09-03: https://letsdatascience.com/news/claude-extends-desktop-control-across-macos-and-windows-0e32c092
- Cowork on Windows Home (Hyper-V) issue: https://github.com/anthropics/claude-code/issues/27906
- Claude in Chrome GA, 2026-08-26 (coverage): https://gigazine.net/gsc_news/en/20260827-claude-chrome-available/
- Windows Experience Blog, "Building Windows for hybrid intelligence", 2026-10-07: https://blogs.windows.com/windowsexperience/2026-10-07/building-windows-for-hybrid-intelligence/
- Microsoft Support, experimental agentic features: https://support.microsoft.com/en-us/windows/ai/ai-features/experimental-agentic-features
- Microsoft Learn, MCP on Windows overview: https://learn.microsoft.com/en-us/windows/ai/mcp/overview
- Windows Latest, Ask Copilot paused, 2026-10-07: https://www.windowslatest.com/2026/10/07/nobody-wants-copilot-microsoft-pauses-copilot-on-windows-11s-taskbar-to-incorporate-your-feedback/
- Neowin, Hey Copilot and Copilot Vision: https://www.neowin.net/news/microsoft-launches-copilot-vision-for-windows-11-and-hey-copilot-voice-activation/
- Microsoft Scout coverage: https://www.thurrott.com/a-i/336926/build-2026-microsoft-unveils-scout-personal-work-agent-powered-and-new-in-house-ai-models and https://learn.microsoft.com/en-au/microsoft-scout/overview
- Google, Gemini app for Windows, 2026-09-10: https://blog.google/innovation-and-ai/products/gemini-app/gemini-app-now-on-windows/
- Project Mariner shutdown, 2026-05: https://www.androidheadlines.com/2026/05/google-shuts-down-project-mariner-ai-agent.html
- Gemini Spark: https://techcabal.com/?p=192551
- Gemini CLI consumer cutoff (Hermes issue): https://github.com/NousResearch/hermes-agent/issues/29294
- Antigravity CLI headless bridge (issue #76 context): https://github.com/rhishi99/agy-headless-bridge
- Perplexity Portable Computer for Windows, 2026-09-14: https://www.unite.ai/perplexity-brings-its-local-portable-computer-agent-to-windows-pcs/
- Perplexity on X, Personal Computer for Windows: https://x.com/perplexity_ai/status/2082103880155046176
- Manus My Computer, 2026-03: https://www.shopifreaks.com/metas-manus-launches-desktop-app-for-mac-and-windows-with-ai-agent-that-automates-files-apps-and-emails/
- Meta Muse for Windows, 2026-10-07: https://windowsreport.com/meta-muse-is-coming-to-windows-microsoft-confirms/
- Grok Bot and Grok desktop coverage: https://www.productcompass.pm/p/grok-build-desktop
- Copilot+ 40 TOPS requirement: https://www.qualcomm.com/news/onq/2024/06/what-on-earth-is-a-copilot-plus-pc

Open-source projects (README, docs and GitHub API)

- OpenClaw: https://github.com/openclaw/openclaw · https://docs.openclaw.ai/platforms/windows · https://docs.openclaw.ai/nodes/computer-use · https://docs.openclaw.ai/nodes/talk · changelog https://raw.githubusercontent.com/openclaw/openclaw/main/CHANGELOG/2026.10.1.md
- OpenClaw Windows Hub: https://github.com/openclaw/openclaw-windows-node · Talk Mode proposal https://github.com/openclaw/openclaw-windows-node/issues/1124
- Hermes Agent: https://github.com/NousResearch/hermes-agent · voice https://hermes-agent.nousresearch.com/docs/user-guide/features/voice-mode · wake word https://hermes-agent.nousresearch.com/docs/user-guide/features/wake-word · computer use https://hermes-agent.nousresearch.com/docs/user-guide/features/computer-use · providers https://hermes-agent.nousresearch.com/docs/integrations/providers
- Open Interpreter: https://github.com/openinterpreter/openinterpreter · 01: https://github.com/OpenInterpreter/01
- Paseo: https://github.com/getpaseo/paseo · voice docs https://github.com/getpaseo/paseo/blob/main/public-docs/voice.md
- qwen-audio-agent: https://github.com/QwenAudio/qwen-audio-agent
- Happy: https://github.com/slopus/happy · https://github.com/slopus/happy-agent · https://github.com/slopus/happy-desktop
- Happier: https://github.com/happier-dev/happier
- Omnara: https://github.com/omnara-ai/omnara
- Jarhead: https://github.com/Kevin-Liu-01/Jarhead
- Skales: https://github.com/skalesapp/skales
- vierisid/jarvis: https://github.com/vierisid/jarvis
- Orkas: https://github.com/Orkas-AI/Orkas · open-cowork: https://github.com/OpenCoworkAI/open-cowork · Monaw: https://github.com/kailiang0120/monaw
- Cua: https://github.com/trycua/cua · Windows internals post, 2026-05-27: https://cua.ai/blog/inside-windows-computer-use
- Windows-MCP: https://github.com/CursorTouch/Windows-MCP · terminator: https://github.com/mediar-ai/terminator · mcp-windows: https://github.com/sbroenne/mcp-windows · pi-computer-use: https://github.com/injaneity/pi-computer-use
- Microsoft UFO: https://github.com/microsoft/UFO · MXC: https://github.com/microsoft/mxc
- Agent S: https://github.com/simular-ai/Agent-S
- Codex: https://github.com/openai/codex · codex-acp: https://github.com/agentclientprotocol/codex-acp · claude-agent-acp: https://github.com/agentclientprotocol/claude-agent-acp · ACP: https://github.com/agentclientprotocol/agent-client-protocol
- Antigravity CLI: https://github.com/google-antigravity/antigravity-cli · Gemini CLI: https://github.com/google-gemini/gemini-cli
- Grok Build: https://github.com/xai-org/grok-build · DeepSeek Harness: https://github.com/deepseek-ai/deepseek-harness
- voicemode: https://github.com/mbailey/voicemode · Pipecat: https://github.com/pipecat-ai/pipecat
- Talon: https://talonvoice.com/ · talon-ai-tools: https://github.com/mpqmpqm/talon-ai-tools
- Wispr Flow: https://wisprflow.ai/ · Windows footprint review: https://www.eesel.ai/blog/wispr-flow-review
- Highlight: https://docs.highlightai.com/guides/context/ht-auto-attach · Clicky Windows status: https://www.voiceos.com/compare/heyclicky-windows · Fluent: https://fluentforall.com/ · Ski: https://heyski.io/ · VoiceOS: https://www.voiceos.com/changelog
- Hacker News search (Algolia): https://hn.algolia.com/api/v1/search
