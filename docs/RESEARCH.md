# Eileen: what to build on, and what to build

Research brief, 2026-09-19, written under an earlier working name. The project was named Eileen on 2026-10-11 ([DECISIONS.md, D16](DECISIONS.md#d16-the-product-is-named-eileen)). Everything here was checked against primary sources (repos, LICENSE files, vendor docs, pricing pages) on this date by five parallel research passes. Where something could not be verified it is marked as such. Sources are listed at the end.

## 1. The short version

Nobody has built this. The pieces exist, and some are very good, but no open-source project combines an always-listening voice loop that keeps talking while work runs, a grounded model of the Windows desktop, deterministic app control, multi-harness delegation (Claude Code, Codex, Gemini CLI) with spoken routing overrides, and phone reach. The only shipped example of the target voice UX is OpenAI's closed GPT-Live + Codex integration in the ChatGPT desktop app.

The recommendation is to compose, not fork:

| Layer | Build on | License | Why |
|---|---|---|---|
| Windows action layer (UIA tree, window state, input, background element actions) | Cua Driver (trycua/cua) | MIT, Rust core, Python + TS SDKs, MCP | Runs on the Windows host, UI Automation first with pixel fallback, synthetic cursor so your pointer is untouched, daily commits. OpenClaw and Hermes both build their computer use on it. |
| Desktop grounding ("what is the user looking at") | Write it: a small service on Win32 + DWM + UI Automation events | your code | Nothing exposes focused element, current selection, monitor model and focus history as one state object. Reference code: terminator (MIT), Windows-MCP (MIT), mcp-windows (MIT), UFO (MIT). |
| Browser | Playwright MCP with its browser extension | Apache-2.0 | The sanctioned way to drive your real logged-in Chrome or Edge tabs. Chrome DevTools MCP is the Chrome-only alternative. |
| Voice loop | Pipecat (or a small custom loop on RealtimeSTT/RealtimeTTS) + Silero VAD + Smart Turn v3 + sherpa-onnx + Kokoro | BSD-2, MIT, BSD-2, Apache-2.0, Apache-2.0 | Only Pipecat and LiveKit treat the LLM as swappable; Pipecat's async tool calls are the closest primitive to "jobs run while we talk", and its turn model is portable (LiveKit's is license-locked). |
| Agent harnesses | Agent Client Protocol (ACP) adapters: claude-agent-acp, codex-acp, Gemini CLI, OpenCode | vendor terms apply | One protocol for every coding agent. Goose, OpenClaw and OpenHands already use it. Spawn the unmodified vendor binaries; never touch credentials. |
| Model routing | LiteLLM | MIT | Configurable policies plus per-request override, which is exactly "you are the supreme router". |
| Phone | Telegram bot + ntfy + Tailscale, Discord for live voice | free | Zero cost, works behind home NAT, voice notes both ways. |
| Off-PC execution | Claude Code cloud sessions (Pro/Max, no compute charge), Codex cloud; later a VPS or OpenClaw cloud workers | included in plans | Coding jobs survive the PC being off without you running infrastructure on day one. |
| Channels, cron, memory, mobile apps at scale (later) | OpenClaw Gateway, with Eileen as a node | MIT, 501(c)(3) foundation | 390k stars, 20+ channels, iOS/Android apps, cron, sub-agents, cloud workers. Integrate as an optional gateway rather than inheriting a 400k-line monorepo and its security history on day one. |

Smallest useful v0.1 (unchanged from the plan): talk to it, it knows what is on your Windows desktop, it moves and arranges windows and apps, it hands a coding task to one or more harnesses, and you can interrupt or reroute while those jobs run. Section 9 turns that into a build order.

## 2. What already exists, ranked as a base

### 2.1 Closest projects

| Project | License / governance | Stack | What it has | What it lacks for Eileen | Verdict |
|---|---|---|---|---|---|
| OpenClaw (ex Clawdbot/Moltbot) | MIT, OpenClaw Foundation 501(c)(3); 390k stars; release every few days | TypeScript gateway, C#/WinUI 3 Windows companion | Gateway daemon, 20+ chat channels, iOS/Android/Wear apps, nodes (devices with camera/screen/voice/system commands), cron and event automations, detached sub-agents, ACP sessions for Claude Code/Codex/Gemini CLI/Cursor/OpenCode, cloud workers since v2.0 (Aug 30 2026), cross-conversation memory. Microsoft Scout is built on it. | Voice wake and talk mode are documented for macOS/iOS/Android only. Gateway "recommended" on WSL2. Windows computer use is experimental via the cua plugin. Secret store not encrypted at rest; one-click RCE (CVE-2026-25253, 40k exposed instances) and nine CVEs in four days in March 2026. | Best commodity layer. Use as an optional gateway, not as the core. The Windows companion repo (2k stars, MIT, C#) is the realistic thing to fork if you go this route. |
| Hermes Agent (Nous Research) | MIT, company-owned; 247k stars; 43k open issues | Python + Electron desktop app | Tier-1 native Windows, desktop app with push-to-talk voice, HUD and terminal, UIA-based computer use in background mode, cron, subagents with live steering, self-improving skills, multi-bot teams | No first-party phone app, Claude Code driven by subprocess/tmux, no native sandbox on Windows, spring 2026 CVE cluster, two DB-corruption hotfixes this month | Strong second. Borrow its voice pipeline and computer-use design. |
| Open Interpreter | Apache-2.0 | Rust | Now literally a fork of OpenAI's Codex CLI tuned for open models (proves Codex is forkable). The "01" voice device project is dormant AGPL since Nov 2024. | Not a personal assistant any more | Avoid as base. |
| Leon | MIT, solo maintainer | TypeScript | Long-running, nice skill taxonomy, 2.0 in developer preview | Solo, no channels, no orchestration | Ideas only. |
| Khoj | AGPL-3.0 | Python | Second-brain RAG with automations | Wrong shape, AGPL | Avoid. |
| Agent Zero | MIT, small Czech company | Python in Docker | Agent lives in a container and reaches the Windows host through an explicit, toggleable connector with computer use | No channels or phone, Docker dependency | Borrow the containment model. |
| NanoClaw, ZeroClaw, IronClaw, Nanobot, QwenPaw, OpenFang | MIT / Apache-2.0 | TS, Rust, Python | Lighter OpenClaw-style gateways. IronClaw's credential boundary (secrets never enter model-visible text) and QwenPaw's native Windows AppContainer sandbox are the ideas worth taking. | OpenFang stale since July; NanoClaw is WSL2-only | Reference designs. |
| Microsoft UFO / UFO³ | MIT | Python | Best-documented UIA + vision hybrid grounding, speculative multi-action batching, picture-in-picture agent desktop | Monolith with no public API | Design reference only. |
| Perplexity Computer, Claude Cowork, Microsoft Scout, ChatGPT agent | closed | | Validate the thesis (multi-model orchestrator, phone-to-PC handoff, always-on agent). ChatGPT agent was removed in Aug 2026. | | Competitors, and proof the category is real. |

### 2.2 Windows action and grounding components

| Component | License | Notes |
|---|---|---|
| Cua Driver | MIT, Rust, v0.28.x, nightlies daily | Windows host daemon over named pipes; 57 MCP tools incl. window state with indexed UIA elements, desktop state via DXGI capture, background element actions via UIA patterns, coordinate fallback, explicit foreground escalation, recording/replay. Chromium/Electron targets need foreground delivery. |
| CursorTouch/Windows-MCP | MIT, Python, 7k stars | 20 tools, own comtypes UIA layer, DisplayInventory with DPI, desktop Snapshot with element ids. Its README admits paragraph-level text selection is unsupported. |
| sbroenne/mcp-windows | MIT, C# .NET 10, weekly releases | Multi-monitor window management with per-monitor DPI, ref-based UI tools, a CLI with daemon. Best reference for DPI-aware window code. |
| mediar-ai/terminator | MIT, Rust | Richest selector-based UIA action set (35 tools) but idle since June 2026. |
| FlaUI-MCP (shanselman) | MIT, C# | Tiny but demonstrates the right snapshot-to-ref-to-pattern-invoke shape. |
| Libraries by language | | .NET: FlaUI.UIA3 v5 (MIT, events + TextPattern). Rust: uiautomation-rs 0.25 (Apache-2.0, focus events + TextPattern + selection) plus the windows crate. Python: yinkaisheng uiautomation (Apache-2.0, TextPattern GetSelection, no events). Avoid pywinauto for new work (one release in six years). |
| Screenpipe | source-available commercial license | Richest 24/7 screen capture; integrate on a user's own machine if they install it, never embed. |
| komorebi, GlazeWM | PolyForm-Strict-derived / GPL-3.0 | Great JSON IPC, but licenses block bundling. Optional user-installed backends only. |
| PowerToys | MIT | No API and no MCP. Only FancyZonesCLI (apply layouts per monitor) and the new PowerDisplay CLI are scriptable. |

What has to be written from scratch, because nothing exposes it: a compact DesktopState object built from EnumWindows + DWM cloaked/extended-frame-bounds + MonitorFromWindow, a monitor model joined from EnumDisplayMonitors and QueryDisplayConfig with your own left-to-right ordinals (Settings' "Identify" numbers are not exposed by any public API), a SetWinEventHook thread for foreground/focus/move/name/cloak events, a UI Automation focus-changed handler on an MTA thread, selection capture via TextPattern with a clipboard-sequence-number fallback, and per-monitor-V2 DPI awareness so every rect is physical pixels. All of it is public, stable Win32.

### 2.3 Browser

| Tool | License | Attach to your real logged-in browser | Verdict |
|---|---|---|---|
| Playwright MCP | Apache-2.0, v0.0.82 (Sep 18) | Yes, via the Playwright Extension (Chrome and Edge, ~100k users, tab picker, reuses your sessions) | Primary. Accessibility snapshot with refs, find, JS eval, network/storage caps. |
| Chrome DevTools MCP | Apache-2.0, v1.9.0 | Yes for Chrome 144+ via `--autoConnect` with a per-request consent dialog | Chrome-only second path. Edge unsupported. |
| Claude in Chrome | proprietary | Yes | Only reachable from inside a Claude Code session on a paid direct plan. |
| browser-use, Stagehand | MIT | Launch their own browser, or need a non-default profile for CDP | Library-grade DOM engines, wrong shape. |
| hangwin/mcp-chrome | MIT | Yes (daily Chrome, all windows) | Community fallback. |

Two constraints to design around: since Chrome 136, raw CDP against the default profile is refused unless the user opts in via `chrome://inspect`; and Chrome enables web accessibility trees only after it notices an assistive-technology client, so DOM-level grounding should come from the extension/CDP path and UI Automation should be reserved for browser chrome. "Highlight the second paragraph and copy exactly that" is a small JS snippet (getSelection / setBaseAndExtent) plus a CDP Ctrl+C; no tool exposes selection as a first-class field.

### 2.4 App hooks that beat clicking

| App | Deterministic hook | Attach to running instance | Existing MCP |
|---|---|---|---|
| Word, Excel, PowerPoint, Outlook | COM via the running object table (GetActiveObject); Selection / ActiveCell / ActiveExplorer | Yes | word-mcp-live (MIT), OfficeMCP |
| Photoshop | COM `Photoshop.Application` + DoJavaScript | Yes | photoshop-mcp (MIT) |
| Premiere Pro | UXP DOM (player position, selection) or CEP/ExtendScript; razor via QE is fragile on 26.x | Only via installed plugin + bridge | premiere-pro-mcp, Adobe_Premiere_Pro_MCP |
| DaVinci Resolve 21 | Python API via fusionscript (Studio) or an in-app bridge (Free); playhead read/write, markers, render. No split API: "split at playhead" must fall back to SetCurrentTimecode then Ctrl+B. | Yes (Studio) | samuelgursky/davinci-resolve-mcp (MIT, 3k stars) |
| VEGAS Pro 23 (Boris FX) | In-process .NET (`ScriptPortal.Vegas`): `Transport.CursorPosition`, `TrackEvent.Split(offset)`. No out-of-process API, so ship a small Application Extension DLL that opens a named pipe. | Only via your own extension | None working |
| Spotify, Chrome, Media Player, any media app | Windows SMTC (`Windows.Media.Control`): now-playing, position, play/pause/next/seek, no auth | Yes | None (Spotify Web API needs Premium and a cloud round-trip) |
| Explorer | `Shell.Application` COM: selected files per window | Yes | None |
| Windows Terminal | `wt -w 0 new-tab / split-pane <cmd>`; sending text into an existing pane was rejected by maintainers on security grounds | New tabs/panes only | None |
| VS Code | Extension API only (CLI has `--goto` but no state query) | Needs an extension | juehang/vscode-mcp-server |
| JetBrains | Built-in MCP server since 2025.2 | Yes, localhost | Built-in (no selection tool yet) |
| Blender, Unity, OBS, VLC, Figma | Socket addon (MIT, 29k stars), editor bridge (MIT), obs-websocket (GPL-2.0), HTTP interface, local MCP on :3845 | Yes when enabled | Yes |

Windows' own direction matters here: App Actions (typed, deterministic, ISV opt-in, needs MSIX packaging) and the On-Device Registry for MCP connectors (preview, build 26220.7262+, third-party hosts and servers supported, servers run contained). Plan to register Eileen's control layer as an ODR connector and prefer App Actions when an app publishes them.

### 2.5 Vision fallback

Use it last. Anthropic's `computer_toolset_20260801` (GA, no beta header, zoom tool; Opus 4.8 / Fable 5 / Sonnet 5) is the best-verified desktop performer at 83 to 85 percent on OSWorld-Verified; GPT-6 Astra is the best pure grounder (92.7 on ScreenSpot-Pro) but has no published OSWorld-Verified score; Gemini 3.8 Flash is cheapest and the only one with an explicit desktop environment flag, still Preview. No vendor ships a reusable Windows executor, so the screenshot-plus-input loop is yours (Cua Driver already has it). Local open grounders: UI-Venus-2-9B or Holo2-8B (Apache-2.0). OmniParser is maintenance-mode; if used, take the MIT YOLOv9-E weights, not the AGPL YOLOv8 ones. Windows benchmarks are stale (Windows Agent Arena unmaintained since Nov 2024); the best open Windows number is Agent S3's 56.6 percent, and multi-step vision runs take tens of minutes, which is the strongest argument for keeping the UI Automation path primary.

## 3. Licensing and subscriptions, settled

The earlier argument was about "is Claude Code more restrictive than Codex". Split it four ways:

| | Claude Code | Codex CLI | Gemini CLI | OpenCode |
|---|---|---|---|---|
| CLI source license | Proprietary, "All rights reserved", source closed (the March 2026 source-map leak did not change this) | Apache-2.0, fully open (Rust) | Apache-2.0; consumer login retired June 18 2026, API keys only | MIT |
| SDK license | MIT code, but use governed by Anthropic's Commercial Terms | Apache-2.0, same repo; app-server JSON-RPC documented for embedding "inside your own product" | headless mode in the Apache repo | MIT |
| Your subscription can sign in the CLI | Yes (Pro/Max; plus Remote Control on all plans, cloud sessions and mobile on Pro/Max at no compute charge) | Yes (Free through Pro) | No longer for consumers | Anthropic sub prohibited; ChatGPT Plus/Pro offered |
| Subscription auth from a third-party wrapper | Raw OAuth tokens prohibited and enforced three times in 2026. Permitted: the end user signs into the unmodified `claude` binary themselves, and Agent SDK / `claude -p` usage "still draws from your subscription's usage limits" (June 15 pause). Never: your app offering claude.ai login, storing or proxying tokens, pooling, modifying the binary, or "Claude Code" branding. | No explicit rule. Docs invite embedding via app-server with an honest `clientInfo.name`; ecosystem uses ChatGPT OAuth openly; no enforcement found. Permissive by silence, not by grant. | API key only | It is the third party; its own docs concede Anthropic's prohibition |

So: on source openness Claude Code is clearly more restrictive, and on third-party wrappers Anthropic has written rules and enforced them while OpenAI has neither. But the Claude rule is narrower than the headlines: driving the unmodified official harness through the Agent SDK is allowed today. Policy volatility is the real risk on both sides (Anthropic's stance changed in Jan, Feb, Apr, May and Jun 2026).

The legally safe design for an open-source orchestrator:

1. Never handle vendor credentials. No OAuth flows, no reading `~/.claude` or `~/.codex/auth.json`. Users run `claude auth login` and `codex login` themselves; Eileen only spawns the vendor binary in the user's environment.
2. Drive the unmodified official harness: Claude via the Agent SDK or `claude -p --output-format stream-json`; Codex via `codex app-server` or `codex exec --json`. No patched binaries, spoofed headers, or look-alike system prompts.
3. Make API keys, Bedrock, Vertex, Foundry and `CODEX_API_KEY` first-class, and ship a flag that disables subscription-backed workers so users can comply instantly if policy changes.
4. One human, one login, one machine. No pooling or reselling.
5. Call workers "Claude Agent" or "Codex", never "Claude Code <anything>".
6. Watch `code.claude.com/docs/en/legal-and-compliance` and Anthropic support article 15036540 for changes.

Prices on 2026-09-19 (per million tokens, input / output): Fable 5.1 $10 / $50; Opus 5 $5 / $25; Sonnet 5 $2 / $10; Haiku 4.5 $1 / $5; GPT-6 Astra $10 / $50; GPT-5.6 Sol $4 / $20; GPT-5.6 Terra $2 / $12; Gemini 3.8 Flash $0.75 / $3.75; Gemini 3.1 Pro $2 / $12; DeepSeek V4 Pro $1.32 / $3.96 peak, half off-peak. Batch and Flex tiers are 50 percent off at Anthropic, OpenAI and Google. Subscriptions: Claude Pro $20, Max from $100 (5x or 20x); ChatGPT Plus $20, Pro from $100 (5x or 20x). Cost strategy stands: subscriptions for interactive harness work, cheap models (Haiku, Flash, DeepSeek off-peak) for volume, frontier models only when the problem needs them, local models for the voice loop.

## 4. Voice

The structural fact that shapes everything: an MCP voice tool inside an agent cannot listen while a tool call runs. The voice loop must sit outside the agent, own the microphone, and treat every agent session as an async job. That is what claude-call and backtalk do at toy scale and what GPT-Live + Codex does at product scale.

Two loops: a voice loop with a fast conversational model that never blocks, and agent loops (Claude Agent SDK, Codex app-server, ACP sessions) dispatched as jobs.

Fully local stack (all permissive licenses, $0/month): one audio process owning capture and playback; pywebrtc-audio AEC3 with the played PCM as the far-end reference; Silero VAD 6.2 (MIT); Smart Turn v3.2 (BSD-2, 8 MB, CPU) for semantic end-of-turn; sherpa-onnx running Nemotron streaming 0.6B INT8 for live partials and Parakeet TDT 0.6B v3 for finals (or Moonshine v2 Medium for a pure-MIT CPU option); a local 4B to 8B instruct model or Haiku 4.5 as the conversational model; Kokoro-82M (Apache-2.0) via RealtimeTTS or kokoro-onnx for output, Chatterbox-Turbo (MIT) for a cloned voice, Pocket TTS on CPU as fallback. Estimated 0.8 to 1.2 s voice-to-voice with a local LLM, 1.5 to 2.5 s if the reply comes from a frontier cloud model, barge-in stop in 50 to 150 ms. Pipecat's native Windows support is unverified (WSL2 is the safe path); RealtimeSTT and RealtimeTTS are Windows-proven.

Best-quality hosted (cascade, keeps your brain in your loop): local VAD + Smart Turn gate the mic; Deepgram Flux ($0.0065/min, built-in end-of-turn) or Nova-3; Claude Haiku 4.5 or Sonnet 5 as the conversational model; ElevenLabs Flash (~75 ms) or Deepgram Aura-2. Roughly $50 to $70 per month at two hours a day. Speech-to-speech APIs (Gemini 3.8 Live at about $35/month, gpt-realtime-2.1 at $215 to $400) keep the conversational brain on the vendor side and have session limits (60 min OpenAI, 15 min audio-only Gemini) an always-on loop must paper over. Anthropic has no realtime audio API; Claude Code's `/voice` is dictation only.

Avoid: Porcupine wake word (key-gated, no personal plan), openWakeWord's pre-trained models (non-commercial; train your own), LiveKit's turn detector (license forbids use outside LiveKit Agents), Piper (now GPL-3.0), Vocode (dead since Nov 2024), Fish Speech and XTTS (non-commercial). Wake word is optional when the loop is always on; a "was that for me" check on the fast model works.

Announcement policy, borrowed from Deepgram's InjectAgentMessage, Gemini Live's tool scheduling and the Apache Maka RFC: approval requested or job blocked interrupts (short chime, then one sentence, interruptible); job completed waits for idle (end of your turn plus one second of silence); progress is silent unless asked. When you are away (presence file or no voice activity for N minutes), the same event goes to Telegram or ntfy instead. On barge-in, commit only the spoken prefix to context and mark the announcement partially delivered.

Pronoun resolution ("how's that simulation doing?"): give the conversational model a recency-ordered jobs block with short generated names, kinds and one-line status, plus a focus job (last mentioned or announced). Resolve by kind word then recency; if two match, ask.

## 5. Phone and off-PC execution

Telegram Bot API (free, long polling works behind NAT, voice notes both ways, inline buttons) as primary; Discord bot for free live two-way voice; ntfy (Apache-2.0, self-hostable) for push with action buttons; Tailscale Personal (free) for a private PWA or SSH to the home PC. WhatsApp Cloud API needs a public webhook and a business portfolio; unofficial WhatsApp libraries risk bans. iMessage has nothing programmable on Windows. Real phone calls: Telnyx or Twilio number at about $1/month plus pennies per minute, only if wanted.

For work that must survive the PC being off, use what the subscriptions already include before building infrastructure: Claude Code cloud sessions (`claude --cloud`, Pro/Max, no compute charge, `--teleport` to pull a session back) and Codex cloud tasks. Claude Code Remote Control lets the Claude app steer a local session from your phone but the local process must keep running. Later options: a small VPS running the gateway, or OpenClaw cloud workers.

## 6. Orchestration and routing

Agent Client Protocol as the contract. ACP adapters exist for Claude Code (claude-agent-acp, built on the Agent SDK), Codex (codex-acp), Gemini CLI, OpenCode, Cursor and Droid; Goose, OpenClaw and OpenHands consume them. Eileen becomes an ACP client with a job registry (SQLite: id, generated name, kind, status, origin, last progress, salience, transcript path) and a local event bus (agent hooks publish TaskStarted / Progressed / Blocked / ApprovalRequested / Completed; the voice loop and the phone bridge subscribe).

Patterns worth copying: OpenAI Symphony (Apache-2.0) runs `codex app-server` per work item in isolated workspaces and keeps tracker credentials out of the agent's environment; OpenClaw's background-task ledger announces completions to the originating channel and wakes the main session; Claude Code's agents view names sessions with a Haiku-generated label and fires `agent_needs_input` / `agent_completed` hooks. Claude Code agent teams do not spawn in headless mode, so parallelism at the harness level comes from many sessions, Codex cloud, or Symphony-style fan-out.

Routing: a YAML policy (task kind to harness and model) executed by the coordinator, with LiteLLM (MIT) for direct model calls because it supports per-request overrides. Spoken overrides ("no, send this to Claude", "keep this local", "have all three debate it") are parsed by the conversational model into a one-off override that wins over policy. Policies are defaults, not prisons.

Permission tiers: reversible actions (move window, open site, run tests, draft email) execute immediately; consequential ones (send, delete, spend) confirm. OpenClaw's approval gates, Hermes's hard-blocked key combos and Cua Driver's foreground escalation ladder are existing implementations of the same idea.

## 7. What to avoid or watch

- Forking OpenClaw's monorepo or Hermes outright: you inherit their security reputation and a moving target. Integrate instead.
- komorebi, GlazeWM, screenpipe, obs-websocket, AutoHotkey in your binary (license conflicts). Talk to them over IPC or spawn them.
- Anything that stores or proxies Claude or ChatGPT credentials.
- Windows Agent Arena as a benchmark (unmaintained); build your own small Windows task suite.
- Antigravity CLI (Google's Gemini CLI successor) until it publishes a license.

## 8. Name

Update, 2026-10-11: the owner named the project Eileen, which was not on the shortlist below. This section stays as the record of why the two earlier names were dropped ([DECISIONS.md, D16](DECISIONS.md#d16-the-product-is-named-eileen)). Update, 2026-10-21: by the owner's answer of 2026-10-20, the earlier names and the franchise one of them came from are no longer written here; their collisions are described without naming them.

Sixty candidates were vetted (GitHub orgs and repos, PyPI and npm, product and company collisions, DNS and RDAP for .ai/.dev/.sh/.io/.app, wake-word phonetics against Picovoice's six-phoneme guideline). Trademark registries blocked automated lookups, so trademark status is inferred from commercial use rather than USPTO records. Raw check outputs are in the session scratchpad (`github.txt`, `pkg.txt`, `dns.txt`, `rdap2.txt`).

### The earlier working name: keep as a working name, do not ship under it

- A live, funded San Francisco startup with the same name markets itself as "The AI Operating System built for work" (11 to 50 employees). An open-source "AI operating layer" under that name would share a name and a category with an existing company.
- The name is an author's coinage from a commercial book and film franchise in active multi-class commercial use (tens of millions of books, a feature film, a video game and a live-action series in development), with the rights held by the author's company and a major studio. A voice product answering to that name, with a story borrowed from the franchise, is the profile that draws cease-and-desist letters and store takedowns.
- On npm the name is a security holding package (previously used by a malicious package); PyPI was free; the `.app` domain was available, `.ai` is the startup's, `.dev` was registered and `.com` belongs to the franchise.
- Wake word: three syllables and six phonemes, which is fine, but a near-homophone of a US state's name and a rhyme with several common words, so accidental triggers in ordinary speech are likely.

### The first codename: internal at most

An NVIDIA embedded-computing product line ships under that name in 2026, GitHub search for it is dominated by that product's repositories, three AI-agent companies already use it, PyPI and npm are taken, and its wake word has only four phonemes.

### Shortlist (ranked)

| Name | Story | Collisions | Wake word | Grade |
|---|---|---|---|---|
| **Ilmari** (il-MAH-ree) | Finnish, from Ilmarinen, the immortal smith of the Kalevala who forged the dome of the sky and the Sampo, the machine that makes whatever is needed. You speak, the smith builds. | PyPI and npm free, `ilmari.app` available, `.ai` registered but empty; GitHub only a 17-star tmux radar for coding agents | 6 phonemes, open vowels, no English homophone | B+ |
| **Verdandi** (ver-DAHN-dee) | The Norn of the present tense; her name is the participle "becoming". An OS that acts on what is on your screen right now. | `verdandi.ai` is an unlaunched placeholder; a 3-star "AI nervous system" repo; Verdent AI is a phonetic neighbour | 7 phonemes, very distinctive | B- |
| **Domovoi** (DOM-oh-voy) | Slavic household spirit that keeps the house, does chores while the family sleeps, warns of danger: a background daemon with a soul. | 178-star AWS Lambda tool; a 1-star "local-first harness for AI coding agents" created three weeks ago | 7 phonemes, impossible to trigger by accident | C+ |
| **Sanjaya** (san-JAY-uh) | In the Mahabharata, granted divine sight to watch the whole battlefield and narrate it live to the blind king: the original screen reader plus reporter. | Common South Asian first name; PyPI `sanjaya` is an agent framework | 7 phonemes | C+ |
| **Enkidu** (EN-ki-doo) | The companion the gods made to match Gilgamesh; oldest epic on earth. | PyPI and npm free; `.ai` parked for sale | 6 phonemes, open vowels | B |
| **Pendragon** (PEN-dra-gon) | Welsh "chief of warriors", Uther and Arthur's epithet; closest in sound to the earlier working name | PyPI and npm free; a 2026 TV series and a UK plc share the name | 9 phonemes | B- |
| Kvasir | Born from the mingling of every god, "extremely wise"; fits a layer that blends many models into one voice | Kvasir Technologies (AI hedge fund, kvasir.ai), Kvasir Analytics, two 400-star repos | 6 phonemes, "kv" onset awkward | C |
| Medha | Sanskrit for the faculty that grasps, understands and retains | PyPI, npm and `medha.app` free; tiny existing uses | 4 phonemes; "Hey Medha" is close to Meta's "Hey Meta" | B- / wake C |
| Hermod | Odin's son who rode to Hel on a hard errand and came back with the answer | 94-star voice stack named hermod; confusable with the 247k-star Hermes Agent | fine | C- |

Blocked by collisions despite perfect stories: Huginn (50k-star automation project), Heimdall (9k-star dashboard), Metis (Tencent, Arm, three AI companies), Saga (Deepgram's "Voice OS"), Yantra (three AI companies), Vör (one syllable, "for"), Narada (Narada AI, computer-use agents, $63M raised), Talos, Iris, Mimir.

Every one of the sixty handles is already taken as a GitHub user or org, so plan on `<name>-ai`, `<name>-os` or `get<name>` for the org regardless of choice.

Taglines for the top pick, Ilmari: "Say it. It's forged." / "The smith inside your machine." For Verdandi: "The present tense of your computer." For Domovoi: "Runs the errands. Reports to your pocket."

## 9. Build order for v0.1

1. Desktop grounding service. A Windows-native sidecar (Rust with uiautomation-rs and the windows crate, or C# with FlaUI.UIA3) exposing an MCP server: `desktop.state` (monitors with your own ordinals and EDID names, visible windows with monitor/DPI/rect/cloaked/z-order, foreground window and process, focused UI element with type/name/rect/patterns, selection text or null, caret rect), `window.move|snap|focus|minimize|maximize`, and a focus/foreground event stream. Run the grounding thread per-monitor-V2 DPI aware; wrap UIA calls with timeouts.
2. Action layer. Install Cua Driver and expose its MCP tools; add Playwright MCP with the extension for the browser; add COM adapters for Office and Explorer and SMTC for media. Vision fallback via Anthropic's computer toolset only when no better interface exists.
3. Voice loop. Pipecat (test native Windows first; fall back to a custom asyncio loop on RealtimeSTT/RealtimeTTS) with the local stack above, a conversational model (Haiku 4.5 to start), the DesktopState block and the jobs block in its context, and the announcement policy.
4. Harness adapters. ACP client with claude-agent-acp and codex-acp; job registry; hooks to event bus; spoken routing overrides; permission tiers.
5. Phone. Telegram bot plus ntfy; presence detection to route announcements.
6. Only then: cloud sessions, OpenClaw gateway integration, Vegas and Resolve extensions, ODR registration, HUD overlay.

Suggested repo shape: `core/` (coordinator, job registry, event bus, router, policy), `voice/`, `desktop/` (grounding service + Cua Driver adapter), `browser/`, `apps/` (Office, Explorer, SMTC, Resolve, Vegas), `harness/` (ACP client + adapters), `channels/` (Telegram, ntfy), `docs/`.

## 10. Sources

Personal AI OS projects: github.com/openclaw/openclaw · docs.openclaw.ai (platforms/windows, nodes, nodes/computer-use, nodes/talk, nodes/voicewake, tools/acp-agents, gateway/cloud-sessions, automation/cron-jobs, releases/2026.8.1/security-and-privacy) · openclaw.ai/foundation · github.com/openclaw/openclaw-windows-node · github.com/nousresearch/hermes-agent · hermes-agent.nousresearch.com/docs · github.com/openinterpreter/open-interpreter · github.com/leon-ai/leon · github.com/khoj-ai/khoj · github.com/agent0ai/agent-zero · github.com/nanocoai/nanoclaw · github.com/zeroclaw-labs/zeroclaw · github.com/nearai/ironclaw · github.com/HKUDS/nanobot · github.com/agentscope-ai/QwenPaw · github.com/RightNow-AI/openfang · github.com/aaif-goose/goose · github.com/microsoft/UFO · microsoft.com/en-us/copilot/blog/2026/06/02/introducing-microsoft-scout-your-always-on-personal-agent · theregister.com/ai-and-ml/2026/08/31/openclaw-20-pours-glitter-on-slow-burning-security-dumpster-fire · proarch.com/blog/threats-vulnerabilities/openclaw-rce-vulnerability-cve-2026-25253 · labs.cloudsecurityalliance.org/research/csa-research-note-hermes-agent-cves-20260504-csa-styled · techtimes.com/articles/321882/20260728/perplexity-brings-ai-desktop-agent-windows-routing-tasks-across-20-models.htm

Windows control and grounding: github.com/trycua/cua · cua.ai/docs/reference/cua-driver/mcp-tools-windows.md · cua.ai/docs/concepts/the-no-foreground-contract.md · github.com/CursorTouch/Windows-MCP · github.com/sbroenne/mcp-windows · github.com/mediar-ai/terminator · github.com/shanselman/FlaUI-MCP · github.com/FlaUI/FlaUI · github.com/leexgone/uiautomation-rs · github.com/yinkaisheng/Python-UIAutomation-for-Windows · github.com/microsoft/windows-rs · github.com/microsoft/CsWin32 · learn.microsoft.com/en-us/windows/win32/api/winuser (enumwindows, setwindowpos, setforegroundwindow, enumdisplaymonitors, monitorfromwindow, querydisplayconfig, setwineventhook, getguithreadinfo) · learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute · learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows · learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading · learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtextpattern-getselection · learn.microsoft.com/en-us/answers/questions/1572062/how-to-get-the-monitor-number-displayed-in-the-sys · learn.microsoft.com/en-us/windows/powertoys/fancyzones · github.com/LGUG2Z/komorebi/blob/master/LICENSE.md · github.com/glzr-io/glazewm · github.com/mediar-ai/screenpipe/blob/main/LICENSE.md · learn.microsoft.com/en-us/windows/ai/app-actions · learn.microsoft.com/en-us/windows/ai/mcp (overview, quickstart-mcp-host, odr-tool) · github.com/microsoft/mxc

Vision computer use: platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool · code.claude.com/docs/en/computer-use.md · developers.openai.com/api/docs/guides/tools-computer-use · developers.openai.com/api/docs/deprecations · ai.google.dev/gemini-api/docs/computer-use · github.com/simular-ai/Agent-S · github.com/microsoft/OmniParser · github.com/inclusionAI/UI-Venus · huggingface.co/Hcompany/Holo2-8B · github.com/xlang-ai/OSWorld-V2 · github.com/microsoft/WindowsAgentArena · leaderboard.steel.dev/leaderboards/osworld · llm-stats.com/benchmarks/screenspot-pro

Browser: github.com/microsoft/playwright-mcp · playwright.dev/mcp/configuration/browser-extension · github.com/ChromeDevTools/chrome-devtools-mcp · developer.chrome.com/blog/chrome-devtools-mcp-debug-your-browser-session · developer.chrome.com/blog/remote-debugging-port · code.claude.com/docs/en/chrome · github.com/browser-use/browser-use · github.com/browserbase/stagehand · github.com/hangwin/mcp-chrome · chromium.googlesource.com/chromium/src/+/main/docs/accessibility/browser/uiautomation.md · github.com/webmachinelearning/webmcp

App hooks: gist.github.com/X-Raym/2f2bf453fc481b9cca624d7ca0e19de8 (Resolve API README v21.0.4) · github.com/samuelgursky/davinci-resolve-mcp · help.magix-hub.com/video/vegas/22/en/content/topics/external/vegasscriptapi.html · github.com/evankale/VegasScripts · github.com/GlitchyPSIX/VEGASRPC · github.com/adobe/premierepro-types · github.com/mikechambers/adb-mcp · github.com/alisaitteke/photoshop-mcp · learn.microsoft.com/en-us/dotnet/api/system.runtime.interopservices.marshal.getactiveobject · learn.microsoft.com/en-us/office/vba/api/word.application.selection · github.com/ykarapazar/word-mcp-live · learn.microsoft.com/en-us/uwp/api/windows.media.control.globalsystemmediatransportcontrolssession · learn.microsoft.com/en-us/windows/win32/shell/shellfolderview-selecteditems · learn.microsoft.com/en-us/windows/terminal/command-line-arguments · github.com/microsoft/terminal/pull/20106 · jetbrains.com/help/idea/mcp-server.html · github.com/ahujasid/blender-mcp · github.com/CoplayDev/unity-mcp · developers.figma.com/docs/figma-mcp-server

Licensing, harnesses, orchestrators, routers, prices: code.claude.com/docs/en/legal-and-compliance · code.claude.com/docs/en/agent-sdk/overview · code.claude.com/docs/en/headless · code.claude.com/docs/en/remote-control · code.claude.com/docs/en/claude-code-on-the-web · code.claude.com/docs/en/agent-teams · support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan · anthropic.com/legal/consumer-terms · github.com/anthropics/claude-code/blob/main/LICENSE.md · github.com/anthropics/claude-agent-sdk-python · platform.claude.com/docs/en/about-claude/pricing · claude.com/pricing · github.com/openai/codex · learn.chatgpt.com/docs (pricing, app-server, codex-sdk, auth, non-interactive-mode, cloud) · github.com/openai/codex/discussions/8338 · openai.com/policies/terms-of-use · developers.openai.com/api/docs/pricing · github.com/openai/symphony · developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli · ai.google.dev/gemini-api/docs/pricing · api-docs.deepseek.com/quick_start/pricing · opencode.ai/docs/providers · github.com/anomalyco/opencode/pull/18186 · goose-docs.ai/docs/guides/acp-providers · github.com/earendil-works/pi · github.com/paperclipai/paperclip · github.com/BloopAI/vibe-kanban · github.com/slopus/happy · docs.litellm.ai/docs/routing · openrouter.ai/docs/guides/routing/provider-selection · github.com/musistudio/claude-code-router · venturebeat.com/technology/anthropic-cracks-down-on-unauthorized-claude-usage-by-third-party-harnesses · theregister.com/software/2026/02/20/anthropic-clarifies-ban-on-third-party-tool-access-to-claude · techcrunch.com/2026/04/04/anthropic-says-claude-code-subscribers-will-need-to-pay-extra-for-openclaw-support · venturebeat.com/technology/anthropic-reinstates-openclaw-and-third-party-agent-usage-on-claude-subscriptions-with-a-catch

Voice and phone: github.com/pipecat-ai/pipecat · docs.pipecat.ai/pipecat/learn/function-calling · docs.pipecat.ai/pipecat/fundamentals/interruptions.md · github.com/pipecat-ai/smart-turn · github.com/livekit/agents · huggingface.co/livekit/turn-detector/raw/main/LICENSE · github.com/kyutai-labs/unmute · developers.openai.com/api/docs/guides/realtime · ai.google.dev/gemini-api/docs/live-api/capabilities · platform.claude.com/docs/en/about-claude/models/overview · code.claude.com/docs/en/voice-dictation · deepgram.com/pricing · developers.deepgram.com/docs/voice-agent-inject-agent-message · elevenlabs.io/pricing/agents · hume.ai/pricing · github.com/snakers4/silero-vad · github.com/dscripka/openWakeWord · picovoice.ai/docs/faq/general · github.com/strands-labs/pywebrtc-audio · github.com/k2-fsa/sherpa-onnx · huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b · huggingface.co/nvidia/parakeet-tdt-0.6b-v3 · github.com/moonshine-ai/moonshine · github.com/KoljaB/RealtimeSTT · github.com/KoljaB/RealtimeTTS · huggingface.co/hexgrad/Kokoro-82M · github.com/resemble-ai/chatterbox · github.com/kyutai-labs/pocket-tts · github.com/OHF-Voice/piper1-gpl · github.com/mbailey/voicemode · github.com/caiovicentino/claude-call · github.com/jaredrhod/backtalk · learn.chatgpt.com/codex/features/voice · github.com/apache/maka/issues/1685 · docs.openclaw.ai/automation/tasks · code.claude.com/docs/en/hooks · code.claude.com/docs/en/agent-view · core.telegram.org/bots/api · docs.discord.com/developers/topics/voice-connections · ntfy.sh · tailscale.com/pricing · developers.facebook.com/docs/whatsapp/pricing · twilio.com/en-us/voice/pricing/us · telnyx.com/pricing/call-control
