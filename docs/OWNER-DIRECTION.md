# Owner direction

What the owner has asked Eileen to be, in dated entries. Design documents (BUILD.md, ARCHITECTURE.md, ROADMAP.md) must satisfy every entry here. When an entry conflicts with an older decision in DECISIONS.md, the newer entry wins and a decision record says so.

## 2026-10-11

- **Name.** The product is Eileen. That is final.
- **Always listening.** You talk to the computer any time the mic is not muted. No wake word is required. Mute is one keypress or one click away.
- **Anything a human can do on a computer.** Open and drive any app, go to a specific video, edit in pro apps. Benchmark requests:
  - "Open the video editor, go to 15:30 and make the face of the girl on the right less blurry", and it actually does it in DaVinci Resolve.
  - "Open Photoshop, open the image where I was editing the girl at the beach, and make her face less blurry."
  - "Launch three agents to do X", and they run in parallel.
  - Writing code by speaking.
- **Subscriptions, not API keys.** Models run through each vendor's official, signed-in tool on the owner's existing plans (Claude Code for Claude, Codex for OpenAI). Every model goes to its own harness and to what it is best at. Workers are named by route: Claude, Codex, Grok, DeepSeek. Local models (Ollama and similar) are optional support, never required. Small local speech models are fine.
- **Fast, and in Rust.** The always-on runtime is Rust. The code is spotless: formatted, linted with warnings as errors, tested, and license-checked on every pull request.
- **Open source, on GitHub.** Anyone can download it. Work is pushed as it happens, with tests.
- **Not already done.** Check the prior art honestly and build something clearly better than it.

## 2026-10-20

- **API keys.** No Gemini API key. Gemini is out until a subscription route exists that Google permits for tools like Eileen. A DeepSeek API key is acceptable for now; it is the only key.
- **The feel.** Eileen should feel like talking to a brilliant assistant in the room while you work on something hard. Concretely:
  - It understands natural, half-finished speech and context ("make that bigger", "no, the other one", "go back to what we tried an hour ago").
  - It knows when you are talking to it and when you are not, and when you have finished a thought versus paused mid-sentence.
  - You can interrupt it at any moment and it stops at once, then picks up from what you actually heard.
  - It talks while it works: short, useful narration of what it is doing and what it found, never filler, never a wall of text read aloud.
  - It works on several things at once and keeps you oriented ("the render is at 60 percent; the test agent found two failures").
  - It thinks with you on hard problems, not only executes commands: it can explore, simulate, compare options and explain.
  - A visual layer (a heads-up display or overlay on the screen) shows what it is listening to, doing and watching, since there are no holograms.
- **Signing in.** The owner signs in to each vendor CLI themselves. Eileen never runs a login flow and never reads vendor credential files.
