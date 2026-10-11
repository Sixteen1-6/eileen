# Roadmap

Eileen ships in small, demonstrable steps. Each v0.1 milestone on this page is also a GitHub issue with the same title, so the tracker and this page never disagree on scope.

Every claim here about a third-party project, license or vendor policy comes from [RESEARCH.md](RESEARCH.md). This page summarizes and links. It does not restate.

The order follows [RESEARCH.md section 9](RESEARCH.md#9-build-order-for-v01) and three rules:

1. Grounding before action. You cannot move a window you cannot name.
2. Action before voice. A voice loop with nothing to do is a demo, not a tool.
3. Voice before delegation. Conversation must never block on work, so the loop that owns the microphone exists before the first job runs inside it.

Numbers in acceptance criteria (latency, sizes, counts) are targets we set for our own code. They are not vendor claims. Measure on the owner's machine first, then tighten or loosen with evidence.

## v0.1: the nucleus

One sentence: talk to it, it knows what is on the Windows desktop, it moves and arranges windows and apps, it hands a coding task to one or more harnesses, and you can interrupt or reroute while those jobs run.

| Milestone | Area | Depends on | Status |
|---|---|---|---|
| [M1: DesktopState grounding service](#m1-desktopstate-grounding-service) | desktop | nothing | not started |
| [M2: Action layer](#m2-action-layer) | actions | M1 | not started |
| [M3: Voice loop](#m3-voice-loop) | voice | M1 (M2 for the demo) | not started |
| [M4: Harness adapters, job registry, event bus, overrides, permission tiers](#m4-harness-adapters-job-registry-event-bus-spoken-overrides-and-permission-tiers) | harness | M3 | not started |
| [M5: Phone channels and presence routing](#m5-phone-channels-and-presence-aware-announcement-routing) | channels | M4 | not started |
| [M6: Integration demo](#m6-integration-demo) | integration | M1 to M5 | not started |

M2 and M3 can run in parallel once M1 exposes `desktop.state`. Everything else is sequential.

### M1: DesktopState grounding service

**Goal.** A Windows-native sidecar that answers "what is the user looking at" as one state object over MCP, with a live event stream.

**Scope.**

- Monitor model: `EnumDisplayMonitors` joined with `QueryDisplayConfig`, our own left-to-right ordinals, EDID names, work areas, per-monitor DPI. Settings' "Identify" numbers are not exposed by any public API, so we assign our own and keep them stable.
- Window model: `EnumWindows` filtered by DWM cloaked state, DWM extended frame bounds, `MonitorFromWindow`, z-order, process name and PID.
- Foreground and focus history: a `SetWinEventHook` thread for foreground, focus, move, name and cloak events. A UI Automation focus-changed handler on an MTA thread.
- Focused element: control type, name, rect, supported patterns.
- Selection: `TextPattern.GetSelection` with a clipboard-sequence-number fallback. `null` when unknown, never a stale value.
- Caret rect when the element exposes one.
- MCP tools: `desktop.state`, `window.move`, `window.snap`, `window.focus`, `window.minimize`, `window.maximize`, plus an event stream for focus and foreground changes.
- Per-monitor-V2 DPI awareness on the grounding thread. Every rect is physical pixels.
- A timeout around every UIA call.
- Two output modes: a compact summary for prompts and a full dump for tools.
- Language: Rust (uiautomation-rs plus the windows crate) or C# (FlaUI.UIA3). The first implementer decides. The Rust toolchain is not installed on the owner's machine today, so C# has a shorter path to a first run there.
- Python client in `src/eileen/desktop/` that speaks MCP to the sidecar and exposes a pydantic `DesktopState` model.

**Acceptance criteria.**

- `desktop.state` returns in under 100 ms at p95 over 100 calls on a desktop with 30 open windows and three monitors.
- Monitor ordinals match the physical left-to-right arrangement and survive a reboot and a monitor hot-plug without renumbering.
- On a mixed-DPI setup (100 percent and 150 percent), returned rects agree with a screenshot's pixel coordinates within 2 px.
- Cloaked UWP windows and tool windows are absent from the visible list. Test: open Settings, close it, confirm the list is back to baseline.
- A foreground-change event arrives within 50 ms at p95 of an Alt+Tab.
- Selecting text in Notepad, Word, the Chrome address bar and VS Code returns the selected string or `null`. A test copies "A", selects "B" elsewhere, and asserts the result is never "A".
- `window.move` to a monitor ordinal places the window inside that monitor's work area (taskbar excluded) and leaves the foreground window unchanged.
- A hung UIA provider (test with a deliberately suspended process) produces a timeout error within 2 s instead of hanging the service.
- The compact summary of a typical desktop is under 4 KB of JSON.
- The service starts in under 2 s, runs as a normal user (no admin), and logs to a file.

**Demo script.** M1 has no voice yet. The demo is typed; the spoken equivalents arrive in M3.

| You type | Expected behavior |
|---|---|
| `eileen desktop state` | Prints monitors with ordinals and names, visible windows with monitor and rect, the foreground window and process, the focused element, and the current selection or `null`. |
| `eileen desktop watch` then press Alt+Tab twice | Two foreground events print within 50 ms each, with the new window title and process. |
| Select a sentence in Word, then `eileen desktop state --field selection` | Prints exactly the selected sentence. |
| `eileen window move --to 2` | The foreground window lands inside monitor 2's work area. Focus does not change. |
| Unplug a monitor, then `eileen desktop state` | The monitor list shrinks, ordinals of the remaining monitors are unchanged, no restart needed. |

Spoken equivalents from M3 onward: "What am I looking at?", "Move this to screen two."

**Builds on.** Win32 (`EnumWindows`, `SetWindowPos`, `SetWinEventHook`, `EnumDisplayMonitors`, `QueryDisplayConfig`), DWM window attributes, UI Automation. Reference code: terminator, Windows-MCP, mcp-windows, Microsoft UFO, FlaUI-MCP (all MIT). Libraries: uiautomation-rs and the windows crate, or FlaUI.UIA3. Details in [RESEARCH.md section 2.2](RESEARCH.md#22-windows-action-and-grounding-components).

**Suggested owner skills.** Win32 and COM threading (MTA versus STA), UI Automation, DPI handling, Rust or C#, MCP server basics, a little Python for the client.

### M2: Action layer

**Goal.** Deterministic control of windows, apps, the browser and media, with vision as the last resort and never the default.

**Scope.**

- Cua Driver installed as the host daemon. Its MCP tools exposed through Eileen's tool registry: window state with indexed UIA elements, background element actions, coordinate fallback, explicit foreground escalation.
- Playwright MCP with the Playwright browser extension for real logged-in Chrome and Edge tabs. Chrome DevTools MCP as the Chrome-only alternative.
- COM adapters in `src/eileen/apps/`: Word, Excel, PowerPoint and Outlook through `GetActiveObject` (Selection, ActiveCell, ActiveExplorer). Explorer through `Shell.Application` (selected files per window).
- SMTC adapter through `Windows.Media.Control`: now playing, position, play, pause, next, seek. No vendor credentials.
- The tool ladder, enforced in code: App Actions when an app publishes them, then COM and app scripting, then UI Automation, then browser DOM, then vision. Every action records which rung ran.
- Vision fallback wired to Anthropic's computer toolset behind a config flag, off by default. Cua Driver provides the screenshot-and-input executor.
- Every action returns a structured result: `rung`, `duration_ms`, `reversible`.
- `src/eileen/browser/` for Playwright MCP, `src/eileen/desktop/` for the Cua adapter.

**Acceptance criteria.**

- A `move_window(target_monitor=2)` call moves the foreground window into monitor 2's work area within 300 ms and does not steal focus from the user.
- Snap left, right and quarter positions respect the work area and per-monitor DPI on both a 100 percent and a 150 percent monitor.
- A Cua Driver background click on a button in a non-foreground Win32 app succeeds while the user's real cursor stays where it was.
- For a Chromium or Electron target, the result reports that foreground delivery was used, and the foreground is restored afterwards.
- Playwright MCP attaches to an existing logged-in tab through the extension, returns an accessibility snapshot with refs, clicks a ref, and never launches a second browser profile.
- Office: the selected text in a running Word document and the active cell address in a running Excel workbook are read without opening a new instance.
- Explorer: selected files in the frontmost Explorer window come back as absolute paths.
- SMTC: pause Spotify, then read title, artist and position, with no Spotify account linked.
- With a UIA path available, no screenshot is taken. A test asserts zero calls to the vision tool across the whole action suite.
- Enabling vision requires both a config flag and an API key. Without either, the vision rung reports "disabled" and the ladder stops there.
- The Windows Terminal adapter only opens new tabs or panes. It never sends text into an existing pane (rejected upstream on security grounds).

**Demo script.** Typed until M3; the spoken lines are listed so the voice milestone has a fixed target.

| You type now, say from M3 | Expected behavior |
|---|---|
| `eileen act snap --side left` ("Put this on the left") | Foreground window fills the left half of its monitor's work area. |
| `eileen act arrange chrome:left word:right` ("Chrome on the left, Word on the right") | Both windows snap on the current monitor. Result shows `rung: uia`. |
| `eileen browser tabs` ("What tabs do I have open?") | Lists the titles and URLs of the real, logged-in browser's tabs via the extension. |
| `eileen apps word selection` ("What did I just highlight in Word?") | Prints the selected text. Result shows `rung: com`. |
| `eileen apps explorer selected` ("Which files are selected?") | Prints absolute paths. |
| `eileen media pause` ("Pause the music") | Playback pauses. Result shows `rung: smtc`. |
| `eileen act click --vision "the blue OK button"` with vision disabled | Refuses with "vision is disabled; enable it in config". |

**Builds on.** Cua Driver (MIT), Playwright MCP with the Playwright browser extension (Apache-2.0), Chrome DevTools MCP (Apache-2.0), Office COM, `Shell.Application`, Windows SMTC, Anthropic's computer toolset for the vision rung, M1 DesktopState. Details in [RESEARCH.md sections 2.2 to 2.5](RESEARCH.md#22-windows-action-and-grounding-components).

**Suggested owner skills.** Python, COM interop (pywin32 or comtypes), Windows Runtime access for SMTC, Playwright, MCP client plumbing.

### M3: Voice loop

**Goal.** An always-on local voice loop that hears you, answers fast, knows the desktop, and never blocks while work runs elsewhere.

**Scope.**

- A two-day spike first: does Pipecat run natively on Windows? If yes, build on it. If not, a custom asyncio loop on RealtimeSTT and RealtimeTTS, which are Windows-proven.
- One audio process owns capture and playback. pywebrtc-audio AEC3 uses the played PCM as the far-end reference.
- Silero VAD for speech detection. Smart Turn v3 for semantic end-of-turn.
- STT: sherpa-onnx running Nemotron streaming 0.6B INT8 for live partials and Parakeet TDT 0.6B v3 for finals. Moonshine v2 Medium as the pure-MIT CPU option.
- TTS: Kokoro-82M through RealtimeTTS or kokoro-onnx.
- Conversational model: Claude Haiku 4.5 through LiteLLM to start. A local 4B to 8B instruct model as the free option. Swappable by config only.
- Context blocks: a DesktopState summary from M1 and a jobs block (recency-ordered, generated names, kind, one-line status, focus job). The jobs block stays empty until M4, but the slot and the prompt exist now.
- Barge-in: playback stops in 50 to 150 ms, only the spoken prefix is committed to context, and the announcement is marked partially delivered.
- Announcement policy as a small state machine: interrupt (approval or blocked), wait for idle (completed), silent (progress).
- No wake word. A "was that for me?" check on the fast model decides whether to respond.
- Tool calls from the conversational model go to M2 actions as async calls. Listening never pauses.
- `src/eileen/voice/`.

**Acceptance criteria.**

- Voice-to-voice p50 under 1.2 s with the local model and under 2.5 s with Haiku 4.5, measured with a scripted 50-utterance run on the owner's machine.
- Barge-in stops TTS within 150 ms at p95, and the transcript shows only the prefix that was actually spoken.
- With speakers instead of a headset, 20 announcements produce zero self-transcriptions.
- "Move this window to ... the left monitor" with a 700 ms mid-sentence pause is handled as one turn.
- "What am I looking at?" answers with the foreground app, window title and any selection, from the DesktopState block, with no screenshot.
- "Move this to screen two" triggers the M2 action, the window moves within 300 ms, and the mic stays open during the call.
- A simulated 10 s tool call does not stop the mic. A second utterance during it is transcribed and answered.
- A simulated ApprovalRequested event interrupts with a chime and one sentence. A simulated Completed event waits for end of turn plus one second of silence.
- Switching between Haiku 4.5 and a local model is a config change with no code change.
- The loop runs with no cloud key at all (local STT, local model, local TTS), with degraded latency and a warning in the log.
- Background speech from a TV or a second person that is not addressed to Eileen gets no spoken reply in at least 9 of 10 scripted trials.

**Demo script.**

| You say | Expected behavior |
|---|---|
| "What's on my screen right now?" | Names the foreground app, window title, monitor and any selected text. Under 2 s. No screenshot. |
| "Move this to screen two." | The foreground window moves to monitor 2's work area. A confirmation of five words or fewer. |
| "Put Chrome on the left and Word on the right." | Both windows snap to halves of the current monitor. |
| (while it is still talking) "Stop." | Playback stops within 150 ms. Nothing after "Stop" is answered. |
| "Pause the music." | SMTC pause. |
| "Never mind, play it again." | SMTC play. |
| (a friend in the room) "Do you want coffee?" | No reply from Eileen. |

**Builds on.** Pipecat (BSD-2) or RealtimeSTT and RealtimeTTS (MIT), Silero VAD (MIT), Smart Turn v3 (BSD-2), sherpa-onnx (Apache-2.0) with Nemotron streaming and Parakeet, Moonshine v2, Kokoro-82M (Apache-2.0), pywebrtc-audio, LiteLLM (MIT), Claude Haiku 4.5, M1 and M2. Details in [RESEARCH.md section 4](RESEARCH.md#4-voice).

**Suggested owner skills.** Python asyncio, audio (device handling, sample rates, echo cancellation), ONNX Runtime, prompt design for small models, LiteLLM.

### M4: Harness adapters, job registry, event bus, spoken overrides and permission tiers

**Goal.** Hand a coding task to Claude Agent or Codex as an async job, keep talking while it runs, and override the routing by voice.

**Scope.**

- An Agent Client Protocol client in `src/eileen/harness/` with adapters for claude-agent-acp and codex-acp. The adapter interface leaves room for Gemini CLI and OpenCode later.
- Vendor binaries spawned unmodified in the user's own environment. No OAuth flows. No reading `~/.claude` or `~/.codex/auth.json`. No token proxying. Users run `claude auth login` and `codex login` themselves.
- API keys as first-class config: Anthropic, OpenAI, Bedrock, Vertex, Foundry, `CODEX_API_KEY`. A `subscription_workers: false` flag refuses dispatch to subscription-backed harnesses.
- Workers are named "Claude Agent" and "Codex" in every UI string and log line.
- Job registry in SQLite: id, generated name, kind, status, origin, last progress, salience, transcript path.
- Event bus with TaskStarted, Progressed, Blocked, ApprovalRequested and Completed. The voice loop subscribes now; the phone bridge subscribes in M5.
- Router in `src/eileen/core/`: a YAML policy from task kind to harness and model, and LiteLLM for direct model calls with per-request override.
- Spoken overrides parsed by the conversational model into a one-off override that beats policy: "send this straight to Claude", "keep this local", "have all three debate it". In M4, "all three" fans out in parallel; the judged debate arrives in v0.3.
- Permission tiers: reversible executes immediately, consequential confirms first. Agent-side approval requests surface as ApprovalRequested events.
- Sandbox by default: one workspace per job, and secrets never appear in model-visible text.
- Pronoun resolution for "how's that simulation doing?": match by kind word, then recency; ask when two jobs match.

**Acceptance criteria.**

- "Have Claude Agent fix the failing test" starts a claude-agent-acp session in under 3 s, a job row appears with a generated name, and an unrelated spoken question gets answered while it runs.
- The same holds for Codex through codex-acp.
- Progressed events update the job row silently. "How's it going?" reads the last progress line aloud.
- A Completed event during the user's turn is announced only after end of turn plus one second of silence.
- An ApprovalRequested event interrupts with a chime and one sentence. "Yes" resumes the agent; "no" cancels that step and the job reports Blocked.
- "No, send this to Codex instead" during dispatch cancels the pending Claude Agent job and starts Codex. The registry records `origin: override:voice`.
- "Keep this local" routes the direct model call to the local model. A network mock asserts nothing left the machine.
- With one simulation job and one refactor job running, "how's that simulation doing?" resolves to the simulation. With two simulations, Eileen asks which one.
- Send, delete and spend verbs never execute without an explicit spoken or typed yes. One test per verb.
- A repository grep finds no "Claude Code" in user-facing strings, and no code path reads `~/.claude` or `~/.codex/auth.json`.
- Killing a harness process mid-job marks the job failed with a reason, announces per policy, and Eileen stays up.
- With `subscription_workers: false`, dispatch to a subscription-backed harness is refused with a one-line explanation, and API-key workers still run.
- Two jobs run in parallel with separate workspaces and cannot see each other's files.

**Demo script.**

| You say | Expected behavior |
|---|---|
| "Have Claude Agent add a JSON flag to the desktop state command and run the tests." | "Starting Claude Agent on json-flag." A job row appears. |
| "And have Codex write the changelog entry." | "Starting Codex on changelog." Second job row. |
| "What's on my second monitor?" | Answers from DesktopState while both jobs run. |
| "How's the JSON flag going?" | Reads the last progress line. |
| (chime) "Claude Agent wants to run the test suite with network access. Allow?" then you say "Yes." | The agent resumes. |
| "Actually, send the changelog to Claude instead." | The Codex job is cancelled. A new Claude Agent job starts with `origin: override:voice`. |
| (you stop talking, one second passes) "The JSON flag job finished. Tests pass." | Announcement waited for idle. |
| "Keep the next one local. Summarize the diff." | The summary comes from the local model. No network call. |

**Builds on.** Agent Client Protocol, claude-agent-acp (built on the Claude Agent SDK), codex-acp (`codex app-server`), LiteLLM (MIT), SQLite. Patterns copied: OpenAI Symphony's per-item app-server in isolated workspaces, OpenClaw's background-task ledger, Claude Code's `agent_needs_input` and `agent_completed` hooks. The credential rules are in [RESEARCH.md section 3](RESEARCH.md#3-licensing-and-subscriptions-settled); the orchestration design is in [section 6](RESEARCH.md#6-orchestration-and-routing).

**Suggested owner skills.** Python asyncio, JSON-RPC and ACP, subprocess management on Windows, SQLite, intent parsing prompts, a careful attitude toward secrets.

### M5: Phone channels and presence-aware announcement routing

**Goal.** Reach you on your phone when you are away from the PC, and take redirection from there with the same authority as your voice.

**Scope.**

- Telegram bot in `src/eileen/channels/` using long polling (no public webhook): text both ways, voice notes both ways (transcribed by M3 STT, answered with M3 TTS), inline buttons for approvals.
- ntfy push with action buttons for approvals and completions. Works with ntfy.sh or a self-hosted instance.
- Presence: a presence file plus "no voice activity for N minutes", N configurable. Away routes announcements to the phone. Present routes them to voice.
- The same event bus subscribers, the same permission tiers. An approval from the phone equals a spoken one.
- Tailscale documented as the private network path for anything beyond Telegram and ntfy. No port forwarding.
- Bot token and ntfy topic stored in the encrypted config store. Never in logs or model context.
- Discord live voice is deferred to v0.2.

**Acceptance criteria.**

- With presence `away`, a Completed event reaches Telegram within 5 s and is not spoken.
- With presence `present`, the same event is spoken and not pushed, unless the config says both.
- ApprovalRequested while away produces a Telegram message with Allow and Deny buttons plus an ntfy push. Tapping Allow resumes the agent within 5 s.
- A Telegram voice note "how's the simulation going?" is transcribed, resolved to the right job, and answered with text and a voice note.
- "Send this to Codex instead" typed in Telegram performs the same override as the spoken version and records `origin: override:telegram`.
- Presence flips back to `present` on the next voice activity, and the events that went to the phone are summarized once by voice.
- A test with the bot token in config confirms it never appears in any log line or any prompt sent to a model.
- Everything works behind home NAT with zero inbound ports.
- A message from any Telegram user other than the configured owner is ignored and logged.

**Demo script.**

| Where | You | Expected behavior |
|---|---|---|
| At the PC | "Start the load test with Codex. I'm heading out." | "Starting Codex on load-test." Presence flips to away after N minutes of silence. |
| Phone | (nothing, you are walking) | Telegram: "Codex finished load-test. p95 latency dropped. Report attached." |
| Phone | Voice note: "Have Claude write that up as a PR description." | Reply and voice note: "Starting Claude Agent on load-test-pr." |
| Phone | (nothing) | Telegram: "Claude Agent wants to push the branch." with Allow and Deny buttons. |
| Phone | Tap Allow. | The agent pushes. Telegram: "Pushed." |
| Back at the PC | "I'm back." | "While you were away: load test done, PR description pushed." |

**Builds on.** Telegram Bot API, ntfy (Apache-2.0), Tailscale, M3 STT and TTS, M4 event bus and job registry. Details in [RESEARCH.md section 5](RESEARCH.md#5-phone-and-off-pc-execution).

**Suggested owner skills.** Python asyncio, Telegram Bot API, networking behind NAT, secret storage on Windows (DPAPI or an equivalent).

### M6: Integration demo

**Goal.** One continuous recorded session that exercises M1 to M5 end to end on a Windows 11 machine that is not the author's.

**Scope.**

- Install path: a Python package install plus one `eileen setup` command that installs Cua Driver and the DesktopState sidecar, sets up Playwright MCP, prompts for the browser extension, and checks for `claude` and `codex` on PATH.
- A seed Windows task suite of 10 to 20 scripted tasks, since Windows Agent Arena is unmaintained. This becomes the basis of the v0.3 evaluation loop.
- CI runs unit tests on every pull request. The Windows integration suite runs on a self-hosted runner or by hand before each release.
- Release notes, a recorded demo, and the `v0.1.0` tag.

**Acceptance criteria.**

- A friend installs Eileen on a clean Windows 11 machine from the README in under 30 minutes without asking the author anything. Every stumble becomes an issue.
- The demo script below runs end to end with no restart and no typed command after `eileen start`.
- A file-access audit during the demo shows zero reads of `~/.claude` and `~/.codex`.
- All six milestone issues are closed, and the README, this roadmap and the tracker agree.
- The seed task suite passes at 100 percent on the owner's machine and is checked in under `tests/`.
- The demo video is linked from the release notes.

**Demo script.** The full session, about four minutes.

| You say | Expected behavior |
|---|---|
| "Morning. What's open?" | Lists the foreground app and the windows per monitor. |
| "Put the editor on screen one and the browser on screen two." | Both windows move. Focus stays where it was. |
| "Have Claude Agent add retries to the Telegram client and run the tests. Have Codex review the diff when it's done." | Two jobs, the second waiting on the first. |
| "Pause the music." | SMTC pause. |
| "What did I highlight in Word?" | Reads the selection via COM. |
| "How's the retries job?" | Last progress line. |
| (chime) "Claude Agent wants to run the tests with network access. Allow?" then "Yes." | Resumes. |
| "Actually, keep the review local." | The review runs on the local model instead of Codex. `origin: override:voice`. |
| (you leave the room; N minutes pass) | Telegram: "Retries job finished. Tests pass. Local review found two nits." |
| (Telegram voice note) "Have Claude fix the nits." | "Starting Claude Agent on nits." |
| (back at the PC) "I'm back." | "While you were away: retries done, review done, nits fixed and tests pass." |

**Builds on.** Everything in M1 to M5.

**Suggested owner skills.** Packaging on Windows, CI, technical writing, patience while watching someone else follow your instructions.

## v0.2

Only after v0.1 is tagged. Each item becomes its own issue when it starts.

| Item | What it adds | Builds on |
|---|---|---|
| Cloud sessions for off-PC jobs | Jobs that survive the PC being off, using what the subscriptions already include: Claude Code cloud sessions (`claude --cloud`, `--teleport` to pull one back) and Codex cloud tasks. No self-hosted infrastructure yet. | [RESEARCH.md section 5](RESEARCH.md#5-phone-and-off-pc-execution) |
| OpenClaw Gateway as an optional channel layer | Eileen registers as a node. The gateway brings 20+ chat channels, cron and mobile apps. Optional, off by default, and never a fork. Its security history is the reason to keep it optional. | [RESEARCH.md section 2.1](RESEARCH.md#21-closest-projects) |
| Gemini CLI and OpenCode adapters | Two more ACP adapters behind the M4 interface. Gemini is API key only. | [RESEARCH.md section 3](RESEARCH.md#3-licensing-and-subscriptions-settled) |
| VEGAS Pro 23 extension | A small in-process .NET Application Extension DLL that opens a named pipe, exposing cursor position and split-at-cursor. There is no out-of-process API. | [RESEARCH.md section 2.4](RESEARCH.md#24-app-hooks-that-beat-clicking) |
| DaVinci Resolve 21 extension | Python API through fusionscript (Studio) or an in-app bridge (Free): playhead read and write, markers, render. Split at playhead is `SetCurrentTimecode` then Ctrl+B. Start from davinci-resolve-mcp (MIT). | [RESEARCH.md section 2.4](RESEARCH.md#24-app-hooks-that-beat-clicking) |
| Windows On-Device Registry registration | Register Eileen's control layer as an ODR MCP connector (preview, build 26220.7262 or later). Prefer App Actions whenever an app publishes them. | [RESEARCH.md section 2.4](RESEARCH.md#24-app-hooks-that-beat-clicking) |
| HUD overlay | A small always-on-top strip: listening state, active jobs, last announcement, a mute button. A status strip, not the GUI listed under non-goals. | M3, M4 |
| Discord live voice | Two-way live voice from the phone through a Discord bot, so redirection while away is spoken rather than typed. | [RESEARCH.md section 5](RESEARCH.md#5-phone-and-off-pc-execution) |

## v0.3

| Item | What it adds |
|---|---|
| Evaluation loop | Build, test, measure, compare, keep or revert. The M6 seed suite grows into a real Windows task suite. A nightly run reports pass rates, latency and regressions to the phone. A change that regresses the suite is reverted and reported. |
| Multi-model debate mode | "Have all three debate it" becomes a real mode: fan out to N harnesses or models, a judge model summarizes where they disagree, and you pick by voice. Results land in the job registry like any other job. |
| Local-only mode | One switch: no network egress. Local STT, local TTS, local conversational model, and a local grounder for the vision rung if it is enabled (UI-Venus-2-9B or Holo2-8B, Apache-2.0). Every cloud tool refuses cleanly and says why. |

## Non-goals for now

| Not doing | Why |
|---|---|
| A polished GUI | The interface is voice, the phone, and a status strip. A settings window can wait until the core is stable. |
| macOS and Linux | The grounding service and action layer are Win32, DWM, UIA and COM. Porting is a rewrite of that layer, not a flag. |
| A mobile app of our own | Telegram, ntfy and later Discord cover the phone. OpenClaw's apps are the optional upgrade path. |
| Training models | We compose models; we do not train them. Wake-word models with non-commercial licenses are avoided rather than replaced with our own. |

Also not happening, as the design principles already say: forking OpenClaw or Hermes, handling vendor credentials, or bundling GPL or source-available code into the binary.

## Picking up a milestone

Comment on the milestone issue with what you plan to take. Split it into task issues that link back. Every acceptance criterion above should map to at least one test. A milestone closes when its criteria pass on the owner's machine and on one other Windows 11 machine.
