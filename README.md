# Eileen

An open-source, voice-first AI layer for Windows 11. You talk to your PC while you work; Eileen knows what is on your screen, drives any app, runs coding and computer-use jobs in parallel on your own Claude plan, and learns from every request so the next one is faster.

## What it is

You leave it running and talk to it. There is no wake word: it listens whenever the microphone is not muted, works out when you are talking to it, and mute is one key or one click away. It knows which monitor you are looking at, which window has focus and what you have selected. Simple things happen at once with no model call. Bigger things become jobs that run while the conversation continues, and a small on-screen layer shows what it hears, what it is doing and what it is waiting for. Things you can say:

- "Open the video editor, go to 15:30 and make the face of the girl on the right less blurry."
- "Launch three agents to fix the flaky login tests, and tell me when one passes."
- "Play the timelapse on YouTube and go to 3:20."
- "What am I looking at?"
- "Stop." (mid-sentence; it stops at once and remembers what you actually heard)
- "What have you learned?" and "forget that".
- "I want to call you Nova now."
- "Learn a new voice." (with the voice owner's own spoken consent)

Hold a key in any app to dictate: clean, punctuated text lands where your cursor is, locally and free.

## How it compares

The halves of Eileen exist and we say so. The ChatGPT desktop app lets voice direct several Codex tasks, but for one vendor, in a voice session you start and stop, and its Windows computer use takes over the foreground. Paseo runs Claude, Codex and others in parallel with local voice, but does not see or drive the desktop. Hermes Agent has voice and background Windows control, but it is its own harness and handles vendor sign-in itself. What Eileen adds is the integration on Windows done carefully: always-on local listening at near-zero idle cost, a desktop state object instead of screenshots, deterministic hooks into the apps you use, workers on the vendor's own unmodified CLI that Eileen never signs in for you, a learning loop you can see and erase, and a test suite that grows from real use on many people's PCs. The survey with sources is in [docs/research/2026-10-prior-art.md](docs/research/2026-10-prior-art.md).

## Principles

1. **You are the supreme router.** Policies are defaults; a spoken override wins at once.
2. **Conversation never blocks on work.** The core owns the microphone and treats every Claude session as an async job.
3. **Subscriptions, not API keys.** Models run through each vendor's own signed-in CLI on your plan. You sign in yourself; Eileen never runs a login and never reads credential files. Claude is the route in v0.1; Codex, Grok and DeepSeek follow. Workers are named by route.
4. **Deterministic first, vision last.** App scripting and UI Automation before screenshots and the mouse.
5. **Reversible things happen, consequential things ask.** Sending, deleting, pushing and spending confirm first, and only your own click, key or spoken action word can say yes; credentials and payments are never Eileen's.
6. **It learns, locally.** Every request can teach it; you can ask what it learned and make it forget. A learned skill never skips a confirmation.
7. **Engineered to be trusted.** Rust core, every risky part in a process that can be killed, a deterministic simulator for the core, and every convention checked by CI.

## Architecture at a glance

```
                     you (speak, click, type)
                                |
+---------------------------------------------------------------+
| eileen.exe (Rust core, always on)                             |
|  audio, mute, echo cancellation | Core::step: turns, intent,  |
|  jobs, routing, permissions, learning | store, bus, supervisor|
+---------------------------------------------------------------+
     |              |               |              |           |
 eileen-speech  eileen-desktop  eileen-apps    eileen-hud   claude -p
 VAD, ASR,      UI Automation,  Resolve,       overlay      brain and
 end of turn,   input, capture, media, Office, and strip    workers on
 TTS, voices    app launch      recent files                your plan
```

Each box below the core is a separate process the core supervises and can restart, so a hung UI Automation call or a crashed speech model never takes the microphone loop down. The map is in [ARCHITECTURE.md](ARCHITECTURE.md) and the full v0.1 specification in [docs/BUILD.md](docs/BUILD.md).

## What we build on

| Layer | Project | License |
|---|---|---|
| Speech | sherpa-onnx with Silero VAD, Nemotron streaming ASR, Smart Turn, Kokoro; Kyutai Pocket TTS and Chatterbox (through ONNX Runtime) for cloned voices | Apache-2.0, MIT, BSD-2, NVIDIA Open Model License, CC-BY-4.0 |
| Audio | cpal, sonora (WebRTC AEC3 in Rust) | Apache-2.0, BSD-3 |
| MCP | rmcp, the official Rust SDK | Apache-2.0 |
| Windows | the `windows` crate, UI Automation, DWM, SMTC, COM | MIT or Apache-2.0 |
| Workers | the claude CLI, unmodified, signed in by you | vendor terms |
| Resolve | Resolve's own scripting interpreter, through a small Lua bridge | ours, MIT |
| Dictation | ideas and code from Handy and SpeakoFlow, with attribution | MIT |

## Status

| Item | State |
|---|---|
| Research | Done, October 2026 ([docs/research/](docs/research/)) |
| v0.1 specification | [docs/BUILD.md](docs/BUILD.md), ten work packages, revised on 2026-10-20 after five reviews, a verification round and the owner's answers to its open questions |
| Rust workspace | Being built (the old Python scaffold is retired) |

Milestones are in [docs/ROADMAP.md](docs/ROADMAP.md).

## Get involved

The repository is public. Two ways to help:

- **Test it on your PC.** Install it, use it for real, and when something works or breaks, say "save that as a test". You see exactly what would leave your PC before anything is shared, and you share it from your own GitHub account. Every shared scenario becomes a permanent regression test. The testers' guide is `TESTING.md` (it lands with the first live slice, so you can share scenarios from the first build you can run).
- **Build it.** Read [CONTRIBUTING.md](CONTRIBUTING.md), the work packages in [docs/BUILD.md](docs/BUILD.md), and [docs/DEV-SETUP.md](docs/DEV-SETUP.md). Coding agents read `AGENTS.md`.

Maintainer: Shubhanshu Pokharel ([Sixteen1-6](https://github.com/Sixteen1-6)).

## The name

The product is Eileen, chosen by the owner on 2026-10-11 (decision D16 in [docs/DECISIONS.md](docs/DECISIONS.md)). Your own assistant can answer to any name you give it; the product and the repository stay Eileen.

## Docs

- [docs/BUILD.md](docs/BUILD.md): the v0.1 specification, contracts and work packages
- [ARCHITECTURE.md](ARCHITECTURE.md): the map, the invariants, the boundaries
- [docs/ROADMAP.md](docs/ROADMAP.md): milestones
- [docs/DECISIONS.md](docs/DECISIONS.md): decision records
- [docs/OWNER-DIRECTION.md](docs/OWNER-DIRECTION.md): what the owner asked for
- [docs/research/](docs/research/): the October research reports
- [docs/DEV-SETUP.md](docs/DEV-SETUP.md): getting a Windows machine ready to work on it
- [docs/PLAYBOOK.md](docs/PLAYBOOK.md): how this repository is built, as a cited playbook any project can adopt

## License

MIT.
