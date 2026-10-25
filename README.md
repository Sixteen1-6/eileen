# Eileen

An open-source, voice-first AI layer for Windows 11, with Linux following. The always-on core is Rust, models run through each vendor's own signed-in CLI on the plan you already have, and the license is MIT.

> **Eileen does not run yet.** Work package 0, the foundation, is being built: the CI, the cross-process contracts, the base crates and every other crate as a documented stub. `eileen --version` runs, and every other command says it is not built yet. Nothing listens, speaks or acts. The rest of this page describes what v0.1 is designed to do; [Status](#status) says what exists and what will run first.

## What v0.1 will do

You will leave it running and talk to it while you work on something hard. It will listen whenever the microphone is not muted, with no wake word, follow half-finished speech and context ("no, the other one"), and work out when it is addressed and when a thought is finished. Mute will be one key (Ctrl+Alt+M by default) or one click, and muting stops the capture stream itself, so the Windows microphone indicator goes off. Eileen will know which window has focus, on which monitor, and what is selected, without a screenshot. Common requests will run at once from a deterministic fast path with no model. The rest will go to the brain, a warm Claude session on your plan, and long or hands-on work will become Claude jobs that run in parallel while you keep talking, with short narration as they go. You will be able to interrupt at any moment, and it picks up from what you actually heard. A small on-screen layer, the HUD, will show what it hears, what it is doing and what waits for your approval. It will record what each job did, so the second time you ask for something hard, it is faster.

Requests from the demos in [docs/BUILD.md](docs/BUILD.md) section 1, each of which becomes an automated test:

- "Open the video editor, go to 15:30 and make the face of the girl on the right less blurry." The owner's first benchmark, run for real in DaVinci Resolve, with "This face, on the right?" asked before any edit.
- "Launch three agents to fix the flaky login tests." Three git worktrees, three Claude jobs, three rows on the HUD.
- "Play that timelapse at 1:10", then "pause". Matched by the built-in grammar with no model; pause goes through the system media controls.
- "What am I looking at?" Answered from the desktop's state, with no screenshot.
- "Stop." Mid-sentence; "go on" plays the part you missed.
- "Why did you do that?", "What have you learned?" and "Forget that."
- "I want to call you Nova now."
- "Learn a new voice." Only after the voice's owner reads a consent sentence.
- "Save that as a test."

Dictation will be built in: hold Right Ctrl (configurable) in any app, and clean, punctuated text lands at the caret, with fillers and false starts removed by local rules. A Claude pass that polishes tone is optional and off until you turn it on. Every setting will be reachable by voice and on the HUD, written to commented TOML, with "undo that" and profiles. Eileen will fit itself to your PC: it reads the machine, runs short benchmarks, picks its speech models and how many workers run at once, and explains each choice, which you can override. Also in v0.1: undo for everything Eileen fully controls; quiet during calls, presentations, full-screen apps and screen recording; every task doable without a mouse; and a first-run tour.

Timings are targets, to be measured on the owner's PC (Windows 11 Home, Core Ultra 7 155H, 16 GB, no NVIDIA GPU) and adjusted with evidence. "At once" means under about 0.6 s from the end of speech to the first sound on a quiet PC (about 0.8 to 1.2 s under typical load), which only the fast path and cached phrases reach. A brain turn targets 1.5 s to the first spoken word on a quiet PC, where the measured parts add up to about 2 to 3 s today, and 3 to 5 s under typical load, with an earcon within 150 ms so you know it heard. A worker starts in 1 to 2 s from the warm pool, or 30 to 111 s cold, which is usual on the owner's PC under load, and a deep answer takes tens of seconds from a pooled session, or one to two minutes cold. Idle listening is targeted at under 8 percent of one core for the speech process (BUILD.md section 4.5); voice activity detection alone measured 5.8 percent.

## Principles

These are the design's rules; most of the code that will enforce them is not written yet.

1. **You decide.** Policies are defaults; a spoken override ("use the strongest model for this") wins at once and is recorded on the job.
2. **Conversation never blocks on work.** The brain answers, dispatches jobs and runs a few deterministic actions itself (opening an app, moving a window, seeking media), but never moves the mouse or types. Hands-on work, such as clicks and edits, runs as jobs, done by a worker or by a proven skill's replay.
3. **Intent, not commands.** A request goes through layers: exact and learned phrasings with no model, a local semantic matcher, the brain with live context, and a worker when no capability exists yet. An unclear request gets one short question, never an error.
4. **Subscriptions, not API keys.** You sign in to each vendor's official CLI yourself; Eileen spawns it unmodified and never runs a login or reads, copies or proxies credential files. Claude is the only route in v0.1. Codex, Grok and DeepSeek are designed slots, built in v0.2, and DeepSeek's key is the one API key Eileen accepts, kept in the OS credential store. There is no Gemini route, local models are optional, and workers are named by route.
5. **The app's own route first, screenshots last.** Its API, MCP server or scripting, then registered MCP connectors, UI Automation and the page structure, with screenshots and clicks last. Work happens in the background unless you should look.
6. **Reversible things happen, consequential things ask.** Sending, deleting, pushing and spending confirm first, and only your own input approves: a physical click, the approval hotkey on a real keyboard, or the spoken action word. Screen text, tool results and model output never can. Credentials, payments and security settings are never Eileen's.
7. **It learns on your PC, through the tools it uses.** A successful job becomes a skill in the vendor's own format (SKILL.md folders), and a well-proven skill replays with a check after every step, handing back to a worker the moment one fails. Your preferences, and what you tell it about yourself, live in the memory the vendors already ship: on the Claude route, the claude CLI's own notes, kept in Eileen's data directory, not in your own Claude folder. Names for things, dictation words and recent activity stay in Eileen's own store on your PC. Every request keeps a receipt, so "why did you do that?" has an answer. You can ask what it learned, correct it or make it forget, and a learned skill never skips a confirmation or raises a permission.
8. **Built to be checked.** Every decision is made in one sans-IO state machine, `Core::step`; time and a seeded random number generator are passed in, so a simulator drives it in tests and a failure replays from its seed. Every type that crosses a process, pipe, file or socket lives in `eileen-contracts`. One request ID is traced end to end, with a flight recorder and replay. Pull requests lead with evidence, and a fix adds a regression test. Some of this is code already (the contracts, the supervisor, the platform traits); the rest waits for its work package.

## Privacy and consent

What v0.1 is designed to guarantee:

- Muted means nothing is captured. Speech not addressed to Eileen is never written to disk; during a call it listens only for its name and replies on the HUD.
- What Eileen learns stays on your PC. Sharing skills is planned for v0.3, opt-in and stripped of personal data; in v0.1 a trusted skill can only be offered as a test, through the scenario share preview. Screen text never becomes a skill without your say-so, and "forget that" removes a skill, its versions and its evidence from every store. An off-the-record mode records nothing.
- Memories that vendor apps keep in the cloud are off by default, opt-in per route, and come in only through the vendor's own manual export.
- No scenario, recording or test result leaves the PC without a preview and your approval. A shared scenario is redacted first, screenshots are off by default, and your real voice goes only if you explicitly allow it. What you ask, the desktop context a request needs (such as window titles and the selection) and the relevant notes do go to Claude on your plan as part of each turn, and deep answers run web searches.
- Cloning a voice needs that person's spoken consent, stored with the profile on the PC only. No watermark by default; watermarking is opt-in.
- A speaker check gates nothing unless its measured false-reject and false-accept rates are both well under 1 percent. The test fleet is opt-in and identifies a machine only by a salted one-way hash, never by serials, user names or accounts. Room-sensing devices, when they come, are opt-in per device and processed locally.

## Architecture at a glance

These processes are the v0.1 design; their crates are documented stubs today.

```text
                         you (speak, click, type)
                                    |
+-----------------------------------------------------------------------+
| eileen.exe (Rust core, always on)                                     |
|   audio, mute, echo cancellation, tray, hotkeys                       |
|   Core::step: turns, intent, jobs, routing, permissions, learning     |
|   store, bus, dispatcher, supervisor                                  |
+-----------------------------------------------------------------------+
      |               |               |              |            |
eileen-speech   eileen-desktop   eileen-apps    eileen-hud    claude -p
VAD, ASR,       UI Automation,   media, Office, overlay,      brain and
end of turn,    input, capture,  Explorer,      strip,        workers on
TTS, voices     DesktopState     Resolve        thumbnail     your plan
                                                                  |
                                                              eileen-shim,
                                                              one per session:
                                                              Eileen's MCP tools

eileen-guard: crash and hang dumps and the core watchdog, outside the Job Object
```

Each child runs in a kill-on-close Job Object, every cross-process call has a deadline, and every sidecar has a heartbeat. One UI Automation call on the owner's PC blocked for 38.7 minutes, and a blocked COM call cannot be cancelled in-process, so nothing that can hang shares a process with the microphone loop. Workers reach the desktop only through Eileen's own MCP tools, behind a permission gate, a path guard and an input lock. Apps Eileen opens are started by the shell and survive a restart of Eileen. The PC is the first connector; a phone, a home hub and other devices come later with the same permission tiers, job model and learning loop. The map is [ARCHITECTURE.md](ARCHITECTURE.md); the full specification is [docs/BUILD.md](docs/BUILD.md).

## How it compares

A survey dated 2026-10-11 found that the halves of Eileen already exist. A closed, single-vendor desktop app lets voice direct several coding tasks, but voice is a session you start and stop, and its computer use on Windows takes over the foreground. An open-source runner of parallel coding agents has local voice but does not see or drive the desktop. An agent with voice and background Windows control is its own harness and handles vendor sign-in itself. Learning from an agent's own work is well studied, and Eileen does not claim the idea. Its aim is the integration on Windows, judged on things that can be measured: listening at a low, measured idle cost, desktop state before any screenshot, deterministic app hooks, route-named workers on unmodified vendor CLIs, a learning loop you can see and erase, and a test suite that grows from real use. None of this is built yet. The survey, with names and sources, is [docs/research/2026-10-prior-art.md](docs/research/2026-10-prior-art.md); it predates the route decisions, so it lists more vendor routes than v0.1 has.

## What it builds on

| Layer | Project | License |
|---|---|---|
| Speech runtime | sherpa-onnx; ONNX Runtime through `ort` | Apache-2.0; MIT |
| Hearing | Silero VAD, Nemotron streaming ASR (default), Parakeet v3 (selectable), Smart Turn | MIT; NVIDIA Open Model License; CC-BY-4.0; BSD-2-Clause |
| Voices | Kokoro (default); Pocket TTS by Kyutai (default cloner, through sherpa-onnx); Chatterbox-Turbo (per voice, through `ort`) | Apache-2.0; code MIT, weights CC-BY-4.0; MIT |
| Echo cancellation | sonora (WebRTC AEC3 in Rust) | BSD-3-Clause |
| Faces | YuNet, for the video-editor benchmark | MIT |
| MCP | rmcp, the official Rust SDK | Apache-2.0 |
| Platform | the `windows` crate; zbus for D-Bus and quick-xml for the recent-files list on Linux | MIT or Apache-2.0; MIT; MIT |
| Video editor | Resolve's own MCP server on 21.1 or later; a Lua bridge run by Resolve's scripting interpreter as the fallback | bridge: Eileen's, MIT |
| Dictation | Handy's clipboard paste transaction for text insertion (WP5) and SpeakoFlow's cleanup rules and conformance suite (WP4), to be ported with attribution | MIT; MIT |

[NOTICE](NOTICE) holds the attributions for the native libraries, voices, models and ported code, and the licenses still to confirm before anything that downloads them ships, such as the Pocket TTS archive from sherpa-onnx and the Chatterbox-Nano export. Models will be downloaded and verified by `eileen setup` and are not in the repository. No GPL code goes into release binaries.

## Status

Updated 2026-10-25. **Eileen does not run yet**, and no feature work package (WP1 to WP9) has landed.

- **WP0 phase A**, in order: the CI bootstrap (#13, 2026-10-21), which retired the old Python scaffold; the contracts (#15, 2026-10-21); the six base crates (#73 and #75, 2026-10-25); the stubs (#96, 2026-10-25), which add the remaining crates under `crates/` as 30 documented stubs, 40 crates in all; the `cargo xtask` commands on the build guard and the simulator's skeleton (#98, 2026-10-25); then the guidance pull request that adds this README. Phase B adds the rest of the 30 repository checks, `tools/repo-tools`, the code map and the nightly, weekly and release workflows.
- **CI.** Hygiene (formatting, cargo-deny, cargo-machete) runs on every pull request. Clippy with warnings denied and the tests run on Linux and Windows, and the docs build on Linux, unless a pull request changes only docs or scenarios. `main` requires the aggregate `all-green` check and `pr-meta`.
- **Linux, WP10.** The first backends are on `main` in `eileen-shell-linux` and `eileen-apps-linux`, with shared D-Bus helpers in `eileen-dbus-linux`: machine facts, notifications, global shortcuts, session, power, opening files, recent files and media sessions, over D-Bus, the kernel's `/proc` and `/sys` files and XDG files. The D-Bus clients are tested in CI against fake services and the rest over fixture files, not yet on a real Ubuntu desktop, and no running binary uses them.
- **First live build: slice S1.** The real microphone and speakers, a mute key and tray, a console view, one fast-path command, one interruption, one brain answer and one Claude job, with latencies recorded. No desktop or app work starts until it passes on the owner's PC.
- **Specification.** [docs/BUILD.md](docs/BUILD.md) has eleven work packages, WP0 to WP10 (WP10 is Linux). It was revised after five reviews and four verification rounds, applied the owner's answers in full on 2026-10-21, and has been amended through decision D79 on 2026-10-25. The research is in [docs/research/](docs/research/).

Each work package records its progress in its living file in [docs/wp/](docs/wp/), which is more current than the status column in [docs/ROADMAP.md](docs/ROADMAP.md).

## Windows first, Linux following

v0.1 targets Windows 11 and is tested there. Everything that touches the operating system sits behind traits in `eileen-platform`, so a port adds `-linux` crates instead of changing the core, and every platform-neutral crate has built and passed its tests on Linux in CI on every pull request that changes code since the Rust workspace began (#13, 2026-10-21). Linux is WP10, with Ubuntu 26.04 LTS (GNOME on Wayland) as the reference desktop. On Wayland, input and screen capture need your consent through the desktop portals, and a few Windows behaviours, such as background input to a named window, degrade. The roadmap lists Linux as a v0.2 target; whether slice S1 on Linux ships with v0.1 or v0.2 is the owner's call. macOS is not planned for v0.1 or v0.2. The plan is [docs/LINUX.md](docs/LINUX.md).

## After v0.1

v0.2 plans the Codex, Grok and DeepSeek routes, phone and home connectors, a real worker sandbox and encryption at rest, the photo-editor request run for real, the Linux platform crates, signed installers and updates, routines and spoken conversation in other languages. v0.3 plans opt-in shared skills, multi-model debate and a local-only mode. Once v0.1 ships, a scout will review new models and tools, and a candidate is adopted only if it wins the same benchmarks. Not planned for now: macOS in v0.1 or v0.2, a polished settings GUI, a mobile app of Eileen's own, API keys other than DeepSeek's, handling vendor credentials, patching or replacing the Claude app, and bundling GPL or source-available code. The details are in [docs/ROADMAP.md](docs/ROADMAP.md).

## Get involved

The repository is public, and work is pushed as it happens, with tests.

- **Build it now.** Read [CONTRIBUTING.md](CONTRIBUTING.md) and set up with [docs/DEV-SETUP.md](docs/DEV-SETUP.md): Windows 11 for everything, or Ubuntu 24.04 or later for the platform-neutral and `-linux` crates. Then read [ARCHITECTURE.md](ARCHITECTURE.md) and the work package you take in [docs/BUILD.md](docs/BUILD.md) section 6, with its living file in [docs/wp/](docs/wp/). Coding agents read [AGENTS.md](AGENTS.md). Every build goes through `cargo xtask` on Rust 1.99.0, edition 2024, and a pull request merges when `all-green` and `pr-meta` pass.
- **Test it, once there is a build.** There is nothing to install yet. Testing opens with slice S1 and its testers' guide, `TESTING.md`, and testers will install from a nightly unsigned prebuilt zip, with no Rust toolchain needed. Most scenarios use your own Claude plan and spend its allowance. When something works or breaks, you will say "save that as a test", see exactly what would leave your PC after redaction, and submit it from your own GitHub account.
- **Report a vulnerability privately**, as [SECURITY.md](SECURITY.md) describes, not in a public issue.

## The name

The product is Eileen, chosen by the owner on 2026-10-11, and the choice is final ([D16](docs/DECISIONS.md#d16-the-product-is-named-eileen); [docs/NAMING.md](docs/NAMING.md)). In v0.1 you will be able to give your own assistant another name by voice; the product and the repository stay Eileen.

## Docs

- [ARCHITECTURE.md](ARCHITECTURE.md): the map, the processes, the invariants and the boundaries
- [docs/BUILD.md](docs/BUILD.md): the v0.1 specification, contracts and work packages
- [docs/wp/](docs/wp/): the living file of each work package, WP0 to WP10
- [docs/ROADMAP.md](docs/ROADMAP.md): milestones for v0.1, v0.2 and v0.3, and the non-goals
- [docs/DECISIONS.md](docs/DECISIONS.md) and [docs/OWNER-DIRECTION.md](docs/OWNER-DIRECTION.md): decision records, and what the owner asked for, by date
- [docs/LINUX.md](docs/LINUX.md): the Linux plan, Ubuntu first
- [CONTRIBUTING.md](CONTRIBUTING.md), [docs/DEV-SETUP.md](docs/DEV-SETUP.md) and [AGENTS.md](AGENTS.md): working on Eileen, for people and for coding agents
- [SECURITY.md](SECURITY.md): reporting a vulnerability, and what counts as one
- [docs/research/](docs/research/): the research reports, from October 2026 on
- [docs/RESEARCH.md](docs/RESEARCH.md): the first research brief, of 2026-09-19, partly superseded
- [docs/PLAYBOOK.md](docs/PLAYBOOK.md): how this repository is built, as a cited playbook another project can adopt
- [docs/NAMING.md](docs/NAMING.md): the name and where it appears
- [NOTICE](NOTICE): third-party code, voices and models, with their licenses

## License

MIT; see [LICENSE](LICENSE). Third-party attributions are in [NOTICE](NOTICE).
