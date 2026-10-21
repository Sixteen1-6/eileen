# Roadmap

Eileen ships in small, demonstrable steps. Each v0.1 milestone on this page is also a GitHub issue with the same title, so the tracker and this page never disagree on scope.

The milestones are the spine. The October 2026 direction ([OWNER-DIRECTION.md](OWNER-DIRECTION.md)) changed what is inside them: the core is Rust, Claude on the owner's plan is the only v0.1 route, Eileen drives the PC through its own desktop server, and the learning loop, the HUD, dictation, voices (including Chatterbox per voice), the first-run surprise path and settings by voice are part of v0.1. The full specification, contracts and the ten work packages that build these milestones are in [BUILD.md](BUILD.md); the demo scripts referenced below are in its section 1. Research and measurements are in [docs/research/](research/) and [RESEARCH.md](RESEARCH.md).

Numbers in acceptance criteria are targets we set for our own code, to be measured on the owner's PC (Windows 11 Home, Core Ultra 7 155H, 16 GB, no NVIDIA GPU, one 1920x1200 monitor) and then tightened or loosened with evidence.

## v0.1: the whole PC, by voice, learning as it goes

One sentence: talk to it with no wake word, it knows what is on the Windows desktop, it opens and drives any app including Resolve, it starts Claude jobs that run in parallel while you keep talking, it shows what it is doing on the HUD, and the second time you ask for something hard it is faster.

| Milestone | Area | Work packages (BUILD.md section 6) | Depends on | Status |
|---|---|---|---|---|
| M0: Foundation | workspace, contracts, gates | WP0 | nothing | not started |
| M1: Desktop grounding | desktop | WP5 (DesktopState, windows, app index) | M0, slice S1 | not started |
| M2: Action layer and app hooks | actions, apps | WP5 (tools), WP6 (Resolve, media, Office, Explorer, recent work) | M1 | not started |
| M3: Voice loop and the HUD | voice, HUD | WP1 (turn-taking), WP2 (runtime, mute key and tray, slice S1), WP4 (speech, dictation, voices), WP8 (HUD, first run) | M0 | not started |
| M4: Claude workers, jobs, routing, tiers, learning, settings | harness, core, learning | WP1, WP3 (Claude driver, brain, the approval contract test), WP7 (intent, settings, learning), WP8 (Claude plugin) | M3 (slice S1) | not started |
| M5: Phone channels | channels | moved to v0.2 as the first outward connector | | deferred |
| M6: Integration demo and the test fleet | integration | WP9 (fleet, benchmarks, demo); scenario sharing already ships with slice S1 (WP2) | M1 to M4 | not started |

The build order is not the milestone order. The thin slice comes first and is live: as soon as its four named pull requests (the runtime's live wiring, the Claude brain and worker, and the audio and speech slice) have merged, which the plan schedules inside the first feature wave, with the real microphone and speakers, a mute key and a console view, one fast-path command, one interruption, one brain answer and one Claude job. No desktop or app work starts until it passes on the owner's PC. BUILD.md section 6.1 has the waves and the rules that keep parallel builds from overloading the owner's PC.

### What changed in October, and why

| Item | Was | Now | Why |
|---|---|---|---|
| Core language | Python | Rust | Owner direction; start-up, idle footprint, hook and UIA latency, single binaries (decision D17) |
| Conversational model | Haiku through LiteLLM with an API key | A warm `claude -p` session on the owner's plan with a hot spare, plus a deterministic fast path; it may run deterministic actions itself | Subscriptions, not API keys; DeepSeek is the only key ever allowed (D18, D20, D41) |
| Workers | ACP adapters for Claude and Codex | Native Claude driver with a small warm pool; Codex, Grok and DeepSeek as slots behind the same trait | Claude is the only v0.1 route; Codex is not installed (D19, D41) |
| Action layer | Cua Driver | Eileen's own Rust desktop server, which stops the moment the user's own hands move | Measured faster grounding, smaller tool list, safety rules in the server (D21) |
| Approvals | any "yes" | Only the user's own input: a physical click, the approval hotkey on a real keyboard, or the action word in the voice window (also matched to the user's voice once the speaker check is enrolled); the route for Claude workers proven against the real CLI | Safety review and its verification round (D43, D51) |
| HUD with mute | v0.2 | v0.1: the mute key and tray with slice S1, the overlay in M3 | Mute one click away and a visual layer are part of the feel |
| Resolve hook | v0.2 | v0.1, M2, with a Fusion-first edit, undo that follows the edit route, and Resolve's own MCP server first once the owner is on 21.1 | The owner's first benchmark is a Resolve request (D48, D59) |
| Learning loop | not planned | v0.1, M4 | The owner's main point (D27, D44) |
| Dictation, voices with consent, settings by voice | not planned | v0.1 | Owner direction 2026-10-20 |
| Chatterbox | v0.2 | v0.1, selectable per voice, live when fast enough, after a head-to-head with Pocket TTS on the owner's PC | Owner direction 2026-10-20 (D39) |
| First-run surprise path | not planned | v0.1, M3 and M6 | Owner direction 2026-10-20 (D49) |
| Community scenario corpus and test fleet | v0.3 evaluation loop | v0.1: sharing with slice S1, the fleet in M6 | Friends test on their own PCs now (D35, D45) |
| Phone channels | v0.1 M5 | v0.2 | The whole PC first, then outward through connectors |
| Gemini | v0.2 | removed | No permitted subscription route; no Gemini key |
| Linux | non-goal | v0.2 target | "Windows first, not Windows only"; neutral crates are tested on Linux from day one |

### M0: Foundation

**Goal.** The workspace, the contracts and every quality gate exist before any feature, so every later package inherits them.

**Scope.** WP0 in BUILD.md section 6.2. Phase A: the Cargo workspace with pinned dependencies and the native libraries declared under Windows target tables, the contracts crate, tracing, the task wrapper, platform traits, the supervisor, the memory-admitting build guard, the simulator skeleton, the basic CI lanes on Windows and Linux with `all-green` replacing the Python checks, the guidance files, and every crate as a documented stub. Phase B, alongside the first features: all 30 repository checks, the generated code map and references, the scenario checks, the nightly, weekly, long-run and release workflows, and `docs/PLAYBOOK.md` with `templates/`, the project-agnostic, fully cited record of how this repository is built, for any other project to adopt.

**Acceptance.** `cargo xtask ci` is green on the owner's PC and `all-green` on GitHub; `main` requires `all-green` and `pr-meta`; every repository check has a test proving it fails on a seeded violation; contract snapshots pin every wire shape.

### M1: Desktop grounding

**Goal.** One cheap state object answers "what is the user looking at".

**Scope.** DesktopState in `eileen-desktop` (BUILD.md section 2.6): monitors with stable ordinals, windows with DWM frame rects and executable versions, foreground and focus, the focused element, the selection, presentation state and whether another app uses the microphone, served from an event-fed cache.

**Acceptance.** `desktop_state` p95 under 100 ms on the owner's desktop; ordinals stable across a reboot; rects agree with a screenshot within 2 px; a hung UIA provider produces a typed error within its deadline, its thread is replaced, and window moves and app launches keep working meanwhile. Demo E4 ("what am I looking at") answers from DesktopState with no screenshot.

### M2: Action layer and app hooks

**Goal.** Deterministic control of windows, apps and media, background first, vision last, and real hooks for the owner's apps.

**Scope.** The 18 desktop tools with safety rules in the server, the input lock, leases that are announced, stop on physical input and hand the screen back, and visible intent; app launch by AUMID with a margin rule; text insertion; volume and window close; SMTC and the media history; YouTube by URL; the Resolve Lua bridge with project and timeline loading, timecode math, stills, face finding, tracking and sharpness, undo that follows the edit route, the Fusion-first edit and render progress, and Resolve's own MCP server as the primary route once the owner is on 21.1 or later; route discovery through the MCP registry and per-app route statistics; Word, Excel and Explorer over COM; the recent-work index; Photoshop designed and tested against a fake.

**Acceptance.** Background UIA actions verified on owned fixtures with the user's cursor and foreground unchanged; password fields and blocked combos refused, including for coordinate clicks; "go to 15:30" lands on the computed timecode for four frame rates; an edit is undone on its own route (Eileen's composition deleted and the previous one loaded, or the previous color version); a YouTube URL plays at the requested second and a seek through SMTC is confirmed. Demos E2, E3 and the deterministic steps of E7.

### M3: Voice loop and the HUD

**Goal.** An always-on local voice loop that hears you, knows when you are talking to it, answers fast, stops the moment you interrupt, and shows what it is doing.

**Scope.** WASAPI audio with sonora echo cancellation against the system's own output and a mute that stops capture; Silero VAD, Nemotron streaming ASR, Smart Turn fused with transcript cues, Kokoro TTS with clause chunking and a phrase cache; barge-in that pauses, resumes after a backchannel and records what was heard; the local addressee gate with a conversation window, and silence during calls, when only the name is heard and replies go to the HUD; the mute key, tray and console view (slice S1); dictation with the insertion ladder and cleanup; voice cloning with consent enrollment and an engine per voice (Pocket TTS, Chatterbox); the HUD overlay and strip with the live thumbnail, confirm panels and settings; the first-run surprise path.

**Acceptance.** Slice S1 passes live on the owner's PC; barge-in at most 150 ms at p95 with speakers; zero self-transcription over 20 announcements and zero leaked words from a video playing beside Eileen; room talk ignored in at least 9 of 10 scripted trials and unnamed requests in a conversation acted on in at least 8 of 10; idle listening a few percent of one core; mute stops capture within one buffer; the HUD never takes focus, never appears in Eileen's screenshots, and ignores synthetic clicks; the Chatterbox and Pocket TTS head-to-head recorded. Demos E1, E12, E13, E16, E17 and E18.

### M4: Claude workers, jobs, routing, tiers, learning and settings

**Goal.** Hand work to Claude as async jobs, keep talking while they run, steer them by voice, and learn from every request.

**Scope.** The warm brain with a hot spare, connectors off and its deterministic `act` tools; the Claude worker driver with sessions, a warm pool, per-session hooks and the tier guard, the path guard, the shell policy and taint; jobs, announcements, narration, approvals, reroute, steer and cancel; fan-out with git worktrees and memory admission; plan-limit awareness; the layered intent resolver; settings by voice with checks, versions, undo and profiles; the learning loop (capture, the librarian, per-session skill snapshots, recipes, the promotion ladder, replay with step checks and handoff, correct and forget); the opt-in `eileen` Claude plugin.

**Acceptance.** The approval route for Claude workers proven against the real CLI; three parallel workers in worktrees with the HUD showing each; an approval only from the user's own input (a physical click, the hotkey from a real keyboard, or the action word in the voice window, with a speaker match once enrolled); the learning loop's CI acceptance tests; "call you Nova" checked, applied on the next turn and undone; the second run of a learned request uses fewer model turns, measured as an A/B. Demos E5, E6, E9, E10, E11 and E14.

### M6: Integration demo and the test fleet

**Goal.** One continuous session exercises M1 to M4 end to end, the owner's benchmarks are timed scenarios, and friends' PCs are a test fleet.

**Scope.** The opt-in fleet runner (recorded mode with no model by default, live mode on the contributor's own plan), anonymised results, ingest and failure clustering, benchmarks B1 to B10 (B10 is the brain benchmark) and the learning curve with A/B timing, soak tests, `docs/FLEET.md`; the real-desktop runner and scenario sharing already exist from slice S1.

**Acceptance.**

- A friend installs Eileen on their own Windows 11 PC from TESTING.md without asking the author anything, says "save that as a test", previews what will be shared, and submits it; CI validates and replays it on every pull request from then on.
- B1 runs for real: "Open the video editor, go to 15:30 and make the face of the girl on the right less blurry", with the sharpness gain measured and the owner's confirmation.
- The demo below runs on the owner's single monitor with no restart and no typed command after `eileen run`, with peak memory and the memory admission record kept. It starts from a data directory in which the user once answered "Resolve" to "which video editor?" (so the phrasing exists) and once played the timelapse (so the media history knows it), and nothing else is learned. It asserts the first row of BUILD.md section 2.10's run-by-run table: "learn from that now" closes the Resolve job's regret window at once, and a drafted skill for that job arrives (committed, or held for review because the job read a screenshot or a web page) before "what have you learned today?" is asked (the script waits up to ten minutes for it), and the answer names it. Each of the three agents starts beside Resolve by the normal memory admission rules within ten minutes, with no "start it anyway". Lines spoken while the video plays carry the name, because Eileen needs it while other audio plays (except for words such as "stop" and "pause").

**Demo script** (about five minutes):

| You say | Expected behaviour |
|---|---|
| "Morning. What's open?" | The foreground app and the open windows, from DesktopState |
| "Open the video editor and snap it to the left half." | Resolve (the stored phrasing) starts; once its main window exists, the HUD outlines the left half and the window snaps |
| "Go to 15:30 and make the face of the girl on the right less blurry." | The Resolve sequence of demo E7, narrated briefly, with the crop shown and "This face, on the right?" before the edit |
| "Yes." (then, once it is done) "Learn from that now." | The edit is applied and checked; the librarian starts on the Resolve job (HUD only) while the demo goes on |
| "Meanwhile, launch three agents to fix the flaky login tests in the demo repo." | Three Claude workers in worktrees of the `flaky-login` fixture; three HUD rows; if memory is tight, the third says why it waits and starts when admission allows, and the record notes which rule admitted each |
| "Play the timelapse on YouTube at 3:20." | Plays from 3:20 (from the media history, no model) |
| (Eileen is talking) "Stop." | Silence within 150 ms |
| "Eileen, how are the agents doing?" | "Two of three pass; agent three is still running." |
| (chime, the video pauses) "Claude wants to push the login branch. Allow?" "No." | Denied; the job reports blocked; the video resumes |
| "Eileen, what have you learned today?" | The Resolve sharpen skill drafted from this session, one sentence, with the full list on the HUD |
| "Eileen, save that as a test." | A scenario saved locally, previewed on the HUD |

## v0.2

| Item | What it adds |
|---|---|
| Codex, Grok and DeepSeek routes | The designed driver slots: Codex app-server, ACP for Grok, the Claude driver on DeepSeek's endpoint with the one permitted key |
| Phone connector | Telegram and ntfy with presence-aware announcements and approvals with a nonce, then voice from anywhere |
| Home connector | Home Assistant over its local WebSocket API, Matter through it; room-sensing devices opt-in per device, local only |
| Containment and storage | A real sandbox for workers and encryption at rest with a DPAPI-wrapped key (D40) |
| ML projects on the user's Colab | A connector that runs a model project end to end on the user's own Google Colab account (owner direction of 2026-10-20): parallel research on what already exists, an evidence-first go or no-go, data collection with licence checks and AI-assisted labelling, training with narrated, steerable progress, honest evaluation, and export to ONNX so the result becomes one of Eileen's own skills. The user signs in to Google; the route (Google's Colab CLI or its Colab MCP server) is chosen at v0.2 design time with a live test. v0.1 keeps it possible: job kinds and connectors are plug-ins |
| Adobe | Premiere and Photoshop UXP plugins connecting out to a loopback WebSocket; the Photoshop benchmark run for real |
| VS Code extension | Editor state (active file, selection) over a named pipe, unless benchmark B4 pulls it into v0.1 |
| Windows App Actions and location | App Actions as a route rung; Windows' location service as an opt-in source |
| Show me first | A whole-plan preview before anything runs |
| Undo inside pro apps | App-level undo inside other pro apps, beyond what Eileen journals itself |
| HUD accessibility | The HUD exposed to screen readers |
| Page-content recall | "What was that page about GPU pricing?" answered from page content, not only titles |
| A richer tour | A longer guided first-run tour with per-app suggestions |
| Releases | Signed automatic updates with one-step rollback, on the nightly, beta and stable channels v0.1 already publishes (needs the signing certificate) |
| Teach by showing | "Eileen, watch me do this", recorded through Eileen's own hooks |
| Routines | Scheduled jobs on the same workers, skills and tiers |
| Languages | Spoken conversation in other languages, capability-driven: Eileen discovers which languages the chosen speech models and routed model support (every speech stage reports its languages from v0.1) and falls back to another model or to typing when one is not supported (owner answer 21) |
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

## After v0.1 ships: staying at the frontier

The owner's direction of 2026-10-20 makes keeping Eileen current a built-in loop, after the main product ships: a weekly scout of new models, CLIs, speech and voice models, MCP servers and research, with a digest and an issue per worthwhile upgrade; bold ideas tried as small measured experiments; every candidate adopted only if it wins the same benchmarks; failing scenarios triaged and drafted into fix pull requests that the owner approves. v0.1 keeps this possible with what it needs anyway: plug-in interfaces for the brain, workers, every speech stage, voices, the intent matcher and the app routes; model names as aliases in config; the benchmark suite, including the brain benchmark, with its public results table (`RESULTS.md` on the repository's `devhub` branch) and results page; and nightly runs against the latest Claude CLI, both the credential-free flag check and the owner's behavioural contract test.

## Non-goals for now

| Not doing | Why |
|---|---|
| macOS in v0.1 or v0.2 | Windows first, Linux next; the platform traits keep macOS a new set of crates later |
| A polished settings GUI | Settings are spoken, shown and clickable on the HUD, and written to commented TOML |
| A mobile app of our own | The phone connector covers it in v0.2 |
| Training Eileen's own models | Eileen composes models and adapts no weights of its own; running the user's own training projects on the user's Colab account is a v0.2 connector (above), which is the user's work, not Eileen training itself |
| API keys other than DeepSeek's | Subscriptions through each vendor's own signed-in tool; Gemini stays out until a permitted route exists |

Also not happening: forking OpenClaw or Hermes, handling vendor credentials or running login flows, patching or replacing the Claude app, or bundling GPL or source-available code into Eileen's own binaries.

## Picking up a milestone

Comment on the milestone issue with what you plan to take, and work from the matching work package in BUILD.md. Every acceptance criterion maps to at least one test or scenario, and every pull request leads with its evidence. A milestone closes when its criteria pass on the owner's PC and on one other Windows 11 machine from the test fleet.
