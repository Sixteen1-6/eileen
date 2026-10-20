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
- **Claude is the default.** For now the brain and every worker route to Claude on the owner's plan. Codex is added later, once the owner installs and signs in to the Codex CLI; the routing design keeps a slot for it and for Grok and DeepSeek, but v0.1 must work end to end with Claude alone.
- **Public.** The repository is public as of 2026-10-20.
- **Eileen learns from every request. This is a main point of the product.** When someone asks for something hard and a worker finds a way to do it, that way is captured and built in, so the next time the same kind of request comes in it is faster, cheaper and more reliable, and eventually needs no model at all. Eileen keeps learning from how the user works (their apps, files, habits, phrasing and corrections) and gets better over time. Concretely, v0.1 must include:
  - recording what each job did (the request, what was on screen, every tool call, the result, and whether the user was happy, corrected it or undid it);
  - turning a successful job into a reusable, named skill that future workers load automatically;
  - matching new requests to known skills, and replaying a well-proven skill directly, checking each step and falling back to a worker the moment a check fails;
  - repairing a skill when it breaks, with its history kept so a bad change can be rolled back;
  - letting the user ask what Eileen has learned, correct it, or tell it to forget something;
  - keeping all of this on the user's PC, private by default, with any sharing of skills strictly opt-in and stripped of personal data;
  - never letting a learned skill skip a confirmation or raise a permission, and never turning text seen on screen into a skill without the user's say-so.
- **Connected fully to the computer, then to everything around the user.** v0.1 is the whole PC: every app, file, window, setting Eileen is allowed to touch, and the browser. After that Eileen reaches outward: the phone (messages, notifications, voice from anywhere), the home (lights, speakers, TV, thermostat and other devices, preferably through an open, local hub such as Home Assistant and standards such as Matter), other computers and devices the user owns on their own network, and earbuds or other microphones as ways to talk to it. The architecture must treat each of these as a pluggable connector with the same permission tiers, learning loop and job model as the PC, so adding one later is a new connector, not a redesign. Privacy rules get stricter as the reach grows: anything that senses the room (cameras, always-on mics beyond the PC) is opt-in per device and processed locally.
- **Dictation better than the paid tools.** Eileen includes a dictation mode as good as or better than the commercial voice-typing apps: speak and clean, punctuated text lands in whatever field has focus, in any app, with filler words and false starts removed and the user's vocabulary learned over time. Built on the best open-source work available (verify the current leaders), local and free.
- **Any voice.** Eileen's speaking voice is configurable. Kokoro stays the fast default; Chatterbox (or something better, if the research finds one that runs well on this PC) lets the user pick or clone a voice. Cloning uses only a voice the user has the right to use (their own, or one with consent), and any built-in watermarking stays on.
- **Best possible answer at conversational speed.** A thinking model that researches gives far better answers than a fast one, even for everyday questions like "what's the best place to eat near me". Eileen gets both:
  - Two speeds at once: the fast brain answers within about a second with something useful (never filler), while a deep worker (the strongest model, thinking on, web research) runs in parallel; Eileen speaks the better answer when it lands, and the user can keep talking or interrupt throughout.
  - Depth matched to the question: instant (no model), quick (fast model), considered (mid model, light thinking), deep (strongest model, thinking and research). The brain picks; "quick answer" or "think hard about this" overrides.
  - Research fans out: several searches run in parallel (reviews, hours, distance, prices) and one model combines them, so deep answers take tens of seconds, not minutes.
  - Context makes it right: location (opt-in, local), time, what the user likes and has done before (from the learning loop's user model), and what they are doing right now.
  - Short when spoken, rich on screen: two sentences aloud, the full list with links, reasons and actions on the HUD.
  - Faster every time: the learning loop remembers which sources gave the best answers for each kind of question, slow-changing results are cached with an expiry, and the user's follow-ups ("closer", "I loved that place") are kept.
  - Optional anticipation (off by default, because it spends plan limits): looking things up before the user asks.
- **Use Claude's official extension points to make the Claude app better with Eileen.** Claude now supports mods (changes to its interface and behaviour such as panes, panels, status lines, toasts, slash commands and hooks), plus plugins, skills and hooks in Claude Code. Where these make Eileen better, use them: for example Eileen's jobs and status inside the Claude app, Claude sessions reporting progress and approval requests to Eileen, Eileen's learned skills available to every Claude session. Only through the supported mechanisms, never by patching or replacing the app, so the owner keeps getting the latest Claude updates.
