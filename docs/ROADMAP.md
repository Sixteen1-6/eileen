# Roadmap

Eileen ships in small, demonstrable steps. Each v0.1 milestone on this page is also a GitHub issue with the same title, so the tracker and this page never disagree on scope.

The milestones are the spine. The October 2026 direction ([OWNER-DIRECTION.md](OWNER-DIRECTION.md)) changed what is inside them: the core is Rust, Claude on the owner's plan is the only v0.1 route, Eileen drives the PC through its own desktop server, and the learning loop, the HUD, dictation, voices and settings by voice are part of v0.1. The full specification, contracts and the ten work packages that build these milestones are in [BUILD.md](BUILD.md); the demo scripts referenced below are in its section 1. Research and measurements are in [docs/research/](research/) and [RESEARCH.md](RESEARCH.md).

Numbers in acceptance criteria are targets we set for our own code, to be measured on the owner's PC (Windows 11 Home, Core Ultra 7 155H, 16 GB, no NVIDIA GPU) and then tightened or loosened with evidence.

## v0.1: the whole PC, by voice, learning as it goes

One sentence: talk to it with no wake word, it knows what is on the Windows desktop, it opens and drives any app including Resolve, it starts Claude jobs that run in parallel while you keep talking, it shows what it is doing on the HUD, and the second time you ask for something hard it is faster.

| Milestone | Area | Work packages (BUILD.md section 6) | Depends on | Status |
|---|---|---|---|---|
| M0: Foundation | workspace, contracts, gates | WP0 | nothing | not started |
| M1: Desktop grounding | desktop | WP5 (DesktopState, windows, app index) | M0, M3 runtime | not started |
| M2: Action layer and app hooks | actions, apps | WP5 (tools), WP6 (Resolve, media, Office, Explorer, recent work) | M1 | not started |
| M3: Voice loop and the HUD | voice, HUD | WP1 (turn-taking), WP2 (runtime), WP4 (speech, dictation, voices), WP8 (HUD, tray, hotkeys) | M0 | not started |
| M4: Claude workers, jobs, routing, tiers, learning, settings | harness, core, learning | WP1, WP3 (Claude driver, brain), WP7 (intent, settings, learning), WP8 (Claude plugin) | M3 | not started |
| M5: Phone channels | channels | moved to v0.2 as the first outward connector | | deferred |
| M6: Integration demo and the test fleet | integration | WP9 (end to end, benchmarks, fleet) | M1 to M4 | not started |

The build order is not the milestone order: the thin slice comes first (speak, the brain answers or starts a Claude job, Eileen speaks the result), at the end of the second build wave; desktop and app work follow. BUILD.md section 6.1 has the waves and the rules that keep parallel builds from overloading the owner's PC.

### What changed in October, and why

| Item | Was | Now | Why |
|---|---|---|---|
| Core language | Python | Rust | Owner direction; start-up, idle footprint, hook and UIA latency, single binaries (decision D17) |
| Conversational model | Haiku through LiteLLM with an API key | A warm `claude -p` session on the owner's plan with a hot spare, plus a deterministic fast path | Subscriptions, not API keys; DeepSeek is the only key ever allowed (D18, D20) |
| Workers | ACP adapters for Claude and Codex | Native Claude driver; Codex, Grok and DeepSeek as slots behind the same trait | Claude is the only v0.1 route; Codex is not installed (D19) |
| Action layer | Cua Driver | Eileen's own Rust desktop server | Measured faster grounding, smaller tool list, safety rules in the server (D21) |
| HUD with mute | v0.2 | v0.1, M3 | Mute one click away and a visual layer are part of the feel |
| Resolve hook | v0.2 | v0.1, M2 | The owner's first benchmark is a Resolve request; the bridge already runs on the owner's PC |
| Learning loop | not planned | v0.1, M4 | The owner's main point |
| Dictation, voices with consent, settings by voice | not planned | v0.1 | Owner direction 2026-10-20 |
| Community scenario corpus and test fleet | v0.3 evaluation loop | v0.1, M0 and M6 | Friends test on their own PCs now |
| Phone channels | v0.1 M5 | v0.2 | The whole PC first, then outward through connectors |
| Gemini | v0.2 | removed | No permitted subscription route; no Gemini key |
| Linux | non-goal | v0.2 target | "Windows first, not Windows only"; neutral crates are tested on Linux from day one |

### M0: Foundation

**Goal.** The workspace, the contracts and every quality gate exist before any feature, so every later package inherits them.

**Scope.** WP0 in BUILD.md section 6.2: the Cargo workspace with pinned dependencies, the contracts crate, tracing, platform traits, the supervisor, the simulator skeleton, `cargo xtask` with the build guard and the repository checks, CI lanes (pull request, nightly, weekly, release) on Windows and Linux, the guidance files, the scenario format and its checks, and the retirement of the Python scaffold.

**Acceptance.** `cargo xtask ci` is green on the owner's PC and `all-green` on GitHub; every repository check has a test proving it fails on a seeded violation; contract snapshots pin every wire shape.

### M1: Desktop grounding

**Goal.** One cheap state object answers "what is the user looking at".

**Scope.** DesktopState in `eileen-desktop` (BUILD.md section 2.6): monitors with stable ordinals, windows with DWM frame rects, foreground and focus, the focused element, the selection, presentation state and whether another app uses the microphone, with a live event stream.

**Acceptance.** `desktop_state` p95 under 100 ms on the owner's desktop; ordinals stable across a reboot; rects agree with a screenshot within 2 px; a hung UIA provider produces a typed error within its deadline and never hangs the core. Demo E4 ("what am I looking at") answers from DesktopState with no screenshot.

### M2: Action layer and app hooks

**Goal.** Deterministic control of windows, apps and media, background first, vision last, and real hooks for the owner's apps.

**Scope.** The 17 desktop tools with safety rules in the server, the input lock and visible intent; app launch by AUMID with a margin rule; text insertion; SMTC; YouTube by URL and SMTC; the Resolve Lua bridge with timecode math, stills, face finding and sharpness; Word, Excel and Explorer over COM; the recent-work index; Photoshop designed and tested against a fake.

**Acceptance.** Background UIA actions verified on owned fixtures with the user's cursor and foreground unchanged; password fields and blocked combos refused; "go to 15:30" lands on the computed timecode for four frame rates; a YouTube URL plays at the requested second and a seek through SMTC is confirmed. Demos E2, E3 and the deterministic steps of E7.

### M3: Voice loop and the HUD

**Goal.** An always-on local voice loop that hears you, knows when you are talking to it, answers fast, stops the moment you interrupt, and shows what it is doing.

**Scope.** WASAPI audio with sonora echo cancellation and a mute that stops capture; Silero VAD, Nemotron streaming ASR, Smart Turn fused with transcript cues, Kokoro TTS with clause chunking and a phrase cache; barge-in that records what was heard; the local addressee gate; dictation with the insertion ladder and cleanup; voice cloning with Pocket TTS and consent enrollment; the HUD overlay and strip, tray and hotkeys.

**Acceptance.** Barge-in at most 150 ms at p95 with speakers; zero self-transcription over 20 announcements; room talk ignored in at least 9 of 10 scripted trials; idle listening a few percent of one core; mute stops capture within one buffer; the HUD never takes focus and never appears in Eileen's screenshots. Demos E1, E12, E13, E16 and E17.

### M4: Claude workers, jobs, routing, tiers, learning and settings

**Goal.** Hand work to Claude as async jobs, keep talking while they run, steer them by voice, and learn from every request.

**Scope.** The warm brain with a hot spare and connectors off; the Claude worker driver with per-job hooks and the tier guard; jobs, announcements, narration, approvals, reroute, steer and cancel; fan-out with git worktrees; plan-limit awareness; the layered intent resolver; settings by voice with checks, versions, undo and profiles; the learning loop (capture, librarian, native skills, promotion ladder, replay with step checks and handoff, forget); the opt-in `eileen` Claude plugin.

**Acceptance.** Slice S1 passes (BUILD.md section 6.4); three parallel workers in worktrees with the HUD showing each; an approval only from the user's own voice or click; the learning loop's eleven CI acceptance tests; "call you Nova" checked, applied and undone; the second run of a learned request uses fewer model turns. Demos E5, E6, E9, E10, E11 and E14.

### M6: Integration demo and the test fleet

**Goal.** One continuous session exercises M1 to M4 end to end, the owner's benchmarks are timed scenarios, and friends' PCs are a test fleet.

**Scope.** The real-desktop scenario runner, benchmarks B1 to B9 with A/B timing, soak tests, scenario sharing from the contributor's own GitHub account, `eileen test --fleet`, results ingest and failure clustering, TESTING.md.

**Acceptance.**

- A friend installs Eileen on their own Windows 11 PC from TESTING.md without asking the author anything, says "save that as a test", previews what will be shared, and submits it; CI validates and replays it on every pull request from then on.
- B1 runs for real: "Open Resolve, go to 15:30 and make the face of the girl on the right less blurry", with the sharpness gain measured.
- The demo below runs with no restart and no typed command after `eileen run`.

**Demo script** (about five minutes):

| You say | Expected behaviour |
|---|---|
| "Morning. What's open?" | The foreground app and the windows per monitor, from DesktopState |
| "Put Resolve on the left screen." | The window moves; the HUD outlines it first |
| "Go to 15:30 and make the face of the girl on the right less blurry." | The Resolve sequence of demo E7, narrated briefly |
| "Meanwhile, launch three agents to fix the flaky login tests in the eileen repo." | Three Claude workers in worktrees; three HUD rows |
| "Play the timelapse on YouTube at 3:20." | Plays from 3:20 |
| (Eileen is talking) "Stop." | Silence within 150 ms |
| "How are the agents doing?" | "Two of three pass; agent three is still running." |
| (chime) "Claude wants to push the login branch. Allow?" "No." | Denied; the job reports blocked |
| "What have you learned today?" | Two or three skills, one sentence each |
| "Save that as a test." | A scenario saved locally, previewed on the HUD |

## v0.2

| Item | What it adds |
|---|---|
| Codex, Grok and DeepSeek routes | The designed driver slots: Codex app-server, ACP for Grok, the Claude driver on DeepSeek's endpoint with the one permitted key |
| Phone connector | Telegram and ntfy with presence-aware announcements and approvals with a nonce, then voice from anywhere |
| Home connector | Home Assistant over its local WebSocket API, Matter through it; room-sensing devices opt-in per device, local only |
| Adobe | Premiere and Photoshop UXP plugins connecting out to a loopback WebSocket; the Photoshop benchmark run for real |
| VS Code extension | Editor state (active file, selection) over a named pipe |
| ResolveMCP | Register Resolve 21.1's own MCP server for workers when the owner upgrades, measured against the bridge |
| Chatterbox | Optional expressive pre-rendered speech in a cloned voice |
| Show me first | A whole-plan preview before anything runs |
| Teach by showing | "Eileen, watch me do this", recorded through Eileen's own hooks |
| Routines | Scheduled jobs on the same workers, skills and tiers |
| Languages | Conversation in other languages |
| Linux | `-linux` platform crates: AT-SPI, PipeWire, Wayland or X11, MPRIS, Secret Service |
| One-step install | winget and a signed MSI |
| Observability extras | OpenTelemetry export to a local viewer, ETW, tokio-console builds |
| Claude app extras | An Eileen pane in the Claude app, handing a spoken request into an open Claude session, a desktop extension for the chat tab |

## v0.3

| Item | What it adds |
|---|---|
| Shared skills, opt-in | Signed skill packs, verified across the test fleet, untrusted until checked locally |
| Multi-model debate | Fan a question out to several routes; a judge summarises where they disagree |
| Local-only mode | One switch for no network egress, with a local brain |
| Published evaluation | The scenario corpus's pass rates and latencies per machine class, published per release |

## Non-goals for now

| Not doing | Why |
|---|---|
| macOS in v0.1 or v0.2 | Windows first, Linux next; the platform traits keep macOS a new set of crates later |
| A polished settings GUI | Settings are spoken, shown on the HUD and written to commented TOML |
| A mobile app of our own | The phone connector covers it in v0.2 |
| Training models | We compose models; we never train weights |
| API keys other than DeepSeek's | Subscriptions through each vendor's own signed-in tool; Gemini stays out until a permitted route exists |

Also not happening: forking OpenClaw or Hermes, handling vendor credentials or running login flows, patching or replacing the Claude app, or bundling GPL or source-available code into Eileen's own binaries.

## Picking up a milestone

Comment on the milestone issue with what you plan to take, and work from the matching work package in BUILD.md. Every acceptance criterion maps to at least one test or scenario. A milestone closes when its criteria pass on the owner's PC and on one other Windows 11 machine from the test fleet.
