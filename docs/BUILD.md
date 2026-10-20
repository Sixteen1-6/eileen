# Eileen v0.1 build specification

Written 2026-10-20 by the architect pass of the design workflow. This is the document parallel engineer agents build v0.1 from. It turns [OWNER-DIRECTION.md](OWNER-DIRECTION.md) (binding) and the twelve October research reports into one design, one Cargo workspace, one set of contracts and ten work packages.

How to use it:

- Engineers: read section 0, then the work package you were given in section 6, then every section that package names. The contracts in section 3 are the only interface between packages. If a contract here is wrong, say so in your pull request and propose the fix; do not quietly diverge.
- Reviewers: section 4b lists every check CI runs, and section 7 lists what is still the owner's call.
- Numbers come from the research reports, which say how and under what load each was measured. Almost every measurement on the owner's PC was taken under heavy contention, so read them as pessimistic.

Reports this spec is built on (all present on 2026-10-20, in `docs/research/`): prior-art, subscription-harnesses, app-hooks, brain-latency, voice-stack, computer-use, rust-architecture, learning-loop, dictation-and-voices, claude-mods, engineering-practice, observability.

## 0. Summary

v0.1 is one always-on Rust program for Windows 11 that listens whenever the microphone is not muted, understands what the user means, and either acts at once on the PC or starts Claude jobs that run in parallel while the conversation continues. Everything it does is recorded locally so the next request of the same kind is faster, and everything the user and the owner's friends try can be saved as an automated test.

The shape, in one paragraph. A core process (`eileen.exe`) owns the microphone, mute, the event bus, the job registry, permissions, learned skills and a supervisor. Everything that can hang or crash runs in a supervised child process the core can kill: speech models (`eileen-speech.exe`), UI Automation and input (`eileen-desktop.exe`), app scripting such as the Resolve bridge (`eileen-apps.exe`), the on-screen layer (`eileen-hud.exe`) and every Claude session. The brain is a warm `claude -p` session on the owner's plan with a hot spare; common commands skip it entirely through a deterministic fast path. Long or hands-on work goes to Claude workers that drive the PC through Eileen's own MCP tools. Claude is the only route in v0.1; Codex, Grok and DeepSeek are designed slots behind the same trait.

Build order, thin slice first:

| Wave | Packages (at most two compiling at once) | What exists at the end |
|---|---|---|
| W0 | WP0 foundation | Workspace, contracts, quality gates, CI lanes, simulator skeleton, tracing, scenario schema, every crate as a documented stub |
| W1 | WP1 core logic, WP3 Claude harness | The core decides everything in simulation; Claude brain and workers run from Rust |
| W2 | WP2 runtime, WP4 voice | **Slice S1:** speak, the brain answers or starts a Claude job, Eileen speaks the result |
| W3 | WP5 desktop control, WP7 intent, settings and learning | Open any app, move windows, computer-use jobs; the learning loop and settings by voice |
| W4 | WP6 app adapters | Resolve, YouTube and media, Office, Explorer, recent files |
| W5 | WP8 HUD, shell and Claude integrations | The visual layer, tray, hotkeys, the `eileen` Claude plugin |
| W6 | WP9 end to end, benchmarks and the test fleet | The owner's benchmarks as timed scenarios; friends' PCs as a test fleet |

## 1. The target experience

Each demo below is a script a tester can follow. "Under the hood" names the mechanism and the section that specifies it. Every demo (E1 to E17) becomes at least one scenario in `scenarios/` (section 4.8), so these scripts are also the first tests.

Timing words: "at once" means under about 0.6 s from the end of speech to the first sound, which only the fast path and cached phrases reach. "Quickly" means about 2 s for a brain turn on a quiet PC (measured 1.0 to 1.5 s to first text with connectors off, plus 0.5 to 1.3 s to synthesize the first clause). Deep answers take tens of seconds and arrive while the user keeps talking. Section 4.3 has the per-stage budget.

### E1. Always listening, mute one key away

| You do or say | What happens | Under the hood |
|---|---|---|
| Sign in to Windows | Eileen starts from the Start menu entry. The HUD strip shows the mic as listening, and "brain warming up" until the Claude session is ready (cold start measured 39 to 48 s on a loaded PC). Fast-path commands work immediately | 2.1, 2.3 |
| "Eileen, what time is it?" | Spoken answer at once, no model | fast path, 2.4 |
| (to a friend) "Want coffee?" | Nothing spoken. The HUD shows the words greyed as heard but not for Eileen | addressee gate, 2.8 |
| Press the mute key (default Ctrl+Alt+M) or click the mic on the HUD | The capture stream itself stops; the Windows mic indicator goes off; the HUD mic turns red | 2.8, 2.9 |
| "Eileen, are you there?" while muted | Nothing. Muted means nothing is captured | invariant 9 in 4.7 |
| Press the mute key again | Listening resumes; a short earcon | |

### E2. Open any app

| You say | What happens | Under the hood |
|---|---|---|
| "Open Premiere" | Premiere launches; "Opening Premiere Pro." from the phrase cache | fast path, app index, `desktop_launch_app` (2.6) |
| "Open DaVinci" | "DaVinci Resolve or DaVinci Control Panels?" (the fuzzy matcher scored them equal, so it asks instead of guessing) | margin rule, 2.6; one clarifying question, 2.2 |
| "Resolve" | Resolve launches. The choice is remembered as a phrasing fact, so next time "open DaVinci" launches Resolve directly | user model, 2.10 |
| "Put it on the left screen" | Resolve's window moves to the left monitor's work area | `desktop_window` snap, fast path |

### E3. A specific video, at a time

| You say | What happens | Under the hood |
|---|---|---|
| "Play the Mount Rainier timelapse on YouTube" | If the video is known (played before, in the conversation, or in the recent-work index), its watch URL opens in the default browser at once. If not, a Claude worker opens the YouTube results page in the user's own browser, reads the results from a screenshot, and opens the matching video. Narration: "Found it, playing." | `apps_open_url`, worker computer use, 2.7 |
| "Go to 3:20" | Playback jumps to 3:20 through the system media controls, no keystrokes | `apps_media` seek over SMTC, 2.7 |
| "Pause" / "Play" | Instant, through SMTC | fast path |
| Second time, another day: "play that timelapse at 1:10" | Template match on the learned `youtube-play-at` skill; the URL opens at `&t=70s` with no model call | learning ladder level 4, 2.10 |

A preroll or mid-roll ad can follow a load or a seek; Eileen waits for the SMTC session to come back with the expected title before saying it worked (app-hooks report section 5).

### E4. "What am I looking at?"

| You say | What happens | Under the hood |
|---|---|---|
| "What am I looking at?" | "Resolve, the Color page of 'Beach Edit', on your left monitor. Nothing is selected." Answered from DesktopState in the brain's context block, with no screenshot | DesktopState, 2.6; context blocks, 2.3 |
| "What's in this picture?" (a photo is open) | The brain calls `desktop_look`, gets one downscaled screenshot, and describes it | 2.3 |
| "Copy the second paragraph into my email" | A worker job reads the page and the email field through the desktop tools and inserts the text | 2.6 |

### E5. "Launch three agents to ..."

| You say | What happens | Under the hood |
|---|---|---|
| "Launch three agents to fix the flaky login tests in the eileen repo" | "Starting three Claude workers on flaky login tests." Three git worktrees and branches are created, three jobs start, and the HUD shows three rows. If the plan's five-hour window is above 80 percent, Eileen asks first | fan-out, 2.5; plan-limit awareness, 2.5 |
| (keep working; say nothing) | Short narration only for the focus job, at most one line every 20 s: "Agent two found the race in the session cache." | narration, 2.5 |
| "How are they doing?" | "Two of three pass the tests; agent three is still running them." | jobs block, 2.3 |
| "Keep agent two's fix" | Agent two's branch is merged into the user's branch (local, reversible); the other worktrees are removed after a one-sentence confirmation because they hold unmerged work | tiers, 2.12 |

### E6. Coding by voice

| You say | What happens | Under the hood |
|---|---|---|
| "In the eileen repo, add a json flag to the status command and run the tests" | A Claude worker job in that repo. Approvals for anything beyond the worktree come back by voice | worker layer, 2.5 |
| Hold the dictation key in VS Code and say "if result is none return early" | The text lands at the caret in code style (no capital, no period), local only | dictation, 2.8 |
| "Eileen, rename that variable to session id" | A worker job edits the file; the change is in git and can be undone | 2.5 |

### E7. The Resolve benchmark, done for real

"Open Resolve, go to 15:30 and make the face of the girl on the right less blurry."

| Step | How | Rung |
|---|---|---|
| Resolve open and scriptable | If `Resolve.exe` is not running, launch it (tier 1). The bridge's `hello` confirms scripting works; if External scripting is not set to Local, Eileen says exactly which preference to change | apps, 2.7 |
| Go to 15:30 | The bridge reads the timeline's start timecode and frame rate; Rust computes the absolute timecode (01:15:30:00 for a 01:00:00:00 start) and calls `SetCurrentTimecode`; the playhead is read back | scripting |
| The girl on the right | Switch to the Color page; export the current frame as a still; the local YuNet face detector finds faces; take the right-most; if the detector is unsure, the worker's model looks at the still | scripting plus local vision |
| Less blurry | A Claude worker adds a corrector node with a circular window at the face box and sharpening, through a prepared DRX grade where its format allows the window to be placed, otherwise through the Color page UI with screenshots and the input lock. Narration: "Adding a sharpen window on her face." | scripting, then computer use |
| Track | Track forward with the Color page's tracker through a keystroke, foreground held with the input lock | input |
| Check | Export the frame again; sharpness inside the face box (variance of the Laplacian) must rise by at least 15 percent; otherwise the worker tries once more, then reports honestly | local vision |
| Learn | The trajectory becomes a draft skill `resolve-sharpen-face` with slots for the time and which face | 2.10 |

Measured as benchmark B1 (section 4.7) on the owner's PC. The second run must use fewer model turns and less time than the first.

### E8. The Photoshop equivalent (designed; Photoshop is not installed on the owner's PC)

"Open Photoshop, open the image where I was editing the girl at the beach, and make her face less blurry." The design is in section 2.7: candidates from Photoshop's recent files over COM plus the recent-work index, ranked by name and recency, then by embedded thumbnails if two remain close; one confirming sentence; open over COM; duplicate the layer (keeps it reversible); detect the face locally; feathered elliptical selection; Smart Sharpen on the duplicate. Neural Filters and Generative Fill spend Adobe credits and are tier 2. The code is written and unit-tested against a fake COM target; the real-app scenario is marked `requires = ["photoshop"]` and is skipped until someone with Photoshop runs it on the test fleet.

### E9. Many jobs, one conversation

| You say | What happens |
|---|---|
| "How's the render?" while a coding job and a render job run | "The render is at 60 percent; the test agent found two failures." Pronouns resolve against the jobs block (2.5) |
| (Eileen is mid-sentence) "Stop." | Playback ducks within one 32 ms window and stops by the third; the transcript records only what was actually played; nothing after "Stop" is answered |
| "Actually, tell the test agent to only look at the parser" | The instruction is steered into that job's session (interrupt, then a new user turn) |
| "Cancel the render job" | Cancelled; the job ends in exactly one terminal state |
| (chime) "Claude wants to run git push on the login branch. Allow?" "Yes." | The approval came from the microphone during an approval window; the worker resumes. "No" denies, silence for 20 s denies |
| "Send this one to a stronger model instead" | Reroute: the job is cancelled and restarted on the deep model with the old transcript as context |

### E10. Thinking together, and the best answer at conversational speed

| You say | What happens |
|---|---|
| "What's the best place to eat near me?" | The brain answers quickly with something useful from what it knows ("For tonight, the ramen place you liked on 5th is open until ten."), and at the same time starts a deep research job (strongest model, thinking on, parallel web searches). When it lands, two sentences are spoken ("Better answer: ...") and the full list with links, hours, distance and reasons appears on the HUD. If the user is talking, it goes to the HUD only |
| "Closer." / "I loved that place." | Kept as user-model facts; the next answer uses them |
| "I'm trying to decide between SQLite and a flat-file store for skills; think it through with me" | Quick take spoken, a considered worker explores both (reads the code, writes a small benchmark in a scratch worktree, compares), and Eileen talks through the trade-offs while it works. "Quick answer" or "think hard about this" overrides the depth |

### E11. It learns

| You say | What happens |
|---|---|
| (E7 again, a day later, "go to 42:10 and sharpen the guy on the left") | The worker loads the learned skill natively, so there is no exploration: fewer turns, no dead ends. Measured as B8 |
| "What have you learned?" | The newest two or three items, one sentence each; the HUD shows the full list with level, success rate and time saved |
| "Show me how you do that" | The HUD shows the skill's steps and checks, each line colored by where it came from |
| "Forget that" | "Forget the Resolve sharpen skill and its four recordings?" "Yes." The skill, its versions and its evidence are deleted, and a tombstone stops the librarian relearning it |

### E12. Dictation better than the paid tools

Hold Right Ctrl (configurable) in any app and speak; words appear in the HUD while speaking; on release, clean punctuated text lands at the caret: fillers, stutters and "scratch that" removed, "at 3, no, 4 pm" becomes "at 4 pm", code fields get literal text, chat drops the final period. The user's own clipboard is preserved. "Undo that" removes the last insertion. Corrections the user makes afterwards teach the vocabulary. An optional Claude "scribe" polishes tone when the user turns it on (section 2.8).

### E13. Any voice, including a friend's

| You say | What happens |
|---|---|
| "Eileen, learn a new voice" | "Whose voice? They need to be here." "Sam's." Sam reads a short consent sentence and a passage (about 30 s). A same-speaker check compares the two; the model's terms are shown once; the profile "Sam" is saved on the PC only |
| "Use Sam's voice for announcements" | Announcements use Sam's cloned voice (Pocket TTS); conversation stays on the fast default voice if the PC lacks headroom, and Eileen says so once |
| "Back to normal" | Default voice |
| "Delete Sam's voice" | Reference clip, consent clip, caches and profile removed |

No watermark is applied by default on any engine; watermarking saved or exported audio is an opt-in setting (owner decision of 2026-10-20).

### E14. Customise anything by saying it

| You say | What happens |
|---|---|
| "Eileen, I want to call you Nova now" | Eileen checks the name first: not a common word, not a household name it knows, and its own speech recognizer hears "Nova" correctly in three synthesized sentences. Then: "Okay, I'm Nova now. Say undo that to go back." The answer-to name, the addressee cue, the recognition hint and how it refers to itself all change |
| "Talk less" / "always answer in one sentence" | Verbosity setting changes; confirmed in one sentence |
| "Stop announcing progress on renders" | That announcement row changes |
| "Put the HUD bottom left and make it smaller" | HUD moves and scales |
| "Change the mute key to F8" | Eileen tests that F8 can be registered (another app may own it) before applying |
| "Always let Claude move windows without asking" | Window moves are already reversible and silent; Eileen says so instead of adding a rule |
| "Undo that" | Reverts the last settings change; settings are versioned locally |
| "Save this as editing mode" / "switch to work mode" | Profiles |

Nothing a setting does can let Eileen enter credentials, make payments, or skip a confirmation for a consequential action without an explicit, logged decision (section 2.11).

### E15. Something went wrong, save that as a test

| You say | What happens |
|---|---|
| "Eileen, that went wrong" or press Ctrl+Alt+Shift+B | The last ten minutes are frozen into a local incident: event log, spans, inputs (replayable), process table, last DesktopState |
| "Save that as a test" | The last request becomes a scenario file: what was said (as text), the starting state, every step Eileen took, and checkable expectations inferred from what changed. If it failed, Eileen asks once "what should have happened?" and stores the answer |
| `eileen scenario share last` | A preview shows exactly what would leave the PC after redaction; on approval, `eileen scenario submit` opens a pull request from the user's own GitHub account. CI replays it in simulation on every pull request from then on |

### E16. The feel

| The owner asked for | Mechanism | Proven by |
|---|---|---|
| Understands half-finished speech and context ("make that bigger", "no, the other one", "go back to what we tried an hour ago") | Turn end needs Smart Turn plus transcript continuation cues plus a silence cap; the brain gets the focus history, the last referenced element and the episodic ledger | sim invariants; scenarios `feel/*` |
| Knows when it is addressed and when a thought is finished | Local addressee gate (name, follow-up window, command grammar); ambiguous speech goes to the HUD, not to action | room-talk scenario, at least 9 of 10 ignored |
| Stops at once when interrupted, resumes from what you heard | Duck at the first VAD window, stop by the third (measured parts give 70 to 110 ms); played samples mapped to text; the brain is told exactly what was heard | sim invariants 2 and 3; B5 |
| Talks while it works, briefly | Workers mark narration lines; at most one spoken line every 20 s for the focus job; never filler | narration tests |
| Several things at once, keeps you oriented | Job registry, jobs block, announcement policy | E9 scenarios |
| Thinks with you | Depth tiers, deep workers in parallel, HUD for detail | E10 |
| A visual layer | HUD overlay and strip; visible intent before clicks | E17 below |

### E17. The visual layer

The HUD shows, in a corner the user chooses: the mic state and level; the words being heard, greyed when not addressed to Eileen; what Eileen is saying; each job with route, name and progress; pending approvals with Allow and Deny; what Eileen is watching (the app in focus and the last DesktopState summary); learning, recording and off-the-record indicators; plan usage when it is high. Before Eileen clicks or types on the user's screen, the HUD outlines the target for 400 ms and names it, so "no, the other one" works mid-action. The HUD excludes itself from screen captures.

### Roadmap spine: what moves

ROADMAP.md keeps its milestones M1 to M6 as the spine, adding an M0 for the foundation WP0 builds. The October direction changes their content and pulls some later items into v0.1:

| Item | Was | Now | Why |
|---|---|---|---|
| HUD with mute | v0.2 | v0.1, in M3 | The owner asked for mute one click away and a visual layer as part of the feel; the prior-art report lists a status surface as table stakes |
| Resolve hook | v0.2 | v0.1, in M2 | The owner's first benchmark is a Resolve request; the Lua bridge already runs under a Rust supervisor on the owner's PC |
| Learning loop | not planned | v0.1, in M4 | The owner's main point (2026-10-20) |
| Dictation | not planned | v0.1, in M3 | Owner direction 2026-10-20 |
| Voice cloning with consent | not planned | v0.1, in M3 | Owner direction 2026-10-20 |
| Settings by voice, rename | not planned | v0.1, in M4 | Owner direction 2026-10-20 |
| Community scenario corpus and test fleet | v0.3 evaluation loop | v0.1, in M6 and WP0 | The owner's friends test on their own PCs now |
| Claude plugin (handoff, display mod) | not planned | v0.1, opt-in, in M4 | Owner direction on Claude's extension points |
| Phone channels (Telegram, ntfy) | v0.1 M5 | v0.2, as the first outward connector | The owner wants the whole PC first, then reach outward through connectors |
| Codex, Grok, DeepSeek routes | v0.1 M4 (Codex) | slots in v0.1, built in v0.2 | Claude is the only v0.1 route; Codex is not installed |
| Gemini | v0.2 | removed | No Gemini key; no permitted subscription route |
| Linux | non-goal | v0.2 target; neutral crates tested on Linux from day one | Owner direction "Windows first, not Windows only" |

### Proposed ideas: adopted now, scheduled later

From the "Proposed 2026-10-20" section of OWNER-DIRECTION.md:

| Idea | v0.1 | Later |
|---|---|---|
| Undo for everything | Adopted for what Eileen fully controls: window moves, dictation insertions, settings, skills (rollback), job worktrees. Each action is journaled with its inverse; "undo that" reverses the last one or says honestly that it cannot | App-level undo inside pro apps (v0.2) |
| Visible intent | Adopted: HUD outline and label 400 ms before any click or keystroke on the user's screen | "Show me first" whole-plan preview (v0.2) |
| Teach by showing | Scheduled | v0.2: "watch me do this" through Eileen's own hooks, same consent rules |
| Discoverability | Adopted: "what can you do here?" by live introspection; two or three HUD suggestions from the same data; a short first-run spoken tour from cached phrases | Richer tour (v0.2) |
| Focus awareness | Adopted: quiet during calls, presentations and full-screen apps (Windows notification state and the microphone-in-use registry), announcements held and summarised afterwards | |
| Plan-limit awareness | Adopted: rate-limit events from the stream drive a HUD chip, warnings before fan-out, and routing small things to the fast model | |
| Accessibility | Adopted as a design rule: every task works with no mouse; speech rate and HUD scale are settings; everything on the HUD is also available by voice and from `eileen status` | Screen-reader exposure of the HUD (v0.2) |
| Memory you can ask about | Adopted minimally: the episodic ledger records app, window title and time of focus changes for 14 days, locally, searchable by the brain; an off-the-record mode records nothing | Page-content recall (v0.2) |
| Routines | Scheduled | v0.2 |
| Shared skills, opt-in | Scheduled | v0.3: signed packs, untrusted until verified locally |
| Languages | Partly: multilingual dictation (Parakeet v3, on demand) | Conversation in other languages (v0.2) |
| One-step install | Partly: `eileen setup` downloads and verifies models, checks the Claude CLI sign-in without touching credentials, creates the Start menu entry and says hello | winget and a signed MSI (v0.2, needs a signing certificate) |

## 2. The architecture as it will be built now

### 2.1 Processes

```
 mic, speakers                      hotkeys, tray                         HUD (eileen-hud.exe)
      |                                  |                                      ^  JSON lines (stdio)
      v                                  v                                      |
+--------------------------------------------------------------------------------------------------+
| eileen.exe (core, always on; links nothing that can hang)                                         |
|  audio thread: WASAPI capture and render, AEC (sonora), ring buffer, mute, ducking                |
|  Core::step (sans-IO): turns, addressee, intent resolver, jobs, router, permission gate,          |
|    approvals, announcements, barge-in, journal, learning matcher                                  |
|  drivers (tokio): store (SQLite), bus, loopback HTTP (hooks, state, control), dispatcher,         |
|    supervisor, learning recorder and librarian scheduler, settings, flight recorder               |
+--------------------------------------------------------------------------------------------------+
   | named pipe (frames)    | MCP stdio          | MCP stdio          | stream-json stdio      ^ named pipe
   v                        v                    v                    v                        | (per-session token)
 eileen-speech.exe     eileen-desktop.exe    eileen-apps.exe     claude -p (brain, spare,   eileen-shim.exe
 VAD, ASR, end of      UIA, capture, input,  SMTC, Office COM,   workers, librarian,        (one per Claude
 turn, TTS, voices,    app index, windows,   Explorer, recents,  scribe)                    session; serves the
 speaker check,        DesktopState,         Resolve Lua bridge  |                          `eileen` MCP tools)
 text embedding        text insertion        (fuscript.exe child) +-- HTTP hooks to the loopback endpoint
 eileen-guard.exe (outside the Job Object): minidumps of crashed or hung processes, restarts a hung core
```

| Process | Contains | What a failure costs | Transport to the core |
|---|---|---|---|
| `eileen.exe` | Everything that decides, plus audio I/O and the ring buffer | Everything, so it links no UIA, no app COM, no ONNX Runtime and no web view; `panic = "unwind"` so a panicking task fails one job, not the process | n/a |
| `eileen-speech.exe` | sherpa-onnx and ort sessions: Silero VAD, Nemotron streaming ASR, Smart Turn, Kokoro, Pocket TTS, speaker embedding, the intent embedding model | Recognition pauses; the core keeps capturing into its ring buffer and replays it after the restart; the HUD says "speech restarting" | Binary frames over a named pipe (section 3.3.7) |
| `eileen-desktop.exe` | UI Automation on its own MTA thread with deadlines, capture, input, the app index, window management, DesktopState, text insertion | Grounding pauses; computer-use calls fail with a typed error and the job falls back or waits | MCP over stdio |
| `eileen-apps.exe` | One STA thread per COM target, SMTC manager, recent-work index, the Resolve bridge as its child, YuNet and the sharpness metric | App hooks pause; desktop tools still work | MCP over stdio |
| `eileen-hud.exe` | The overlay and the clickable strip | The overlay disappears and restarts; listening and work continue | JSON lines over stdio |
| `claude -p` sessions | Brain (warm), hot spare, one per worker job, the librarian, the optional dictation scribe | One job or one brain turn; the brain spare takes over | stream-json over stdio, plus HTTP hooks |
| `eileen-shim.exe` | A small rmcp stdio server that each Claude session spawns from its generated MCP config; it forwards every tool call to the core's dispatcher over a named pipe | One session's tools | Named pipe with a per-session token |
| `eileen-guard.exe` | Embark `minidumper` server, core heartbeat watchdog | Crash dumps are not written | Local socket |

Rules (from the rust-architecture and observability reports):

1. Every child starts through `eileen_supervisor` (process-wrap 10.0.1 with a kill-on-close Job Object, `CREATE_NO_WINDOW` and kill-on-drop). Clippy bans `Command::new` everywhere else. The guard is the one child started outside the Job Object, so it survives the core.
2. Every call to another process has a deadline, and every sidecar has a heartbeat. A UIA call that passes its deadline (default 5 s) gets the sidecar hang-dumped by the guard, killed and restarted, and the caller gets a typed error. One UIA call on this PC blocked for 38.7 minutes, far past UIA's own 20 s timeout, which is why this rule exists.
3. Sidecars and Claude sessions are long-lived. Spawning per request costs seconds on this PC (bare Python 4.5 to 6.9 s, claude cold start 11 to 111 s under load).
4. Stdout carries the protocol; stderr carries logs, which the supervisor forwards into the core's tracing.
5. Separate binaries, so an agent working on UIA never links the speech stack and a HUD change relinks only `eileen-hud`.
6. At startup each process opts out of EcoQoS execution-speed throttling (`SetProcessInformation` with `ProcessPowerThrottling`), and audio threads register with MMCSS. Without the opt-out one Kokoro run took 27.8 s instead of 2.4 s.

### 2.2 One request, end to end

1. **Audio.** The core's audio thread captures 48 kHz from WASAPI in 10 ms buffers, resamples to 16 kHz mono, runs sonora's AEC3 with the exact PCM Eileen is playing as the far-end reference, and writes 32 ms frames (512 samples) to the ring buffer and the speech pipe. Muted means the capture stream is stopped, not that samples are discarded.
2. **Speech.** `eileen-speech` runs Silero VAD on every frame (one thread, 300 ms pre-roll). While VAD reports speech it feeds Nemotron and streams partials. On 200 ms of silence it runs Smart Turn on the last 8 s and fuses the result with transcript continuation cues and a 1.2 s silence cap (section 2.8) into an end-of-turn event carrying the final transcript. The speech process mints the `turn_` ID at the first speech onset after the previous end of turn.
3. **Addressee gate** (core, local, no model): the assistant's name in the transcript, a follow-up window of 8 s after Eileen spoke or asked something, or a full match in the command grammar. Anything else is shown on the HUD as heard and not acted on.
4. **Intent resolver** (`eileen-intent`, section 3.3.10), layered so it stays instant where it can:

| Layer | What it does | Latency budget | When it answers |
|---|---|---|---|
| L1 grammar and learned templates | Deterministic grammar for Eileen's own verbs plus learned skill templates with typed slots | under 1 ms (measured matcher: 4 to 6 us median, under 1 ms p99 at 5,000 skills) | A full match with every slot resolved |
| L2 local semantic matcher | BM25 over skill names, descriptions and triggers, plus cosine similarity from a small local sentence-embedding model run in the speech process | under 25 ms including the embedding round trip | Hints into the brain's context, and the paraphrase-to-known-action map for confirmed equivalents (below); an unconfirmed L2 hit never triggers an action |
| L3 brain | The warm Claude session with live context (focused app, its tree summary, selection, jobs, user facts, L2 hints) | first text 1.0 to 1.5 s quiet, 2.2 to 4.2 s busy | Everything else |
| L4 worker | When no capability exists yet, the brain dispatches a worker that figures it out; the learning loop turns the result into a skill that feeds L1 and L2 | seconds to minutes | New capabilities |

   L2 grows a confirmed paraphrase map: when the brain resolves an utterance to a fast-path action or a skill and the user accepts the result, the pair (utterance embedding, action) is stored as a `phrasing` fact; a later utterance within a strict similarity threshold of a confirmed pair and with all slots resolvable runs that action directly, still through the permission gate. Unclear requests get one short clarifying question, never an error: the resolver returns `Ambiguous` with the options, the core asks, and the answer resolves through the follow-up window. "What can you do here?" is answered by introspection (section 3.3.10), not from a static list.
5. **Fast path** (L1 hit): the core emits the action as a tool call through the same dispatcher and permission gate a worker uses, and replies from the phrase cache.
6. **Brain** (L3): the core sends one stream-json user message with the context blocks; text streams back, is cut at clause boundaries and spoken; tool calls such as `jobs_dispatch` start workers.
7. **Workers** run in parallel as jobs. Their narration, approvals and completions come back as events; the announcement policy decides when anything is spoken.
8. **Learning**: every tool call from every origin is recorded at the dispatcher; when a job ends, a 120 s regret window listens for "no", "undo that", "that's wrong", a retry or praise.

### 2.3 The brain

A warm `claude -p` process on the owner's plan, started when Eileen starts, with a hot spare started once the first is ready (`brain.hot_spare = true`, settable off on small machines). Flags, from the subscription-harnesses, brain-latency, claude-mods and learning-loop reports:

```
claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages
  --model <brain.fast model id> --effort low
  --system-prompt <brain prompt, under 1,500 tokens>
  --tools "" --allowedTools "mcp__eileen__*"
  --strict-mcp-config --mcp-config <run\brain\mcp.json>       (only the eileen shim, toolset "brain")
  --setting-sources "" --settings <run\brain\claude-settings.json>
  --no-session-persistence
environment: constructed, not inherited (section 2.12); ENABLE_CLAUDEAI_MCP_SERVERS=false; CLAUDE_CODE_DISABLE_AUTO_MEMORY=1
settings file: {"alwaysThinkingEnabled": false, "disableSkillShellExecution": true}
```

Never `--bare` (it drops subscription sign-in) and never `--safe-mode` (it suppressed `--mcp-config` servers in tests). Connectors are off: with claude.ai connectors and user MCP servers loading, warm turns were two to four times slower. The model id is a full id, never an alias, because aliases drifted between CLI builds; WP3's first task is to read the resolved ids for `haiku`, `sonnet` and `opus` from `system/init` once and write them into the default config. Thinking must be off for the brain: Haiku 5.5 thinks by default, which would put about 10 s before the first word. If Haiku 5.5 with thinking off is slower than Haiku 4.5, the default is Haiku 4.5 until it is retired (subscription-harnesses report, section 6).

**Turn input.** One user message per turn, built by the core, with blocks appended after the cached prefix so prompt caching keeps working:

```
[heard]   what the user said (final transcript); after a barge-in, [interrupted] and the exact prefix they heard
[desktop] monitor 1 (left, 1920x1200, 144 dpi): DaVinci Resolve "Beach Edit - Color" (foreground)
          focused: none; selection: none; recent: window Resolve, job beach-face
[jobs]    * beach-face  computer use  Claude  running 2m  "adding a sharpen window"   (focus)
            login-tests coding        Claude  done 1m ago "3 of 3 pass"
[hints]   known skills that may apply: resolve-sharpen-face (trusted, 4 uses)
[facts]   "the video editor" means DaVinci Resolve; Resolve timelines start at 01:00:00:00, 24 fps
[context] 2026-10-21 14:02, Tuesday; location: Austin (set by the user); depth: auto; plan: 41 percent of 5 h window
```

Caps: windows 12, selection 300 characters, jobs 8, hints 3, facts 8, ledger lines 6; the whole block stays under about 1,200 tokens (600 for desktop and jobs, 600 for learning).

**Turn output.** The brain prompt asks for spoken style: at most two sentences (or what `assistant.verbosity` says), then optionally a line `---hud---` followed by Markdown for the screen. The streamer speaks everything before the marker clause by clause and sends the rest to the HUD. Actions are tool calls on the `eileen` MCP server, toolset `brain` (section 3.3.9). The brain never drives the mouse; computer use is always a worker job.

**Depth tiers.** Instant (L1 or cache, no model), quick (the brain alone), considered (a worker on the mid model with light thinking), deep (a worker on the strongest model with thinking and web research). The brain picks by calling `jobs_dispatch` with `kind = "deep_answer"` and a depth; "quick answer" and "think hard about this" are L1 overrides that set the depth before the brain sees the turn.

**Two speeds.** For considered and deep questions the brain answers at once with what it knows and dispatches the deep job in the same turn. When the deep answer lands: if the user is not speaking and has not moved to an unrelated topic (no new turn for 20 s, or the new turns are about the same job), two sentences are spoken ("Better answer: ...") and the full answer goes to the HUD; otherwise only the HUD changes and a short line waits for a pause. The user never hears both answers in full (simulation invariant 11). The deep worker has the web search and fetch tools and issues several searches in one turn; when the brain passes up to four sub-questions, the core fans out that many quick research workers plus one combiner instead.

**Context makes it right.** Location is opt-in and local (`privacy.location`, set by voice: "I'm in Austin"); time comes from the PC; preferences and past choices come from the user model; what the user is doing comes from DesktopState. Slow-changing results (opening hours, a restaurant list) are cached with an expiry in `learn.sqlite` and reused for follow-ups ("closer"); the user's follow-ups are kept as facts. Anticipation (looking things up before the user asks) is off by default because it spends plan limits.

**Lifecycle.** The session serves many turns. The core recycles it when the context reported in the `result` usage passes a threshold (default 60,000 tokens) or after 10 idle minutes: the spare takes over with a summary of at most 300 tokens of the conversation, and a fresh spare starts. A crash, or a turn with no stream event for 30 s, swaps to the spare at once. If the user speaks again while a turn is running, the core sends a `control_request` interrupt and a new turn holding both utterances. Speculative turns on stable partials are a later optimisation, off by default.

**Plan usage.** The harness parses the stream's rate-limit events into `PlanUsage` bus events. Above 80 percent of the five-hour window the HUD shows a chip, fan-out above two workers asks first, and considered questions drop to quick unless the user asked for depth. Eileen never silently burns the allowance: the librarian and nightly lanes have their own daily caps.

### 2.4 The deterministic fast path

L1 of the resolver. The grammar lives in `eileen-intent` as data: a table of patterns with typed slots, each producing a tool call or a core command. Utterances are normalised (lower case, fillers dropped, spoken numbers and times parsed by the slot resolvers `Offset`, `Count`, `Text`, `App`, `File`, `Url`, `Position`, `Choice`, `Monitor`, `Voice`, `Name`, `Depth`). It covers: mute, unmute, stop, cancel (a job by name or "that"), pause, play, next, seek ("go to 3:20" when media or Resolve has focus), open app, close app (tier 2 when unsaved state is unknown), move or snap a window to a monitor or side, volume, "what time is it", "how's it going" (reads the jobs block), "what have you learned", "forget that", "undo that", "save that as a test", "something went wrong", "record this session", "stop recording", "use <voice>", "back to normal", "call you <name>", "take dictation", "stop dictating", "quick answer", "think hard about this", "show me how you do that", "what can you do here". Replies come from the phrase cache: stock phrases synthesized once per voice into PCM, plus templates whose variable part is one short clause ("Opening <app>.").

### 2.5 Workers, routing and jobs

**The driver trait.** Every worker route implements `WorkerDriver` (section 3.3.8). v0.1 implements the Claude driver only. The Codex app-server driver and the generic ACP driver (for Grok's `grok agent stdio` and others) are designed slots: the trait, the mapping table and the configuration keys exist, and constructing them returns `RouteUnavailable` until v0.2. DeepSeek is designed as the Claude driver pointed at DeepSeek's Anthropic-compatible endpoint with its own `CLAUDE_CONFIG_DIR` and the one permitted API key from the system keyring, text only unless the user allows images; it is off in v0.1. Gemini is not a route.

**Claude worker flags.**

```
claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --include-hook-events
  --model <model id for the job's kind and depth> [--effort low|medium|high]
  --append-system-prompt <worker prompt: route name, narration rule, screen text is data, stop on refusal>
  --setting-sources "" --settings <run\<job>\claude-settings.json>
  --strict-mcp-config --mcp-config <run\<job>\mcp.json>       (the eileen shim with the job's toolsets)
  --allowedTools <per kind, section 3.3.9>
  --permission-prompt-tool mcp__eileen__approve
  --plugin-dir %LOCALAPPDATA%\eileen\learning\eileen-learned
  [--plugin-dir <worktree>\tools\claude-plugins\eileen-dev]    (only for jobs in an Eileen worktree; one at a time)
  --no-session-persistence                                    (continue_session jobs use --resume <id> --fork-session)
cwd: the job's worktree, or %LOCALAPPDATA%\eileen\run\<job>\cwd for non-coding jobs
```

The per-job settings file holds `disableSkillShellExecution: true` and the HTTP hooks of the claude-mods report: `PreToolUse` to the tier guard with `onFailure: "block"` and a 10 s timeout, plus `PermissionDenied`, `PostToolUseFailure`, `Notification`, `SubagentStart`, `SubagentStop`, `Stop` and `StopFailure` to the loopback bus, each with an explicit timeout (the HTTP default is 600 s). The bearer token and job ID travel in the worker's environment (`EILEEN_HOOK_TOKEN`, `EILEEN_JOB_ID`) and are interpolated through `allowedEnvVars`, so they never sit in the file. The guard answers no decision for tiers 0 and 1, `ask` for tier 2 (which routes to the `approve` tool and the user), and `deny` for tier 3; it never answers `allow`, and with `onFailure: "block"` a dead Eileen stops risky work instead of waving it through.

**Narration.** The worker prompt asks for one line starting with `> ` before each significant step. The harness turns those lines into `JobNarration` events; the core speaks at most one line every 20 s, only for the focus job, only when the user is not speaking and not in a call, and only lines of 12 words or fewer. Everything else goes to the HUD.

**Routing.** Policy is TOML (section 2.11): each job kind maps to a route, a model id and a depth. v0.1 defaults: every kind goes to Claude; quick uses the fast model, considered the mid model, deep the strongest. Spoken overrides beat policy at once ("use the strongest model for this", "send this to a stronger model", later "send this to Codex") and are recorded on the job with the words that caused them. "Why did you do that?" is answered from the recorded `router.decide` reason.

**Fan-out.** `jobs_dispatch` takes `fanout` (1 to 8) and `mode` (`compete`: every worker gets the whole task; `split`: the brain supplies one sub-task per worker). For coding jobs each worker gets its own git worktree under `%LOCALAPPDATA%\eileen\worktrees\<repo-hash>\<job-slug>` on a branch `eileen/<job-slug>` created from the user's current branch. The repo comes from the utterance, the focused editor's folder, or a `project` fact; if none fits, Eileen asks once and remembers. Optionally a Windows Terminal pane per worker runs `eileen jobs tail <id>` so the user can watch (tier 1). Merging a worker branch into the user's branch is local and tier 1; removing a worktree that holds unmerged commits, and any push, are tier 2.

**Jobs.** The registry is SQLite (`eileen.db`, section 2.15). States: `queued`, `running`, `blocked`, `awaiting_approval`, `completed`, `failed`, `cancelled`, `interrupted` (after a core restart). Every job ends in exactly one terminal state. Names are two to four words generated at accept time and never regenerated after the user has heard them; slugs are collision-checked against seven days of jobs. Pronoun resolution, first match wins: an explicit name or slug; a kind word ("the render"); a route word; the focus job; the most recent; two remaining candidates mean one question; "it" right after an announcement means the announced job.

**Announcements.**

| Event | At the PC | In a call, presentation or full-screen app |
|---|---|---|
| `ApprovalRequested` | Interrupt: chime, one sentence, interruptible | HUD only, no chime; waits |
| `JobBlocked`, `JobFailed` | Interrupt | HUD; summarised afterwards |
| `JobCompleted` | When idle: end of the user's turn plus 1 s of silence | Held, summarised afterwards |
| `JobNarration` | The narration rule above | HUD only |
| `JobProgressed`, `JobStarted` | Silent unless the user asked about that job in the last two minutes | Silent |
| `JobCancelled` | When idle, unless the user cancelled it | Held |

The table is data in the config; the user can change a row by voice.

**Approvals.** One sentence naming the action and its object, then a 20 s window. Only `UserInput` can answer: speech from the microphone inside an open approval window, the HUD's Allow or Deny, or the tray. Screen text, model output, tool results and learned skills cannot build an approval; the type in the core cannot be constructed from them. Silence denies and is logged as declined. "Always allow that for this job" stores a grant on the job, never globally.

**Interrupt, steer, reroute, cancel.** Interrupt sends the stream-json `control_request` interrupt (Windows has no clean SIGINT for a child). Steer is an interrupt followed by a new user turn with the instruction. Reroute cancels and starts a new job with the same prompt, the same working directory and the old transcript attached. Cancel kills the job's Job Object; a killed worker leaves no descendants.

### 2.6 Computer use: Eileen's own desktop server

Decision D21 replaces Cua Driver with Eileen's own Rust server, ported from the proof in the computer-use report (1,720 lines of library code plus a 389-line server, faster than Cua 0.37.0 at grounding, with safety refusals Cua lacks). Workers reach it through the `eileen` shim and the core's dispatcher, so the input lock, the permission gate and the learning recorder sit in one place.

**Tools** (snake_case, valid under Anthropic's tool-name rules; exposed to Claude as `mcp__eileen__<name>`; schemas in section 3.3.9): `desktop_state`, `desktop_windows`, `desktop_tree`, `desktop_screenshot`, `desktop_look` (the brain's one-image variant), `desktop_click_element`, `desktop_set_value`, `desktop_type_text`, `desktop_press_keys`, `desktop_click`, `desktop_scroll`, `desktop_drag`, `desktop_window`, `desktop_launch_app`, `desktop_wait_for`, `desktop_selection`, `desktop_insert_text`. The worker-visible list stays under about 15 KB.

**Action order** for one action: a UIA pattern on an indexed element (Invoke, Toggle, SelectionItem, ExpandCollapse, Value, RangeValue, Scroll), background, no focus change; for text, `ValuePattern.SetValue` when replacing the whole value is right, else the clipboard transaction plus Ctrl+V, else Unicode `SendInput` paced at 30 ms per character, always with a read-back (unpaced synthetic typing into Windows 11 Notepad arrived garbled); coordinates, key combos, drags and the wheel through `SendInput`, which needs the foreground; vision last (a screenshot with set-of-mark indices, the model picks, then a coordinate action with a post-condition). Every action returns a verification hint (value read back, title change, new focused element), which the learning loop uses as the step check.

**Foreground.** Before taking the foreground, check `GetLastInputInfo`; if the user touched the keyboard or mouse in the last 1.5 s, wait up to 2 s, then ask on the HUD and by voice ("May I take the Resolve window for a moment?"). When Windows refuses the foreground, say so and stop; never loop on workarounds. Restore the previous foreground window and the cursor afterwards. Re-check the foreground before every synthetic key and stop at once if focus moves. One input lock across all workers, held in the core's dispatcher; background UIA actions on different windows run in parallel. The HUD shows who holds the lock. A signed UIAccess helper is a later decision (it needs a code-signing certificate and a Program Files install).

**Visible intent.** Before any `SendInput` or Invoke on a window the user can see, the desktop server emits a `DesktopIntent` event (target rect and label) and waits `desktop.visible_intent_ms` (default 400, settable to 0). Speech onset during the wait cancels the action and releases the lock, which is what makes "no, the other one" work.

**DPI and coordinates.** Per-monitor-v2 awareness is the first call in `main` (DPI-unaware code sees 1280x800 on the owner's 1920x1200 display). One coordinate system everywhere: physical pixels in virtual-screen coordinates, negatives allowed. Window rects are DWM extended frame bounds. Every screenshot reply states its mapping (`image 1280x800 covers screen rect x=0 y=0 w=1920 h=1200; screen = origin + image_px * 1.5`). Screenshot replies carry text and image content only, never `structuredContent`, because the claude CLI dropped images when both were present. Multi-monitor and mixed DPI are untested on the owner's PC; the test fleet covers them.

**Browsers and Electron.** Chromium and Electron expose only browser chrome over UIA and refuse background input. Page content goes through screenshots and foreground input in v0.1 (and the Playwright MCP extension when the user installs it, section 2.7); UIA is used for tabs, the address bar and dialogs. `Chrome_WidgetWin_1` also covers the Claude desktop app and VS Code, so Eileen's and Claude's own windows are excluded by process, never by title. Every cross-process window call is bounded (`ShowWindowAsync`, `SendMessageTimeout`): `ShowWindow` on a paged-out Edge blocked for over 60 s.

**Safety rules in the server**, so they apply to every worker, every learned skill and every future connector: never read or write a password field (`IsPassword`) and never type while one has focus; refuse blocked combos (`Win+L`, `Win+R`, `Win+X`, `Ctrl+Shift+Esc`, `Win+Ctrl+Shift+B`, Ctrl+Alt+Del variants, `Alt+F4` on a window the job did not open); explicit targets only, refusing when a different window has the foreground at send time; never touch Eileen's, Claude's or user-marked private windows; invoking a control whose name or automation id reads Send, Delete, Buy, Pay, Submit, Publish or Install is tier 2; never elevate; third-party components run with telemetry off (`DO_NOT_TRACK=1`); "stop" and mute cancel queued input and release the lock. Tests drive only windows the test created.

**App index.** `shell:AppsFolder` enumeration (222 apps here, 1.9 to 3.1 s idle, up to 41 s under load) at startup and on Start menu or package changes, never per call. Launch by AUMID (packaged built-ins such as Notepad are stubs in System32). Matching is Eileen's own scorer (subsequence, prefix and word-start bonuses; no MPL dependency); it launches only when the top match beats the next by a margin, otherwise it asks.

**DesktopState** answers "what is the user looking at" as one object: monitors with Eileen's own stable ordinals (left to right, matched by device path across reboots), visible windows with frame rects and z-order, the foreground window and process, the focused element (control type, name, automation id, rect, patterns, `is_password`), the selection (TextPattern, else the user's last copy by clipboard sequence number, never a synthetic copy), the caret, recent references, the presentation state (`SHQueryUserNotificationState`) and whether another app is using the microphone. Fields that timed out are listed in `degraded`, never reported empty. Text leaving the server goes through `eileen_contracts::desktop::sanitize` (valid UTF-8, bounded, control and bidirectional-override characters removed, tagged untrusted).

**Dictation insertion** lives here too, behind the same input lock (section 2.8).

### 2.7 App adapters

`eileen-apps.exe` is one resident Rust process: a thread per COM target with call timeouts and an `IMessageFilter` for "call rejected by callee", one SMTC manager held for the process lifetime and fed by events (a fresh manager request per read reached 2.2 s p95 under load, a held one about a microsecond), and the Resolve bridge as a supervised child. Tools: `apps_media`, `apps_open_url`, `apps_resolve`, `apps_office`, `apps_explorer`, `apps_recent`, `apps_image`, `apps_photoshop` (section 3.3.9).

**Media and YouTube.** SMTC from Rust: subscribe to `SessionsChanged`, `CurrentSessionChanged`, `PlaybackInfoChanged` and `MediaPropertiesChanged`; control with `TryPause`, `TryPlay` and `TryChangePlaybackPosition`, confirmed by the next event (the status returned with an action is the old one). The live position is derived from `Position`, `LastUpdatedTime` and the playback rate. YouTube: open `https://www.youtube.com/watch?v=<id>&t=<s>s` with `ShellExecuteW` in the default browser (tier 1; measured to land at 15:30 after a preroll ad); for unknown videos, a worker opens the results page in the user's own browser and picks from a screenshot. Never search YouTube headlessly (robots.txt disallows it) and leave yt-dlp out of the default path. After a load or seek, wait for the SMTC session to return with the expected title before reporting success. The Playwright MCP extension (Node sidecar, Apache-2.0) is supported when the user installs it, as the DOM rung for workers. Claude in Chrome is used only inside a Claude worker started with `--chrome`, by user opt-in, and never by Eileen talking to its native host.

**Resolve.** A Lua bridge (about 130 lines, `adapters/resolve/eileen_resolve_bridge.lua`) run by Resolve's own `fuscript.exe -q -l lua`, spawned once and supervised, gated on `Resolve.exe` running (otherwise every call stalls about 4 s). Line protocol: request `<id>\t<method>\t<arg>...`, reply one JSON object per line; parse, never compare strings (Lua key order varies). Methods: `hello`, `timeline_info` (start timecode, start frame, frame rate, drop frame, current page), `goto_tc` (an absolute timecode computed in Rust, drop-frame aware, never the spoken offset), `open_page`, `current_item`, `export_still`, `apply_drx`, `marker_add`, `markers`. Known limits: no API for nodes, OFX, power windows, trackers, Face Refinement's Analyze, play or blade; `SetCurrentTimecode` refuses on the Media and Fusion pages and before the timeline start with a bare `False`. When the owner moves to Studio 21.1 or later, `ResolveMCP.exe` is added to workers' generated MCP config and measured against the bridge before anything else changes. Python fusionscript is a fallback only and is not built in v0.1.

**Face and sharpness.** `apps_image` runs YuNet (MIT, OpenCV Zoo, a small ONNX model) through `ort` against the shipped `onnxruntime.dll` to find faces in a still, picks by position (left, right, center, largest), and measures sharpness as the variance of the Laplacian inside a box.

**Office and Explorer.** One late-bound `IDispatch` helper (about 120 lines, from the app-hooks probe) serves Word `Selection.Text`, Excel `ActiveCell`, Photoshop and `Shell.Application`. Attach through the running object table with `CLSCTX_LOCAL_SERVER`; bind to the foreground window's document through `AccessibleObjectFromWindow` when several instances exist. Explorer selection through `Shell.Application`, handling tabs that share one top-level window.

**Recent work.** An in-memory index fed by jump lists for known AppIDs, the Recent folder (parsing `.lnk` bytes, never the 30 s cold COM resolve on the voice path), Office MRU from the registry, app-native recents through each adapter, and a `ReadDirectoryChangesW` watcher on the user's folders; Windows Search only when the index has nothing. The index stays local.

**Photoshop** (designed, compiled, tested against a fake COM target only): attach `Photoshop.Application`, `DoJavaScript` for `app.recentFiles`, open, duplicate the layer, a feathered elliptical selection at the face box, Smart Sharpen. Generative Fill and Neural Filters are tier 2.

**Later (v0.2):** Premiere through an Eileen UXP plugin connecting out to a loopback WebSocket, a VS Code extension for editor state, Photoshop UXP for batchPlay-only features.

### 2.8 Voice

**Pipeline** (voice-stack report, all CPU, one shipped `onnxruntime.dll` 1.28.2):

| Stage | Choice | Process |
|---|---|---|
| Audio I/O | cpal 0.18.2, WASAPI shared mode, 48 kHz, 10 ms buffers; the `windows` crate for the endpoint, the optional Communications category (with Windows ducking of other apps opted out) and MMCSS | core (`eileen-audio`, `eileen-audio-win`) |
| Resample | 48 kHz stereo to 16 kHz mono, and TTS output (24 kHz) up to the device rate; WP4 picks `rubato` or its own polyphase filter by measurement and records the choice | core |
| Echo cancellation | sonora 0.2.0 (pure Rust WebRTC AEC3 with noise suppression and high-pass; the reference is the exact PCM Eileen plays; ERLE 33.3 dB on the synthetic test, 592 us p50 per 10 ms frame). Windows' Communications-category AEC is a selectable alternative | core |
| VAD | Silero VAD v6.2 through sherpa-onnx 1.13.8 (`shared` feature), 32 ms windows, one thread, 300 ms pre-roll; idle listening cost 5.8 percent of one logical core and 31 MB | speech |
| ASR | Nemotron speech streaming EN 0.6B, 560 ms chunks, int8, four threads, fed only while VAD reports speech; WER 2.35 percent on the synthetic corpus, final about 0.4 s after speech ends, about 740 MB | speech |
| End of turn | Smart Turn v3.2 CPU through `ort` =2.0.0-rc.13 linked to the same DLL, Whisper log-mel in Rust (matches the reference within 1.1e-5), run after 200 ms of silence. The turn ends when Smart Turn says complete and the partial does not end on a continuation (a conjunction, an article, a preposition, "and", "the", "to"), or after a 1.2 s silence cap. Smart Turn alone ended unfinished synthetic utterances early, so it is one vote | speech |
| TTS | Kokoro v1.0 fp32 from `kokoro-multi-lang-v1_0`, voice `af_heart`, four threads; replies cut at clause boundaries with a first clause of three to six words; the next clause synthesized while the current one plays; stock phrases cached as PCM | speech |
| Barge-in | While Eileen speaks, VAD on the echo-cancelled signal with a stricter threshold; duck about 20 dB at the first speech window, stop at the third consecutive window (96 ms), cancel synthesis through the generation callback (returning `false` in the Rust crate; the Python API is inverted), and record the samples handed to the device; about 70 to 110 ms from onset to silence. An energy detector in the core is the fallback while the speech process is down | speech decides, core acts |
| What was heard | Samples handed to the device are mapped back to the clause being played and, within it, to characters in proportion to time; the brain is told the heard prefix | core |

Resident memory with models loaded is about 1.25 GB. Model loads take 10 to 20 s, so they load at startup, never per utterance. One ASR model stays resident. NPU and iGPU paths are out of v0.1: DirectML failed on Kokoro and Smart Turn, and only Smart Turn ran on the NPU.

**Addressee gate.** The name (from settings, matched phonetically with Soundex plus normalised Levenshtein, so a renamed assistant works at once), the 8 s follow-up window, the L1 grammar. The CAM++ speaker embedder (to ignore other voices) and a name keyword spotter are evaluated in WP4 and adopted only if they measure well; until then, other voices are handled by rules.

**Dictation.** Triggers: hold the dictation key (default Right Ctrl, through a low-level keyboard hook on its own thread), tap to toggle, "Eileen, take dictation" until "stop dictating" or a long pause, or one-shot "Eileen, type: ...". While dictation is active, speech goes to the field, not the brain, except an escape grammar (stop, scratch that, new line, undo that, paste it). Speech model: the already-loaded streaming recognizer (no extra memory, live words on the HUD); Parakeet TDT 0.6B v3 int8 is loaded on demand for other languages and unloaded after 5 idle minutes. Whether Parakeet Unified EN 0.6B should replace Nemotron as the single English model is one measurement in WP4.

Cleanup tier 0, always, in Rust (`eileen-dictation`, a pure crate with a fuzz target, called by the core through the `DictationCleaner` port): vocabulary replacement, scratch-that, fillers per language, stutters, typed-slot self-corrections ("at 3, no, 4 pm"), spoken punctuation and layout, context formatting by app category (email, chat, code editor, terminal, search box, document), the user's own rules last; SpeakoFlow's 79 conformance cases (MIT) plus Eileen's own positives and negatives run in CI. Tier 1, opt-in: a lazily started Claude "scribe" session (fast model, no tools, connectors off, closed after ten idle minutes) that receives each finished sentence while the user is still talking, guarded by a check that rejects output adding unspoken words, dropping content words or exceeding an edit-distance budget; never for terminals, code, search boxes, password fields or private apps.

Insertion ladder (desktop server): refuse password fields and secure desktops; check the target is unchanged; `EM_REPLACESEL` for Win32 Edit and RichEdit (5.8 ms measured, exact for emoji); otherwise a port of Handy's clipboard transaction (MIT, attribution kept: full-format snapshot, delayed rendering with a read receipt, guarded restore, clipboard-history and cloud opt-out formats) plus Ctrl+V, or Ctrl+Shift+V for terminal window classes; last resort paced Unicode `SendInput`; verify where UIA can read the field; record the insertion for "undo that". If focus moved, the text waits in the HUD until "paste it". `ValuePattern.SetValue` is never used to insert, because it replaces the whole field.

Vocabulary: `vocabulary` facts in the user model, used as sherpa-onnx hotwords (if the beam-search cost measures acceptable), as a Soundex plus Levenshtein post-pass, and in the scribe prompt. New words come from explicit requests and from a read-only UIA watch of the field for 30 s after insertion, with Wispr Flow's exclusions (file names, URLs, numbers, code, everyday words); two occurrences or one HUD confirmation make an entry. Names visible in the focused window may bias one utterance and are never stored without consent.

**Voices.** A `Synth` trait (clauses in, PCM chunks out, cancel through the callback) with Kokoro and Pocket TTS implementations through sherpa-onnx (`OfflineTtsPocketModelConfig` with `reference_audio`). Pocket TTS (MIT code, CC-BY-4.0 weights, credited in the about screen and NOTICE) is the cloner; Chatterbox is an optional pre-render extra in v0.2. A per-reply watchdog keeps the measured real-time factor of the last clauses per engine; above 0.8 the next reply (never mid-reply) falls back to Kokoro and the HUD notes it. Profiles live in `%LOCALAPPDATA%\eileen\voices\<name>\`. Roles: `voice.live`, `voice.narration`, `voice.announcements`, `voice.fallback`. Enrollment: the voice's owner reads a consent sentence naming the date and this install, then a passage; the speaker-embedding model must score the two as one speaker above a threshold; Kyutai's terms are shown once and the showing is recorded; a file of someone else without that person's own consent recording is refused. No watermark by default on any engine (Chatterbox's Perth off too, when it arrives); `voice.watermark_exports = true` applies one (AudioSeal or Perth, both MIT) to saved or exported audio. Speaking as the user to other people is tier 2 per use.

### 2.9 The HUD and the Claude app

**HUD.** `eileen-hud.exe` draws two windows with raw Win32 through the `windows` crate (no new dependencies): an overlay (`WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, click-through, GDI text first) and a small strip without `WS_EX_TRANSPARENT` for the mic button, Allow and Deny, and Save a test. Both call `SetWindowDisplayAffinity` with `WDA_EXCLUDEFROMCAPTURE` so screenshots never include them. The core sends a full `HudState` (section 3.3.6) at most ten times a second as one JSON line; the HUD sends clicks back as `HudInput` lines. Layout is computed by platform-neutral code into a draw list that snapshot tests pin; only the renderer is Windows code. The egui and WebView2 alternatives stay in the lab until someone measures all three with `measure-idle.ps1`.

**Tray and hotkeys** run on a message-pump thread inside the core (tray-icon 0.26.1 without default features, global-hotkey 0.9.0): mute (Ctrl+Alt+M), save a report (Ctrl+Alt+Shift+B), dictation (Right Ctrl, low-level hook). Toasts (tauri-winrt-notification 0.8.1) need an AppUserModelID registered through the Start menu shortcut that `eileen setup` creates; they are only for "a job finished while you were away".

**The Claude app** (claude-mods report): one opt-in plugin named `eileen` in `integrations/claude-plugin/`, listed by a `.claude-plugin/marketplace.json` at the repository root and installed by the owner through Claude's own plugin flow (`eileen claude install` prints the two official commands and runs them only after a yes). It holds the handoff skill (`/eileen:handoff`, `disable-model-invocation: true`, passing `${CLAUDE_SESSION_ID}` to Eileen, which continues the session as a `continue_session` job with `--resume <id> --fork-session`), the `eileen` MCP server for the owner's own sessions (`eileen-shim.exe --role session`: `session_status`, `session_jobs`, `session_say`, `session_mute`, `session_handoff`, `session_ask`), and a display-only mod (a status entry, a band with the last utterance and Mute, and `/eileen` answered with no model turn). The mod never answers `tool.check`, never approves anything, never submits prompts, and Eileen never depends on it. Learned skills are not put into the owner's own sessions in v0.1 (they would cost context in every session). CI validates the plugin and fails if the validation report gains a gating hook or a new host call without a reviewed change. Nothing patches, wraps or replaces the app, and nothing turns off its updater.

### 2.10 The learning loop

From the learning-loop report, with two changes recorded in D27: version history lives in `learn.sqlite` instead of a git repository per skill (no `gix` compile cost on this PC, no dependency on a git CLI the user may not have, and "forget" is one transaction), and the crate is named `eileen-learn`.

1. **Capture.** Every tool call from every origin (brain, worker, replay, fast path) becomes a `TrajectoryStep` at the dispatcher, which is authoritative for Eileen's own tools; the harness adds the worker's own tool calls from stream-json and the cost from the `result` event. "What was on screen" is captured as structured app state, not screenshots. Redaction runs before anything reaches disk. Each job gets a 120 s regret window for undo, correction, retry, praise and app-level undo seen by the hooks; silence is weak acceptance.
2. **Distillation.** The librarian is a low-priority Claude worker that runs only when no foreground job runs and the user has been idle for a few minutes, under a daily cap (default 10 sessions). It has no built-in tools and only the `learn` toolset (`learn_trajectory_read`, `learn_skill_list`, `learn_skill_read`, `learn_skill_propose`). It writes native `SKILL.md` folders ("lessons, not logs") with an optional `recipe.json` of Eileen tool calls with pre and post checks. `learn_skill_propose` validates the schema and runs the linter; nothing else can write a skill.
3. **Retrieval.** Every worker loads the `eileen-learned` plugin through `--plugin-dir` (verified on the owner's PC: the model invoked a learned skill with the spoken argument and read its reference file). Plugin skills are fixed at session start, so the brain's hot spare and idle worker sessions are recycled after a skill changes. Locally, L1 templates can trigger replay and L2 hints only inform.
4. **Promotion ladder.** Draft (one clear success), trusted (three successes with two distinct slot values, no failure in the last five), compiled recipe (three agreeing shadow runs and one explicit user approval), spoken fast path (ten clean replays and at least 95 percent clean over the last 20, announced when it happens). Two failures in the last five uses drop a level; an app version change puts a recipe on probation; 30 idle days archive a skill (never automatic deletion); pinned skills are never archived. Thresholds live in `LadderPolicy` so measurement can tune them without code changes.
5. **Replay.** Slots resolve deterministically in Rust or the job goes to a worker before any action; each step runs through the same dispatcher and permission gate as a worker with origin `Replay { skill, version }`; checks call only tier-0 tools; the first failed check stops the replay and hands off to a Claude worker with a briefing (request, skill and version, steps done with results, the failed check with the observed state, steps left, continue from here). The recovery trajectory becomes a repair candidate.
6. **Self-healing selectors.** Steps are recorded by identity (process, window class, automation id, control type, name, ancestor chain) plus a post-condition, with the element index only as a hint. When a selector no longer resolves after an app update, the desktop server re-finds the element by name and control-type similarity within the same window; if exactly one candidate passes the step's post-condition, the replay continues and the librarian writes a repaired version with reason `repair`; otherwise it hands off.
7. **User control** by voice and HUD: what have you learned, show me how, forget that (confirms once, then deletes the skill, its versions and its evidence, and writes a tombstone), don't learn from this, stop or start learning, approve, use the old version, how much time have you saved me.
8. **User model and episodic ledger.** Itemized facts (`user`, `feedback`, `project`, `reference`, `vocabulary`, `phrasing`) with provenance and confidence, updated by deltas, never whole rewrites. The ledger records Eileen's own events plus sampled focus changes (app, window title, time) for 14 days and answers "go back to what we tried an hour ago" and "what was that page I had open yesterday". Off-the-record mode records nothing. At most about 600 tokens per brain turn come from learning.
9. **Safety.** A replayed step carries no permission of its own; confirmations are raised again; "always allow" grants are never written into skills; lines that came from screen text enter a skill only as quoted reference data after the user approves them on the HUD; learned frontmatter is an allowlist (`name`, `description`, `when_to_use`, `arguments`, `argument-hint`, `disable-model-invocation`, `user-invocable`, `metadata`); no `scripts/` folders and no shell preprocessing; secret-like strings are rejected; auto memory and session persistence are off in every session Eileen starts.

Measurement per skill: uses, successes, failures, undos, handoffs, and median duration, model turns and tokens of the first runs against the last ten. "Time saved" is the difference times the uses. B8 (section 4.7) is the owner-facing proof. Sharing skills across many machines is v0.3; the test fleet will verify a skill on several machines before it can ever be offered for sharing.

### 2.11 Settings by voice, and configuration

**Files.** TOML with comments (`toml` 1.1.8), in `%LOCALAPPDATA%\eileen\config\`: `eileen.toml` (the user's settings, every key documented in a comment), `profiles\<name>.toml` (overlays), and a generated `eileen.schema.json` for editor completion. The commented default file and the schema are generated from the config types' doc comments by `cargo xtask codegen` and checked fresh by a test. `eileen config explain` prints every effective value and where it came from (default, file, profile, voice).

**The settings model** (`eileen-config`, section 3.3.11). Every setting has a `SettingSpec`: key, type, default, doc, whether it is reachable by voice and on the HUD (all are, unless stated), its risk class, how it applies (live, or restart of one component) and its safety bound. Changes go through one flow, whatever the source:

1. **Propose.** The brain calls `settings_propose` with the changes and the user's words, or the L1 grammar does it for fixed phrasings. The proposal is validated against the schema and the safety bounds.
2. **Check before applying** when a choice could misfire. A new name: not on a common-word list, not close to a household member's name in the user model, and recognised in three of three sentences Eileen synthesizes and transcribes with its own recognizer. A new hotkey: a trial `RegisterHotKey` that fails if another app owns it. A new voice: the real-time factor of one clause.
3. **Confirm** in one sentence. Low-risk changes apply at once with "say undo that to go back"; changes with a warning ask first.
4. **Apply and version.** Every applied change writes a row in `settings_versions` (full snapshot, diff, source, the utterance); "undo that" or "go back to how it was" applies an earlier snapshot as a new version, so history only grows. Hand edits of the file are picked up by a watcher, validated (an invalid file keeps the previous version and the HUD names the line), and versioned with source `file`.
5. **Profiles** ("work mode", "editing mode") are overlays switched by voice and exportable as one TOML file.

**Safety bounds.** Tier 3 can never be relaxed. A relaxation for a tier-1 action is allowed by voice. Skipping confirmation for a tier-2 action needs an explicit, logged decision: Eileen names the action, the user says "yes, always allow <action>", and the decision is stored with the utterance; it can never cover credentials, payments, sending messages as the user or deleting outside a repository. The product and repository stay Eileen; the user's assistant name, voice and personality are the user's own.

**Rename** ("I want to call you Nova now") changes `assistant.name`, which the addressee gate, the recognizer hint list, the brain prompt's self-reference and the phrase cache all read; the cache re-renders phrases that contain the name.

### 2.12 Permission tiers and security

| Tier | Name | Examples | Behaviour |
|---|---|---|---|
| 0 | Observe | DesktopState, a screenshot, list files, read a page, read a job | Silent |
| 1 | Reversible | Move a window, open an app or a site, play or pause, edit files in a git worktree, create a branch, merge locally, add a Resolve node, insert dictated text | At once, journaled with its inverse |
| 2 | Consequential | Send a message, delete outside a repository, push, install software, spend credits, change settings outside Eileen, speak as the user to others, publish a scenario or results | Confirm first |
| 3 | Blocked | Enter credentials, make payments, disable security features, bypass a CAPTCHA, read vendor credential files | Never; explain and hand the step to the user |

Every tool declares its tier in its `ToolSpec`; a few depend on arguments (a control named Send is tier 2; a blocked combo is tier 3). An undeclared tool is tier 2. Worker built-in tools are classified by a deterministic policy (`eileen_core::tiers::claude_tool_tier`): reads are 0; edits inside the job's working directory are 1, outside are 2; shell commands match allow, ask and deny patterns (tests and builds in the worktree are 1; network installs, pushes and deletes outside the worktree are 2; anything naming a credential path is 3).

**Credential boundary.** Eileen spawns the unmodified, installed `claude` binary (resolved to the real `.exe`, never an npm `.cmd` shim) in a constructed environment and never reads, copies, logs or refreshes vendor credentials, never runs a login flow, and never writes the user's Claude settings for workers (all worker configuration is per-session flags). `eileen doctor` checks sign-in with `claude auth status` (exit code), never by reading files. The constructed environment starts empty and adds only `SystemRoot`, `SystemDrive`, `windir`, `USERPROFILE`, `HOMEDRIVE`, `HOMEPATH`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, `PATH` (system directories, git, the claude install directory), `PATHEXT`, `COMSPEC`, `NUMBER_OF_PROCESSORS`, `PROCESSOR_ARCHITECTURE`, `OS`, plus the job's own variables; every `CLAUDE*`, `ANTHROPIC*` and `MCP_*` variable of the parent is dropped. A tidy check fails if any source names a vendor credential path or the persistent MCP registration command.

**Untrusted input.** Window titles, UIA text, page text, file contents, transcripts of other voices, tool results and model output are data. They can never approve, raise a tier, create an override, or become a skill without the user's approval. The core's approval type can only be built from `UserInput`.

**Loopback endpoint.** Binds 127.0.0.1 only on a random port, with a per-boot bearer token in `%LOCALAPPDATA%\eileen\run\bus.json` (user-only ACL). That token reaches status and control (mute, say) only; no endpoint reachable with it can approve anything.

**Secrets.** Only one is allowed now (the DeepSeek key, used by no v0.1 route), plus connector tokens later. They live in Windows Credential Manager through keyring-core 1.0.1 and windows-native-keyring-store 1.1.0, are typed as `secrecy::SecretString`, and are never in config files, logs or model-visible text.

**Privacy.** Everything stays on the PC except what a Claude session sends to the owner's own plan. Trajectories, the ledger, recordings and incidents are local, size-capped and deletable by voice. Screenshots are not stored by default. The public repository never receives the owner's voice, screen or personal paths (tidy enforces it for fixtures and scenarios). The HUD's privacy note says plainly that Eileen can delete what it stores but not what a vendor retains.

### 2.13 Observability

From the observability report. Typed, prefixed ULIDs: `sess_`, `turn_` (the request ID), `job_`, `call_`, `speak_`, `appr_`, `inc_`, `rec_`. `TraceCtx { turn, job, call }` rides on every bus envelope, every speech frame header, every MCP request (`_meta.traceparent` built from the turn ULID, plus the IDs under an `eileen/` key; rmcp 3.6.0 has the accessors) and every worker environment (`TRACEPARENT`, which `claude -p` reads at start, and `EILEEN_JOB_ID`). One trace per turn; a job is a child span of the turn that started it and may outlive it. Span and event names are constants in `eileen_trace::names`; user content travels only in fields prefixed `pii.`; a span-parent table is checked in every simulation run.

Sinks in every process: JSON-lines files at INFO (`%LOCALAPPDATA%\eileen\logs\<process>.<date>.jsonl`, daily rotation, a janitor caps the total at 500 MB, `pii.` fields reduced to length and keyed hash unless `logging.include_personal`), an in-memory flight recorder at DEBUG (10 minutes or 32 MB in the core, 8 MB per sidecar, plus the core's input lane with a checkpoint every minute, so every incident is replayable), a console at WARN when started from a terminal, and the slow-span layer. The WASAPI callback never calls `tracing`; it bumps a heartbeat counter and pushes fixed-size ticks into a real-time-safe ring. OpenTelemetry, ETW and `tokio-console` are opt-in features that the PR lane never compiles (v0.2).

Incidents are written on a panic, crash, watchdog trip, supervisor kill, "Eileen, something went wrong", the hotkey or the tray, into `%LOCALAPPDATA%\eileen\incidents\<time>_inc_<id>\` with `manifest.toml`, merged events, the replayable input lane, `timeline.json` for Perfetto, the async task tree, the last DesktopState, the job snapshot, redacted config, log tails and minidumps. `eileen-guard.exe` writes minidumps from outside the crashing or hung process (Embark's crash-handler 0.8.1 and minidumper 0.11.0), hang-dumps a sidecar before the supervisor kills it, and restarts a hung core at most once per 10 minutes.

Replay levels: L1 replays the core logic from a recording in the simulator and reports the first divergent event; L2 adds the real speech process on recorded audio; L3 is a live re-run by hand. `eileen replay inc_... --level core` tells logic bugs from model, desktop or timing problems. A redaction canary test injects fake secrets and fails if any sink writes them where policy forbids.

Code navigation for people and Eileen's own Claude workers: the generated `docs/CODEMAP.md` (crate and process graph, module trees, and the event map of which code constructs and handles each `BusEvent` and `Input` variant), and a read-only toolset `dev` on the `eileen` MCP server (`dev_codemap`, `dev_crate_doc`, `dev_event_map`, `dev_trace_get`, `dev_logs_query`, `dev_incident_list`, `dev_incident_read`, `dev_replay_core`, `dev_scenario_list`) that applies the share-pack redaction policy to everything it returns. rust-analyzer reaches at most one worker at a time through a repo-local `eileen-dev` plugin with `checkOnSave` off, so it never compiles outside the build-slot guard.

### 2.14 Connectors and platforms

**Connectors.** A connector is a capability provider: it publishes a `ConnectorManifest` (section 3.3.12) with its tools (name, schema, tier per tool), its event streams, its tier-0 check tools for recipes, where it runs (a task in the core for pure network code, or a supervised sidecar for anything with FFI or drivers), and whether it senses the room. The router, the permission gate, the job registry and the learning recorder see only manifests and tool calls. v0.1 ships two: `pc.desktop` and `pc.apps`. The phone (Telegram and ntfy, then voice from anywhere), Home Assistant (and Matter through it), other devices on the user's network and earbuds as microphones are new connectors with the same tiers, learning loop and job model. Audio-producing connectors deliver 16 kHz frames into the same VAD, ASR and gate, each with its own echo handling. Room-sensing connectors are opt-in per device, processed locally, and contribute only derived events to trajectories; the registry refuses to start one without a recorded opt-in.

**Windows first, not Windows only.** Every OS-touching piece sits behind traits in `eileen-platform` (section 3.4) and is implemented in a per-area Windows crate (`eileen-audio-win`, `eileen-desktop-win`, `eileen-apps-win`, `eileen-shell-win`). The sidecar binaries hold platform-neutral server logic and pick the implementation by target. A Linux port means new `-linux` crates (AT-SPI, PipeWire, Wayland or X11 input and capture, MPRIS over D-Bus, Secret Service), not a redesign. Every platform-neutral crate builds, lints and tests on `ubuntu-latest` in CI from the first commit; Windows-only crates compile to an empty library on other targets.

### 2.15 Data directory

```
%LOCALAPPDATA%\eileen\
  config\           eileen.toml, profiles\<name>.toml, eileen.schema.json
  eileen.db         jobs, events (with seq and TraceCtx), approvals, settings_versions, journal, plan_usage
  learning\         learn.sqlite (trajectories, skills, skill_versions, uses, facts, ledger, answers cache, tombstones)
                    eileen-learned\ (.claude-plugin\plugin.json, skills\<name>\SKILL.md, reference\, recipe.json)
                    pending\ (drafts waiting for approval, not loaded by workers)
  models\           vad\, stt\, turn\, tts\, voice\, embed\, face\ (downloaded by eileen setup, SHA256-checked)
  voices\           <name>\ voice.toml, reference.wav, consent.wav, state cache
  phrases\          <voice>\<hash>.pcm (stock phrase cache)
  logs\             <process>.<date>.jsonl
  incidents\        <time>_inc_<id>\ ...; pending\
  recordings\       rec_<id>\ manifest.toml, inputs.jsonl.zst, audio\ (opt-in)
  blobs\            <2 hex>\<blake3>.zst (shared by recordings, incidents and trajectories)
  scenarios\        saved\<id>\ (local scenarios), outbox\<id>\ (redacted, approved for sharing)
  fleet\            results\<date>.jsonl, machine.toml (random install id, never hardware ids)
  worktrees\        <repo-hash>\<job-slug>\
  run\              bus.json, brain\, <job>\ (claude-settings.json, mcp.json, cwd\), guard socket
```

Retention defaults: trajectories 30 days unless cited by a skill (200 MB cap), ledger 14 days, recordings 7 days and 2 GB, incidents 30 days and 1 GB, logs 14 days and 500 MB. A janitor in the core enforces them; "delete my recordings" and "forget that" remove data at once. `EILEEN_DATA_DIR` (and `eileen run --data-dir`) points everything at another root, which every test uses.

## 3. Workspace and contracts

### 3.1 The Cargo workspace

One workspace (resolver 3, edition 2024, `rust-version = "1.99"`, toolchain pinned to 1.99.0), flat `crates/*`, `publish = false` everywhere. The root manifest uses `members = ["crates/*", "xtask", "tools/codemap"]` and `exclude = ["fuzz"]`, so a package adds or fills its crate without touching the root manifest. Every third-party dependency is declared once in `[workspace.dependencies]` (section 4.1); members write `foo.workspace = true` and may add features. Internal crates are declared there too.

WP0 creates every crate below as a documented stub that compiles on Windows and Linux: the crate doc with the guidance sections (section 4b.4), the public types and signatures this section specifies, and bodies that return a typed `Unavailable` error or do nothing. When WP0 has merged, each stub belongs to the package named in the table, which replaces the stub body and keeps the signatures (or changes them through a BUILD.md amendment in its pull request).

| Crate | Kind | Platform | Process | Unsafe | Owner | May depend on (internal) |
|---|---|---|---|---|---|---|
| `eileen-contracts` | lib | neutral | all | forbid | WP0 | none |
| `eileen-stdx` | lib | neutral | all | forbid | WP0 | contracts |
| `eileen-trace` | lib | neutral | all | forbid | WP0 | contracts, stdx |
| `eileen-platform` | lib | neutral | all | forbid | WP0 | contracts |
| `eileen-supervisor` | lib | neutral, Windows module | all | `ffi` only | WP0 | contracts, trace |
| `eileen-sim` | lib, dev only | neutral | tests | forbid | WP1 (skeleton WP0) | core, record, intent, dictation, contracts, stdx, trace |
| `eileen-core` | lib | neutral | core | forbid | WP1 | contracts, stdx, trace (the resolver, dictation cleaner and settings validator arrive through ports in `eileen-contracts`) |
| `eileen-record` | lib | neutral | core, tools | forbid | WP1 | contracts, trace |
| `eileen-runtime` | lib | neutral | core | forbid | WP2 | core, record, config, mcp, supervisor, contracts, stdx, trace |
| `eileen-mcp` | lib | neutral | core, sidecars, shim | forbid | WP2 | contracts, trace |
| `eileen-crash` | lib | neutral, Windows module | all | `ffi` only | WP2 | contracts, trace |
| `eileen-cli` | bin `eileen` | neutral | core | forbid | WP2 | runtime, core, config, harness, audio, learn, intent, dictation, mcp, record, crash, trace, shell-win (Windows target) |
| `eileen-shim` | bin `eileen-shim` | neutral | shim | forbid | WP2 | mcp, contracts, trace |
| `eileen-guard` | bin `eileen-guard` | neutral, Windows module | guard | none of its own | WP2 | crash, contracts, trace |
| `eileen-harness` | lib | neutral | core | forbid | WP3 | supervisor, contracts, trace |
| `eileen-stub-harness` | bin, dev only | neutral | tests | forbid | WP3 | harness, contracts |
| `eileen-audio` | lib | neutral | core | forbid | WP4 | platform, contracts, stdx, trace; audio-win (Windows target) |
| `eileen-audio-win` | lib | Windows | core, speech | `ffi` only | WP4 | platform, contracts |
| `eileen-speech` | lib and bin | neutral logic, Windows models | speech | forbid | WP4 | supervisor, contracts, trace, audio-win (Windows target) |
| `eileen-dictation` | lib | neutral | core | forbid | WP4 | contracts |
| `eileen-desktop` | lib and bin | neutral logic | desktop | forbid | WP5 | mcp, platform, supervisor, contracts, trace; desktop-win (Windows target) |
| `eileen-desktop-win` | lib | Windows | desktop | `ffi` only | WP5 | platform, contracts |
| `eileen-apps` | lib and bin | neutral logic | apps | forbid | WP6 | mcp, platform, supervisor, contracts, trace; apps-win (Windows target) |
| `eileen-apps-win` | lib | Windows | apps | `ffi` only | WP6 | platform, contracts |
| `eileen-intent` | lib | neutral | core | forbid | WP7 | contracts, stdx |
| `eileen-learn` | lib | neutral | core | forbid | WP7 | record, contracts, stdx, trace |
| `eileen-config` | lib | neutral | core | forbid | WP7 | contracts, trace |
| `eileen-hud` | lib and bin | neutral layout | hud | forbid | WP8 | contracts, trace; shell-win (Windows target) |
| `eileen-shell-win` | lib | Windows | core, hud | `ffi` only | WP8 | platform, contracts |
| `eileen-e2e` | lib and bin, dev only | neutral, Windows fixtures | tests, fleet | forbid | WP9 | record, supervisor, mcp, config, contracts, trace |
| `xtask` | bin, not shipped | neutral | tools | forbid | WP0 | none (std only) |
| `tools/codemap` | bin, not shipped | neutral | tools | forbid | WP0 | none |

Rules the table encodes, each checked by tidy (section 4b.3):

- `eileen-core`, `eileen-intent`, `eileen-dictation`, `eileen-record`, `eileen-learn` and `eileen-config` link no FFI crate and nothing that can hang. `eileen-core` performs no IO at all.
- Only `eileen-mcp` (and the binaries through it) knows rmcp. Only `eileen-harness` knows vendor CLI flags and protocols. Only `eileen-contracts` types are serialized across a process, pipe, file or socket.
- `-win` crates, and the Windows modules of `eileen-supervisor`, `eileen-crash` and `eileen-guard`, are gated with `#![cfg(windows)]` (or a `cfg(windows)` module), so `cargo clippy --workspace` and the neutral tests run on Linux.
- Unsafe code lives only in a private module named `ffi` (`ffi.rs` or `ffi/`) declared with `#[expect(unsafe_code, reason = "...")]`; one unsafe operation per block, each with a `// SAFETY:` comment.

Non-crate directories and their owners:

| Path | Contents | Owner |
|---|---|---|
| root files, `.cargo/`, `.config/`, `.github/`, `.githooks/`, `.claude/` (settings, rules, skills) | Workspace, gates, CI, templates, agent settings | WP0 |
| `AGENTS.md`, `CLAUDE.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`, `TESTING.md`, `SECURITY.md`, `NOTICE` | Guidance and policy | WP0 (TESTING.md completed by WP9) |
| `docs/CODEMAP.md`, `docs/SCENARIOS.md`, `docs/reference/` | Generated by `cargo xtask codegen`; any package regenerates them, nobody edits them by hand | generator |
| `docs/wp/WP<n>-<topic>.md` | Living work package file | that package |
| `docs/DEBUGGING.md` | Debugging guide (observability report section 7) | WP2 |
| `fuzz/Cargo.toml`, `fuzz/README.md` | Fuzz workspace with every target from section 4.7 pre-declared | WP0 |
| `fuzz/fuzz_targets/<target>.rs`, `fuzz/corpus/<target>/`, `fuzz/regressions/<target>/` | One target each | the package owning the parser (section 4.7) |
| `scenarios/README.md`, `scenarios/_schema/`, `scenarios/core/` | Scenario format, examples | WP0 |
| `scenarios/<area>/` | Scenarios per area | `feel/`, `jobs/` WP1; `runtime/` WP2; `claude/` WP3; `voice/`, `dictation/` WP4; `desktop/` WP5; `apps/` WP6; `learning/`, `settings/` WP7; `hud/`, `claude-plugin/` WP8; `bench/` WP9 |
| `tests/harness-scripts/` | Recorded stream-json scripts for the stub harness | WP3 |
| `adapters/resolve/` | Lua bridge, DRX templates | WP6 |
| `integrations/claude-plugin/`, `.claude-plugin/marketplace.json` | The `eileen` Claude plugin | WP8 |
| `tools/claude-plugins/eileen-dev/` | Dev plugin for Eileen's own coding workers | WP0 |

`Cargo.lock` is shared. Packages only add entries for dependencies already declared in the workspace. After a merge conflict, take `main`'s lockfile and run `cargo xtask lock` (which runs `cargo update --workspace` and fails if any third-party version changed).

### 3.2 What stays Python, and the Python scaffold

Nothing in the v0.1 build is Python. The rust-architecture report measured why: the same one-tool adapter answered in 1.7 to 2.9 s as a stdlib-only JSON-RPC shim and in 37 to 69 s with the Python MCP SDK on the loaded PC, and a bare interpreter took 4.5 to 6.9 s to start. Resolve, the one place Python looked unavoidable, runs through a Lua bridge under Resolve's own interpreter. If a later adapter needs Python (the Chatterbox pre-render sidecar in v0.2, or Resolve's fusionscript fallback), it is a stdlib-only JSON-RPC shim at startup, run with `python -I` from a uv venv pinned by its own lockfile, long-lived and supervised, never a global install.

The Python scaffold under `src/eileen`, `tests/*.py`, `pyproject.toml` and the Python CI job is retired by WP0 in the same pull request that adds the workspace. Its pydantic schemas are superseded by `eileen-contracts`, and `policy/default.yaml` by the TOML defaults. Before deleting, WP0 tags the last commit that has it as `python-scaffold-final`, so the history stays one command away.

### 3.3 Contracts

Everything here lives in `eileen-contracts` unless a path says otherwise. Every serialized type derives `serde` and `schemars`; every wire shape and tool input schema is pinned by an insta snapshot, so any change is a reviewed diff. Maps that reach an effect or a snapshot are `BTreeMap` or `IndexMap`, never `HashMap`. Field additions inside a sub-enum belong to the package that owns the producing component and update the snapshots; a new top-level `Input`, `Effect` or `BusEvent` variant is an architectural change and needs a BUILD.md amendment.

#### 3.3.1 Identifiers, time and correlation

```rust
// ids.rs: prefixed ULIDs. Display writes "job_01K8...", FromStr requires the prefix.
// The core mints them from MonoTime plus the injected RNG; drivers go through an `Ids` port.
macro_rules! prefixed_ulid { ($name:ident, $prefix:literal) => { /* newtype over ulid::Ulid */ } }
prefixed_ulid!(SessionId, "sess");   prefixed_ulid!(TurnId, "turn");   prefixed_ulid!(JobId, "job");
prefixed_ulid!(CallId, "call");      prefixed_ulid!(SpeakId, "speak"); prefixed_ulid!(ApprovalId, "appr");
prefixed_ulid!(IncidentId, "inc");   prefixed_ulid!(RecordingId, "rec"); prefixed_ulid!(ProposalId, "prop");

/// Agent Skills name: 1 to 64 chars, lowercase letters, digits, hyphens.
pub struct SkillId(String);
/// Scenario folder slug: lowercase letters, digits, hyphens.
pub struct ScenarioId(String);
/// Tool name, `^[a-z][a-z0-9_]{2,62}$`, unique across connectors (prefix rule: desktop_, apps_, jobs_, ...).
pub struct ToolName(String);
/// Connector id, for example "pc.desktop", "pc.apps", later "phone", "home".
pub struct ConnectorId(String);

/// Monotonic nanoseconds since the core started. Only the clock port creates it.
pub struct MonoTime(pub u64);
/// Microseconds since the Unix epoch, UTC.
pub struct WallTime(pub i64);

/// Correlation carried by every bus event, IPC frame and tool call.
pub struct TraceCtx { pub turn: Option<TurnId>, pub job: Option<JobId>, pub call: Option<CallId> }

/// Every bus event, input and effect travels in this envelope. `seq` is the core-wide replay order.
pub struct Envelope<E> { pub seq: u64, pub at: WallTime, pub mono: MonoTime, pub ctx: TraceCtx, pub body: E }
```

`TraceCtx::traceparent()` renders W3C `00-<32 hex of the turn ULID, or the job ULID>-<16 hex span>-01`.

#### 3.3.2 Shared enums and job types

```rust
pub enum Route { Claude, Codex, Grok, DeepSeek, Local }
impl Route {
    /// The only place a worker's user-facing name is defined: "Claude", "Codex", "Grok", "DeepSeek", "Local model".
    pub const fn display_name(self) -> &'static str;
    /// v0.1: true only for Claude.
    pub const fn built_in_v0_1(self) -> bool;
}
pub enum Tier { Observe = 0, Reversible = 1, Consequential = 2, Blocked = 3 }
pub enum Depth { Instant, Quick, Considered, Deep }
pub enum JobKind { Coding, ComputerUse, Research, DeepAnswer, Librarian, Replay, ContinueSession }
pub enum JobStatus { Queued, Running, Blocked, AwaitingApproval, Completed, Failed, Cancelled, Interrupted }
pub enum FanoutMode { Compete, Split }
pub enum Toolset { Brain, Worker, Desktop, Apps, Learn, Session, Dev }
pub enum TurnSource { Voice, Typed, Hud, Hotkey, Tray, ClaudeSession, Scenario, Connector(ConnectorId) }
pub enum CallOrigin {
    Brain { turn: TurnId },
    Worker { job: JobId, route: Route },
    Replay { job: JobId, skill: SkillRef },
    FastPath { turn: TurnId },
    Session,            // the owner's own Claude session through the plugin
    Core,               // Eileen's own bookkeeping calls (checks, intent highlight)
}

pub struct JobSpec {
    pub kind: JobKind,
    pub task: String,                     // up to 8,000 characters
    pub title_hint: Option<String>,
    pub depth: Depth,
    pub route: Option<Route>,             // None: policy decides
    pub model: Option<String>,            // full model id; None: policy decides
    pub fanout: u8,                       // 1..=8
    pub mode: FanoutMode,
    pub subtasks: Vec<String>,            // Split mode: one per worker
    pub subquestions: Vec<String>,        // DeepAnswer: at most 4
    pub repo: Option<String>,             // repo name or absolute path
    pub resume_session: Option<String>,   // ContinueSession only
    pub skill: Option<SkillRef>,          // set when a learned skill drives the job
    pub origin_turn: Option<TurnId>,
    pub parent: Option<JobId>,            // fan-out children point at their parent
}

pub struct Job {
    pub id: JobId, pub name: String, pub slug: String, pub spec: JobSpec,
    pub status: JobStatus, pub route: Route, pub model: String,
    pub created: WallTime, pub updated: WallTime,
    pub last_progress: Option<String>,
    pub salience_pct: u8,                 // drives announcement order
    pub worktree: Option<String>, pub branch: Option<String>,
    pub cost: Cost, pub grants: Vec<Grant>, pub focus: bool,
    pub override_words: Option<String>,   // the spoken override that routed it, verbatim
}

pub struct Cost { pub duration_ms: u64, pub api_ms: u64, pub model_turns: u32,
                  pub tokens_in: u64, pub tokens_out: u64, pub tokens_cache_read: u64 }
pub struct Grant { pub tool_pattern: String, pub tier: Tier, pub scope: GrantScope, pub granted_by: String }
pub enum GrantScope { ThisJob, Always /* tier 1 only, or tier 2 with a logged decision id */ }
pub struct PlanUsage { pub five_hour_pct: Option<u8>, pub seven_day_pct: Option<u8>, pub resets_at: Option<WallTime> }
```

#### 3.3.3 Inputs and effects of the core

The core is a state machine with one entry point. Drivers turn the outside world into `Input` and carry out `Effect`; they hold no decisions.

```rust
// eileen-core
impl Core {
    pub fn new(config: CoreConfig, snapshot: Option<CoreSnapshot>) -> Self;
    pub fn step(&mut self, now: MonoTime, rng: &mut CoreRng, input: Envelope<Input>, effects: &mut Vec<Envelope<Effect>>);
    pub fn snapshot(&self) -> CoreSnapshot;   // checkpoint for the flight recorder and replay
}
```

```rust
pub enum Input {
    Voice(VoiceEvent),
    Playback(PlaybackEvent),
    Brain(BrainEvent),
    Worker(JobId, WorkerEvent),
    Hook(JobId, HookRequest),
    ToolCall(ToolCallRequest),            // from a shim, a replay or an internal origin
    ToolResult(CallId, ToolResult),       // from a sidecar or the core-side executor
    Sidecar(SidecarId, SidecarEvent),
    Learn(LearnEvent),
    User(UserInput),                      // hotkeys, HUD, tray, typed text; never screen or model text
    Settings(SettingsEvent),
    Store(StoreEvent),
    Timer(TimerId),
}

pub enum VoiceEvent {
    SpeechStarted { turn: TurnId },
    Partial { turn: TurnId, text: String, stable: bool },
    SpeechEnded { turn: TurnId },
    EndOfTurn { turn: TurnId, text: String, smart_turn_pct: u8, silence_ms: u32, reason: TurnEndReason },
    BargeInWindow { speak: SpeakId, window: u8, prob_pct: u8 },     // only while Eileen speaks
    ClauseReady { speak: SpeakId, clause: u16, chars: u32, samples: u32 },
    SpeakDone { speak: SpeakId },
    Embedding { request: u64, vector: Vec<f32> },
    CheckResult { request: u64, result: SpeechCheck },             // name recognisability, speaker match
    Health { rtf_by_engine: BTreeMap<String, u16> },               // per-mille real-time factor
}
pub enum PlaybackEvent {
    Played { speak: SpeakId, clause: u16, samples_played: u32 },   // at most every 50 ms
    Stopped { speak: SpeakId, clause: u16, samples_played: u32 },
    Drained { speak: SpeakId },
    DeviceChanged, Stalled,
}
pub enum BrainEvent {
    Ready { which: BrainSlot, model: String },
    TextDelta { turn: TurnId, text: String },
    TurnDone { turn: TurnId, cost: Cost, context_tokens: u64, is_error: bool },
    RateLimit(PlanUsage),
    Exited { which: BrainSlot, code: Option<i32> },
}
pub enum WorkerEvent {
    Started { session_id: String, pid: u32, model: String },
    Narration { line: String },
    Text { delta: String },
    ToolUse { tool_use_id: String, name: String, input_summary: String },
    ToolResult { tool_use_id: String, ok: bool },
    TurnDone { cost: Cost, is_error: bool, summary: String },
    RateLimit(PlanUsage),
    Malformed { line_len: u32 },
    Exited { code: Option<i32> },
}
pub enum HookRequest {
    PreToolUse { request: u64, tool_name: String, tool_input: serde_json::Value, tool_use_id: String },
    Notification { kind: String, message: String },
    PermissionDenied { tool_name: String }, PostToolUseFailure { tool_name: String, error: String },
    SubagentStart, SubagentStop, Stop { last_message: Option<String> }, StopFailure { error: String },
}
pub struct ToolCallRequest {
    pub caller: Caller,                    // which shim session or internal origin, to route the reply
    pub origin: CallOrigin,
    pub tool: ToolName,
    pub args: serde_json::Value,
    pub traceparent: Option<String>,
}
pub struct ToolResult { pub content: Vec<Content>, pub is_error: bool, pub verification: Option<Verification>, pub duration_ms: u32 }
pub enum Content { Text(String), Image { mime: String, base64: String } }   // never structured content beside an image
pub struct Verification { pub kind: String, pub observed: serde_json::Value }
pub enum SidecarEvent { Ready { manifest: ConnectorManifest }, Heartbeat, Event(ConnectorEvent), Exited { code: Option<i32> } }
pub enum ConnectorEvent { Desktop(DesktopDelta), Media(MediaState), App { app: String, event: serde_json::Value } }
pub enum LearnEvent {
    SkillsChanged { skills: Vec<SkillMeta>, phrasings: Vec<PhrasingFact> },
    Changed(LearnBusEvent),               // drafted, promoted, demoted, forgot, replay step, handoff
    ReplayFinished { job: JobId, outcome: ReplayOutcome },
}
pub enum UserInput {
    SetMute(bool), ToggleMute, Approve(ApprovalId), Deny(ApprovalId), CancelJob(JobId),
    Typed { text: String }, Hud(HudInput), DictationKey(KeyEdge), SaveIncident, SaveScenario,
}
pub enum SettingsEvent { FileChanged { valid: bool, error_line: Option<u32> }, ChecksDone { proposal: ProposalId, checks: Vec<CheckResult> } }
pub enum StoreEvent { Loaded { jobs: Vec<Job>, settings_version: u64 } }
```

```rust
pub enum Effect {
    Speech(SpeechCmd),          // Speak { speak, voice, clauses }, Cancel { speak }, SetMode(ListenMode), SetHotwords,
                                // Embed { request, text }, Check { request, check }, LoadModel, UnloadModel, Enroll
    Audio(AudioCmd),            // SetCapture(bool), Duck { db }, Unduck, Stop { speak }, PlayPhrase { key, voice }, Earcon(Earcon)
    Brain(BrainCmd),            // Turn { turn, message }, Interrupt, Recycle { summary }
    Worker(JobId, WorkerCmd),   // Start(WorkerSpec), Prompt { text }, Interrupt, Cancel, RemoveWorktree
    Hook(JobId, HookReply),     // PreToolUse { request, decision: HookDecision }
    ToolForward { call: CallId, connector: ConnectorId, tool: ToolName, args: serde_json::Value, deadline_ms: u32 },
    ToolReply { caller: Caller, call: CallId, result: ToolResult },
    Sidecar(SidecarId, SidecarCmd),   // Start, Restart, HangDumpThenKill
    Hud(HudState),              // full snapshot; the driver rate-limits to 10 per second
    Learn(LearnCmd),            // Begin, Step, Signal, Finish, RunLibrarian, Replay { job, skill, slots }, Forget, Rollback
    Settings(SettingsCmd),      // RunChecks { proposal }, WriteVersion(SettingsVersion), ApplyLive(Vec<SettingKey>)
    Persist(Record),            // JobUpsert, EventAppend, ApprovalRow, JournalRow, SettingsVersion, PlanUsage
    Emit(BusEvent),
    SetTimer { id: TimerId, at: MonoTime }, CancelTimer(TimerId),
    Incident(IncidentTrigger),
    Notify(Toast),
}
pub enum HookDecision { None, Ask { reason: String }, Deny { reason: String } }   // never Allow
```

#### 3.3.4 Bus events

Published to the HUD, the loopback endpoint, the Claude plugin and the `events` table, each in an `Envelope`.

```rust
pub enum BusEvent {
    TurnStarted { source: TurnSource },
    Heard { text: String, is_final: bool, addressed: Option<bool> },
    TurnRouted { route: TurnRoute, reason: String },     // FastPath, Replay, Brain, Clarify, Ignored, Dictation
    Speaking { speak: SpeakId, cause: SpeakCause, text: String },
    SpeakingStopped { speak: SpeakId, heard_chars: u32, interrupted: bool },
    MuteChanged { muted: bool },
    BrainReady { model: String },
    JobAccepted { job: JobId, name: String, kind: JobKind, route: Route, model: String, reason: String },
    JobStarted { job: JobId },
    JobNarration { job: JobId, line: String },
    JobProgressed { job: JobId, line: String },
    JobBlocked { job: JobId, reason: String },
    ApprovalRequested { approval: ApprovalId, job: Option<JobId>, tier: Tier, summary: String },
    ApprovalResolved { approval: ApprovalId, allowed: bool, by: ApprovalSource },
    JobCompleted { job: JobId, summary: String, artifacts: Vec<String> },
    JobFailed { job: JobId, error: String },
    JobCancelled { job: JobId, by: CancelSource },
    JobInterrupted { job: JobId },
    DesktopIntent { rect: Rect, label: String, until_ms: u32 },
    InputLock { holder: Option<JobId> },
    PlanUsage(PlanUsage),
    Learn(LearnBusEvent),
    SettingChanged { key: String, from: String, to: String, version: u64 },
    RecordingChanged { on: bool },
    IncidentCaptured { id: IncidentId },
    SidecarRestarted { which: SidecarId, reason: String },
    Lagged { missed: u64 },
}
```

`JobNarration`, `JobCompleted` and friends replace the old `Task*` names. Event names in the past tense, commands in the imperative.

#### 3.3.5 DesktopState

The October version of the old ARCHITECTURE section 2 object (physical pixels, virtual-screen coordinates):

```rust
pub struct DesktopState {
    pub captured_at: WallTime,
    pub monitors: Vec<Monitor>,          // ordinal, device_path, friendly_name, rect, work_rect, dpi, is_primary
    pub windows: Vec<WindowInfo>,        // hwnd (i64), pid, process, title (untrusted), class, monitor_ordinal,
                                         // frame_rect, z_order, is_minimized, is_cloaked, is_foreground, is_eileen
    pub foreground: Option<WindowRef>,
    pub focused_element: Option<ElementInfo>, // control_type, name, automation_id, rect, patterns, is_password, window
    pub selection: Option<Selection>,    // text (at most 4,000 chars, untrusted), source: TextPattern | ClipboardFallback
    pub caret: Option<Rect>,
    pub recent_references: Vec<Reference>,   // at most 8, newest first
    pub presentation: PresentationState, // Normal, Busy, FullScreen, Presentation, QuietHours
    pub mic_in_use_elsewhere: bool,
    pub now_playing: Option<MediaState>, // merged in by the core from pc.apps
    pub degraded: Vec<String>,           // fields that timed out: unknown, not empty
}
pub struct DesktopDelta { pub seq: u64, pub changes: Vec<DesktopChange> }
```

`eileen_contracts::desktop::sanitize(&str, max_chars) -> Untrusted<String>` is the only way text from UIA enters a contract type.

#### 3.3.6 HUD state

```rust
pub struct HudState {
    pub v: u16,
    pub layout: HudLayout,               // corner, scale_pct, opacity_pct, show_transcript, high_contrast
    pub mic: MicState,                   // Listening, Hearing { level_pct }, Muted, Dictating { target }, SpeechRestarting
    pub heard: Option<HeardLine>,        // text, is_final, addressed: Option<bool>
    pub speaking: Option<String>,
    pub jobs: Vec<HudJob>,               // at most 8: id, name, route display name, status, line, focus
    pub approvals: Vec<HudApproval>,     // id, summary, tier, seconds_left
    pub intent: Option<HudIntent>,       // rect, label
    pub input_lock: Option<String>,      // job name holding the mouse and keyboard
    pub panel: Option<HudPanel>,         // Markdown from the brain's ---hud--- part, learned list, scenario preview
    pub suggestions: Vec<String>,        // at most 3
    pub indicators: Indicators,          // learning, recording, off_the_record, plan_usage_pct, brain_warming
}
pub enum HudInput { ToggleMute, Approve(ApprovalId), Deny(ApprovalId), CancelJob(JobId), SaveScenario, DismissPanel, ApproveSkillLine { skill: SkillId, line: u32 } }
```

Wire: core to HUD `{"v":1,"state":{...}}` per line; HUD to core `{"v":1,"input":{...}}` per line; at most 64 KiB per line.

#### 3.3.7 The speech pipe

Named pipe `\\.\pipe\eileen-speech-<session>` (Unix socket under `$XDG_RUNTIME_DIR` on Linux tests), user-only ACL, created by the core, opened by `eileen-speech` with a token from its environment.

```rust
#[repr(C)] pub struct FrameHeader { pub kind: u8, pub flags: u8, pub reserved: u16, pub len: u32, pub seq: u64, pub turn: [u8; 16] }
// kind 1 Audio   core -> speech  512 samples of i16, 16 kHz mono (one 32 ms window), len = 1024
// kind 2 Command core -> speech  JSON SpeechCmd
// kind 3 Event   speech -> core  JSON VoiceEvent
// kind 4 Pcm     speech -> core  JSON PcmHeader { speak, clause, sample_rate } then i16 samples, at most 1 MiB
// kind 5 Ping, kind 6 Pong       heartbeat every 1 s
```

`len` is validated against the kind's maximum before any allocation (fuzz target `speech_frame`). PCM goes straight to the audio driver; the core only sees `ClauseReady` and `Played`.

#### 3.3.8 Worker and brain drivers

```rust
// eileen-harness
pub trait WorkerDriver: Send {
    fn route(&self) -> Route;
    fn start(&mut self, spec: WorkerSpec) -> impl Future<Output = Result<WorkerSession, DriverError>> + Send;
    fn send(&mut self, cmd: WorkerCmd) -> impl Future<Output = Result<(), DriverError>> + Send;
    /// None when the session has ended and every event has been delivered.
    fn next_event(&mut self) -> impl Future<Output = Option<WorkerEvent>> + Send;
}
pub enum AnyWorker { Claude(ClaudeWorker), Codex(CodexWorker), Acp(AcpWorker) }   // enum dispatch, no dyn
pub fn worker_for(route: Route, cfg: &HarnessConfig) -> Result<AnyWorker, DriverError>; // RouteUnavailable unless built

pub struct WorkerSpec {
    pub job: JobId, pub kind: JobKind, pub route: Route, pub model: String, pub effort: Option<Effort>,
    pub cwd: String, pub worktree: Option<WorktreeSpec>, pub append_system_prompt: String,
    pub toolsets: Vec<Toolset>, pub allowed_tools: Vec<String>, pub plugin_dirs: Vec<String>,
    pub resume_session: Option<String>, pub trace: TraceCtx, pub hook_endpoint: HookEndpoint,
    pub shim: ShimEndpoint,             // pipe name and per-session token for the generated MCP config
}

pub trait BrainBackend: Send {
    fn turn(&mut self, turn: TurnId, message: String) -> impl Future<Output = Result<(), DriverError>> + Send;
    fn interrupt(&mut self) -> impl Future<Output = Result<(), DriverError>> + Send;
    fn recycle(&mut self, summary: String) -> impl Future<Output = Result<(), DriverError>> + Send;
    fn next_event(&mut self) -> impl Future<Output = Option<BrainEvent>> + Send;
}
```

| Operation | Claude (v0.1) | Codex app-server (slot) | ACP, for Grok and others (slot) |
|---|---|---|---|
| Start | Spawn `claude -p` with the flags of section 2.5 through the supervisor; wait for `system/init`; read `session_id`, model, tools, MCP status, `capabilities` | `initialize`, `initialized`, `thread/start` (`baseInstructions`, `developerInstructions`, `model`, `approvalPolicy`, `ephemeral`) | `initialize`, `session/new` with stdio `mcpServers` (every ACP agent must accept stdio MCP) |
| User turn | One stream-json `{"type":"user",...}` line | `turn/start` | `session/prompt` |
| Steer | Interrupt, then a user turn | `turn/steer` | `session/cancel`, then `session/prompt` |
| Interrupt | `control_request` subtype `interrupt` (feature-detected from `system/init`) | `turn/interrupt` | `session/cancel` |
| Cancel | Kill the Job Object | Kill the Job Object | Kill the Job Object |
| Approvals | `--permission-prompt-tool mcp__eileen__approve` plus the `PreToolUse` HTTP guard | `item/commandExecution/requestApproval`, `item/fileChange/requestApproval` answered by the core | `session/request_permission` answered by the core |
| Tool events | `assistant` tool_use blocks, `user` tool_result blocks, hook events | `item/*` notifications | `session/update` |
| Narration | Text lines starting with `> ` | Same convention in agent messages | Same convention |
| Cost and usage | `result` event (`duration_ms`, `duration_api_ms`, `num_turns`, `usage`) | turn completion | n/a |
| Rate limits | Rate-limit stream events | `account/rateLimits/read` | n/a |
| Eileen's tools | Generated `--mcp-config` with the shim, `--strict-mcp-config` | MCP server config at `thread/start` | stdio `mcpServers` in `session/new` |
| Learned skills | `--plugin-dir eileen-learned` | Unverified (Codex not installed); check when added | Per agent |
| Sign-in check | `claude auth status` exit code | `account/read` | agent's own status command |

Every vendor process is spawned by `eileen_supervisor` in a Job Object; the ACP crate's own `process` feature is never used (it does not job-wrap on Windows), and the Codex types are generated from `codex app-server generate-json-schema` when that driver is built.

#### 3.3.9 MCP tools

One MCP server name, `eileen`, served by `eileen-shim` and forwarded to the core's dispatcher, so Claude sees `mcp__eileen__<tool>`. Each session's hello names its role, and the core answers with the toolsets for that role; `tools/list` shows only those.

| Toolset | Who gets it | Tools |
|---|---|---|
| `brain` | the brain | `jobs_dispatch`, `jobs_status`, `jobs_control`, `desktop_state`, `desktop_look`, `capabilities`, `settings_get`, `settings_propose`, `settings_apply`, `settings_undo`, `skills_query`, `skills_forget`, `ledger_search`, `scenario_save`, `ask_user`, `hud_show` |
| `worker` | every worker | `approve` (the permission-prompt tool), `ask_user`, `report` |
| `desktop` | computer-use workers | the 17 `desktop_*` tools of section 2.6 |
| `apps` | computer-use workers | `apps_media`, `apps_open_url`, `apps_resolve`, `apps_office`, `apps_explorer`, `apps_recent`, `apps_image`, `apps_photoshop` |
| `learn` | the librarian only | `learn_trajectory_read`, `learn_skill_list`, `learn_skill_read`, `learn_skill_propose` |
| `session` | the owner's own Claude sessions, through the plugin | `session_status`, `session_jobs`, `session_say`, `session_mute`, `session_handoff`, `session_ask` |
| `dev` | coding workers in an Eileen worktree | `dev_codemap`, `dev_crate_doc`, `dev_event_map`, `dev_trace_get`, `dev_logs_query`, `dev_incident_list`, `dev_incident_read`, `dev_replay_core`, `dev_scenario_list` |

Built-in tools per job kind (`--allowedTools`, before the tier guard narrows them): coding and continue_session get `Read`, `Grep`, `Glob`, `Edit`, `Write`, `Bash`, `WebSearch`, `WebFetch` and `mcp__eileen__*`; computer_use gets `Read` and `mcp__eileen__*`; research and deep_answer get `Read`, `WebSearch`, `WebFetch` and `mcp__eileen__*`; the librarian gets `--tools ""` and `mcp__eileen__learn_*`.

Every tool has a `ToolSpec`:

```rust
pub struct ToolSpec {
    pub name: ToolName, pub connector: ConnectorId, pub toolset: Toolset,
    pub description: String,             // at most 600 characters, written for the model
    pub tier: TierRule,                  // Fixed(Tier) or ByArgs (the server computes it per call)
    pub read_only: bool,                 // usable as a recipe check (tier 0 only)
    pub needs_foreground: bool,          // takes the input lock
    pub timeout_ms: u32,
    pub input_schema: serde_json::Value, // generated by schemars from the argument struct below
}
```

Argument structs (the JSON Schemas are generated from these and snapshot-tested; `hwnd` is an `i64` because `HWND` is not `Send`; coordinates are physical virtual-screen pixels):

```rust
// brain toolset
pub struct JobsDispatch { pub kind: JobKind, pub task: String, pub title_hint: Option<String>, pub depth: Option<Depth>,
    pub fanout: Option<u8>, pub mode: Option<FanoutMode>, pub subtasks: Vec<String>, pub subquestions: Vec<String>,
    pub repo: Option<String>, pub route: Option<Route>, pub model: Option<String> }
    // returns { jobs: [{ job, name }], ack: String }    tier 1 (the core asks first when plan usage is high or fanout > 2 above 80 percent)
pub struct JobsStatus { pub job: Option<JobRef> }                       // JobRef: id, slug or name; None = all live jobs. Tier 0
pub struct JobsControl { pub job: JobRef, pub action: JobAction }        // Cancel | Interrupt | Steer { text } | Reroute { route, model, depth } | Focus. Tier 1
pub struct DesktopStateArgs { pub detail: Detail, pub since_seq: Option<u64> }   // Summary | Full. Tier 0
pub struct DesktopLook { pub target: LookTarget, pub max_edge_px: Option<u32> }  // Screen | Foreground | Window { hwnd }; default 1280. Tier 0
pub struct Capabilities { pub scope: CapabilityScope }                   // FocusedApp | Everything. Tier 0
pub struct SettingsGet { pub keys: Vec<String> }                         // empty: a short summary. Tier 0
pub struct SettingsPropose { pub changes: Vec<SettingChange>, pub utterance: String }   // returns a Proposal (3.3.11). Tier 0
pub struct SettingsApply { pub proposal: ProposalId }                    // tier per the proposal's safety bound
pub struct SettingsUndo { pub steps: Option<u8>, pub to_version: Option<u64> }           // tier 1
pub struct SkillsQuery { pub about: Option<String>, pub since: Option<String>, pub limit: Option<u8> }   // tier 0
pub struct SkillsForget { pub target: ForgetTarget }                     // Skill { id } | LastUsed | Job { job }. Tier 2
pub struct LedgerSearch { pub query: String, pub since_hours: Option<u32>, pub limit: Option<u8> }      // tier 0
pub struct ScenarioSave { pub which: SaveTarget, pub failed: bool, pub expected: Option<String> }      // Last | Turn { turn } | Job { job }. Tier 1
pub struct AskUser { pub question: String, pub options: Vec<String>, pub timeout_s: Option<u16> }      // at most 140 chars, 4 options. Tier 0
pub struct HudShow { pub markdown: String, pub title: Option<String> }  // at most 8 KiB. Tier 0

// worker toolset
pub struct Approve { pub tool_name: String, pub input: serde_json::Value, pub tool_use_id: Option<String> }
    // replies with one text block holding {"behavior":"allow","updatedInput":{...}} or {"behavior":"deny","message":"..."}
    // (verify the exact reply shape against the CLI reference when WP3 builds it)
pub struct Report { pub summary: String, pub artifacts: Vec<String>, pub outcome: ReportOutcome }      // Done | Blocked | Failed

// desktop toolset
pub struct DesktopWindows { pub include_minimized: bool }                                             // tier 0
pub struct DesktopTree { pub hwnd: i64, pub query: Option<String>, pub max_depth: Option<u8>, pub max_elements: Option<u16> }   // 12 and 250 by default. Tier 0
pub struct DesktopScreenshot { pub target: ShotTarget, pub marks: bool, pub max_edge_px: Option<u32> }  // Screen | Monitor { ordinal } | Window { hwnd } | Region { rect }. Tier 0
pub struct DesktopClickElement { pub hwnd: i64, pub element: u32, pub selector: Option<Selector> }    // tier 1, or 2 for Send-like names
pub struct DesktopSetValue { pub hwnd: i64, pub element: u32, pub value: String }                    // tier 1; refuses password fields
pub struct DesktopTypeText { pub hwnd: i64, pub element: Option<u32>, pub text: String }             // tier 1; foreground
pub struct DesktopPressKeys { pub hwnd: i64, pub keys: String }                                       // "ctrl+s"; tier 1, blocked combos tier 3
pub struct DesktopClick { pub hwnd: i64, pub x: i32, pub y: i32, pub button: MouseButton, pub count: u8 }   // tier 1; foreground
pub struct DesktopScroll { pub hwnd: i64, pub x: i32, pub y: i32, pub dx: i32, pub dy: i32 }          // tier 1
pub struct DesktopDrag { pub hwnd: i64, pub from: Point, pub to: Point, pub button: MouseButton }    // tier 1; foreground
pub struct DesktopWindow { pub hwnd: i64, pub action: WindowAction }
    // Focus | Minimize | Maximize | Restore | Move { rect } | Snap { monitor, zone, fraction_pct }. Tier 1
pub struct DesktopLaunchApp { pub name: String, pub dry_run: bool }       // tier 1; ambiguous names return candidates
pub struct DesktopWaitFor { pub condition: WaitCondition, pub timeout_ms: u32 }   // at most 30,000. Tier 0
pub struct DesktopSelection { pub hwnd: Option<i64> }                     // tier 0
pub struct DesktopInsertText { pub text: String, pub target: InsertTarget, pub category: Option<FieldCategory> }   // tier 1
pub struct Selector { pub process: String, pub window_class: Option<String>, pub automation_id: Option<String>,
    pub control_type: Option<String>, pub name: Option<String>, pub ancestors: Vec<SelectorStep> }

// apps toolset
pub struct AppsMedia { pub action: MediaAction, pub session: Option<String> }
    // Status | Play | Pause | Toggle | Next | Previous | Seek { seconds } | SeekBy { seconds }. Tier 0 for Status, else 1
pub struct AppsOpenUrl { pub url: String, pub wait_for_media_title: Option<String> }   // https only. Tier 1
pub struct AppsResolve { pub op: ResolveOp }
    // Hello | TimelineInfo | Goto { offset_s: f64 } | GotoTimecode { tc } | OpenPage { page } | CurrentItem
    // | ExportStill | ApplyDrx { template, window: Option<Rect> } | MarkerAdd { offset_s, name, color } | Markers. Tier 0 or 1
pub struct AppsImage { pub op: ImageOp }      // Faces { path } | PickFace { path, which: Position } | Sharpness { path, rect }. Tier 0
pub struct AppsOffice { pub op: OfficeOp }    // WordSelection | ExcelActiveCell. Tier 0
pub struct AppsExplorer { pub op: ExplorerOp }   // Selection | OpenFolder { path }. Tier 0 or 1
pub struct AppsRecent { pub query: String, pub kinds: Vec<FileKind>, pub limit: Option<u8> }           // tier 0
pub struct AppsPhotoshop { pub op: PhotoshopOp }
    // Recent | Open { path } | DuplicateLayer | SelectEllipse { rect, feather_px } | SmartSharpen { amount_pct, radius_px }. Tier 1

// learn toolset
pub struct LearnTrajectoryRead { pub job: JobId }
pub struct LearnSkillRead { pub id: SkillId }
pub struct LearnSkillPropose { pub doc: SkillDoc, pub reason: CommitReason, pub evidence: Vec<JobId> }

// session toolset
pub struct SessionHandoff { pub session_id: String, pub cwd: String, pub summary: String, pub next: String }
pub struct SessionSay { pub text: String }     // at most 200 characters
pub struct SessionMute { pub muted: bool }
pub struct SessionAsk { pub question: String }

// dev toolset (read-only, redacted)
pub struct DevCrateDoc { pub krate: String }
pub struct DevEventMap { pub event: String }
pub struct DevTraceGet { pub id: String, pub max_lines: Option<u16> }      // at most 400
pub struct DevLogsQuery { pub filter: String, pub since_minutes: Option<u32>, pub limit: Option<u16> }
pub struct DevIncidentRead { pub id: IncidentId, pub part: IncidentPart }
pub struct DevReplayCore { pub id: String }
```

Shim to core protocol: named pipe `\\.\pipe\eileen-dispatch-<session>`, user-only ACL, JSON lines (at most 16 MiB per line, for images). First line `{"hello":{"v":1,"token":"...","role":"brain"|{"worker":"job_..."}|"librarian"|"session"|"dev"}}`, answered with `{"welcome":{"tools":[ToolSpec...]}}`; then `{"call":{"id":n,"tool":"...","args":{...},"meta":{...}}}` answered by `{"result":{"id":n,"content":[...],"is_error":false}}`. A wrong token closes the pipe. The shim never decides anything.

#### 3.3.10 The intent resolver

```rust
// eileen-intent
pub struct Resolver { /* grammar table, skill templates, BM25 index, confirmed phrasing pairs */ }
impl Resolver {
    pub fn new(grammar: Grammar) -> Self;
    pub fn rebuild(&mut self, skills: &[SkillMeta], phrasings: &[PhrasingFact]);
    /// L1 and the lexical part of L2; pure and deterministic.
    pub fn resolve(&self, utterance: &str, ctx: &ResolveCtx) -> Resolution;
    /// L2 with an embedding the core requested from the speech process.
    pub fn resolve_semantic(&self, utterance: &str, embedding: &[f32], ctx: &ResolveCtx) -> Resolution;
}
pub struct ResolveCtx { pub focused_app: Option<String>, pub media_active: bool, pub dictation: bool,
    pub awaiting: Option<Awaiting>, pub assistant_name: String, pub monitors: u8 }
pub enum Resolution {
    Command { command: CoreCommand, slots: SlotValues },        // L1 grammar: mute, cancel, undo, ...
    Tool { tool: ToolName, args: serde_json::Value },           // L1 grammar or a confirmed phrasing: open app, seek, snap
    Skill { skill: SkillRef, slots: SlotValues, level: Level }, // L1 template; level 3 or 4 replays
    Ambiguous { question: String, options: Vec<String> },
    Hints(Vec<(SkillRef, u16)>),                                // L2: for the brain only
    Brain { depth: Option<Depth> },                             // L3, with any depth override
}
pub fn introspect(tools: &[ToolSpec], app_caps: &[AppCapability], skills: &[SkillMeta], focused: Option<&str>) -> Capabilities;
```

Latency budgets, tested in CI with loose bounds and measured on the owner's PC: `resolve` under 1 ms p99 with 5,000 skills and the full grammar; `resolve_semantic` under 5 ms p99 for the comparison; the embedding round trip under 20 ms p95 for a 12-word utterance (WP4 measures; if it misses, L2 runs lexical only and says so in `eileen doctor`). The candidate embedding model is a small sentence-embedding model of the all-MiniLM-L6-v2 class (Apache-2.0, about 23 MB as int8 ONNX); WP4 verifies the license, size, digest and latency before adopting it and records the result.

#### 3.3.11 Settings and configuration

```rust
// eileen-contracts::config: the whole configuration as types; doc comments become the commented default file
pub struct Config {
    pub assistant: AssistantConfig,   // name ("Eileen"), personality (free text, at most 400 chars), verbosity, humor
    pub voice: VoiceConfig,           // live, narration, announcements, fallback, rate_pct, watermark_exports
    pub hud: HudConfig,               // corner, scale_pct, opacity_pct, show_transcript, suggestions, visible_intent_ms
    pub announcements: AnnouncementsConfig,   // per event kind: Interrupt | WhenIdle | Silent | HudOnly; per job kind overrides
    pub routing: RoutingConfig,       // brain model ids, per kind: route, model, depth; hot_spare; recycle thresholds
    pub permissions: PermissionsConfig,       // grants (tier 1, or tier 2 with a logged decision), private apps
    pub hotkeys: HotkeysConfig,       // mute, report, dictation
    pub dictation: DictationConfig,   // polish, categories, key, languages
    pub learning: LearningConfig,     // enabled, librarian_daily_cap, regret_window_s, ladder thresholds
    pub privacy: PrivacyConfig,       // location, off_the_record, ledger_days, include_personal_in_logs
    pub focus: FocusConfig,           // quiet_during_calls, quiet_full_screen
    pub logging: LoggingConfig, pub flight: FlightConfig, pub incidents: IncidentsConfig, pub recording: RecordingConfig,
    pub fleet: FleetConfig,           // opt_in, schedule, upload
    pub dev: DevConfig,               // max_lsp_workers
}

// eileen-config
pub struct SettingSpec {
    pub key: &'static str,            // "assistant.name"
    pub doc: &'static str,
    pub voice: bool, pub hud: bool,
    pub risk: Risk,                   // Low | Misfire(CheckKind) | Safety(SafetyBound)
    pub apply: ApplyMode,             // Live | Restart(Component)
}
pub enum CheckKind { NameRecognisable, HotkeyFree, VoiceRealtime, None }
pub enum SafetyBound { Tier1Only, Tier2WithLoggedDecision, Never }
pub struct SettingChange { pub key: String, pub value: serde_json::Value }
pub struct Proposal { pub id: ProposalId, pub changes: Vec<SettingChange>, pub diff: Vec<DiffLine>,
    pub checks: Vec<CheckResult>, pub confirm_sentence: String, pub needs_yes: bool, pub utterance: String }
pub struct SettingsVersion { pub version: u64, pub at: WallTime, pub source: SettingsSource, pub utterance: Option<String>,
    pub diff: Vec<DiffLine>, pub snapshot_toml: String }
pub enum SettingsSource { Voice, Hud, File, Profile(String), Undo, Default }

pub fn registry() -> &'static [SettingSpec];
pub fn load(dir: &Path) -> Result<(Config, Vec<Diagnostic>), ConfigError>;          // every error names a line
pub fn validate(current: &Config, changes: &[SettingChange]) -> Result<Proposal, ConfigError>;
pub fn apply(current_toml: &str, changes: &[SettingChange]) -> Result<String, ConfigError>;   // keeps comments (toml_edit)
pub fn explain(config: &Config, layers: &Layers) -> Vec<Explained>;
```

The flow itself (propose, check, confirm, apply, version, undo) is a state machine in `eileen-core::settings`, so it is simulated like everything else; `eileen-config` provides the pure parts and the file IO used by the runtime's driver.

#### 3.3.12 Connectors

```rust
pub struct ConnectorManifest {
    pub id: ConnectorId, pub version: String,
    pub runs: RunsIn,                    // CoreTask | Sidecar { binary: String }
    pub platforms: Vec<Platform>,        // Windows, Linux, MacOs
    pub tools: Vec<ToolSpec>,
    pub events: Vec<String>,             // event kinds it publishes
    pub checks: Vec<ToolName>,           // read-only tools recipes may use as checks
    pub senses_room: bool,               // camera, room microphone: opt-in per device, local only
    pub requires_opt_in: bool,
}
pub struct AppCapability { pub app: String, pub verbs: Vec<String>, pub version: Option<String> }
```

Each sidecar answers a `connector_manifest` call (core-only, never shown to models) right after the MCP handshake.

#### 3.3.13 Learning types

The types of the learning-loop report section 6.2 are adopted as written, with these changes: `Route` has no Gemini variant; `TrajectoryStep` gains `call: CallId` (the join key with recordings) and `tool` is a plain `ToolName`; `SkillMeta` gains `phrasings: Vec<String>` (confirmed L2 pairs); and version history is a table:

```rust
pub struct SkillVersion { pub skill: SkillId, pub version: u32, pub at: WallTime, pub reason: CommitReason,
    pub evidence: Vec<JobId>, pub doc: SkillDoc }           // full content of SKILL.md, references and recipe
pub enum CommitReason { Distill, Repair, UserEdit, Rollback, Promote, Demote }
pub struct PhrasingFact { pub utterance: String, pub target: PhrasingTarget, pub embedding: Vec<f32>, pub confirmed: u16 }
```

`eileen-learn` exposes `Recorder`, `SkillRepo`, the `LadderPolicy` with `next_level` as a pure function, the replay engine (which issues `ToolCallRequest`s with origin `Replay` and therefore passes the same gate as a worker), the librarian scheduler, the user model and the ledger. The matcher itself lives in `eileen-intent`.

#### 3.3.14 Scenarios

The scenario format is a contract because packages, contributors and CI all write and read it. `scenarios/<area>/<id>/scenario.toml`, validated against a JSON Schema generated from these types into `scenarios/_schema/scenario.schema.json`:

```rust
pub struct Scenario {
    pub schema: u16,                     // 1
    pub id: ScenarioId, pub title: String, pub area: String, pub tags: Vec<String>,
    pub origin: ScenarioOrigin,          // Synthetic | SyntheticFromUser | ContributedConsentedAudio
    pub contributor: Option<String>,     // GitHub handle, never an email
    pub intent: ScenarioIntent,          // utterances (text, placeholders allowed), optional consented audio, stock voice
    pub environment: ScenarioEnv,        // min Windows build, requires (capabilities), lanes: Sim | Desktop | Bench
    pub fixtures: Fixtures,              // files from templates, apps to launch, window layout
    pub steps: Vec<Step>,                // Say { utterance } | Type { text } | Hotkey { keys } | Hud { action } | WaitFor { event, timeout_ms } | Approve | Deny | Pause { ms }
    pub expect: Vec<Expectation>,
    pub timing: BTreeMap<String, TimingBudget>,   // "turn.ended -> job.completed" = { max_ms, ci_factor }
    pub sim: Option<SimReplay>,          // inputs = "replay/inputs.jsonl.zst", expected = "replay/expected.trace"
    pub quarantine: Option<Quarantine>,  // issue link and expiry date; required for a known-flaky scenario
}
pub enum Expectation {
    Event { pattern: String },           // matched against the event trace with [ANY], [ANYLINE], [UNORDERED]
    Call { tool: ToolName, args_contain: serde_json::Value },
    Spoken { contains: String },  NotSpoken,
    JobStatus { status: JobStatus },
    WindowOnMonitor { process: String, ordinal: u8 },
    FieldText { selector: Selector, contains: String },
    File { path: String, sha256: Option<String>, contains: Option<String> },
    ResolvePlayhead { timecode: String, tolerance_frames: u32 },
    MediaPosition { seconds: f64, tolerance_s: f64 },
    SharpnessIncrease { min_pct: u16 },
    SettingEquals { key: String, value: serde_json::Value },
}
```

Example:

```toml
schema = 1
id = "resolve-goto-timecode"
title = "Go to a time in the Resolve timeline"
area = "apps/resolve"
tags = ["resolve", "navigation"]
origin = "synthetic"

[intent]
utterances = ["Open Resolve and go to 15:30", "uh go to fifteen thirty in resolve"]
voice = "kokoro:af_heart"            # used when a runner needs audio; the text is the source of truth

[environment]
requires = ["resolve-studio>=21.0"]
lanes = ["sim", "desktop"]

[fixtures]
apps = [{ launch = "resolve", project = "fixtures/eileen-test.drp" }]

[[steps]]
say = 0

[[steps]]
wait_for = "turn.ended"
timeout_ms = 20000

[[expect]]
kind = "resolve_playhead"
timecode = "01:15:30:00"
tolerance_frames = 0

[[expect]]
kind = "call"
tool = "apps_resolve"
args_contain = { op = { goto = { offset_s = 930.0 } } }

[timing]
"turn.ended -> speaking.started" = { max_ms = 2500, ci_factor = 3.0 }

[sim]
inputs = "replay/inputs.jsonl.zst"
expected = "replay/expected.trace"
```

Sim-lane expectations are the ones the simulator can check (events, calls, spoken text, job status, settings); desktop-lane expectations need a real desktop and run on the owner's PC and on fleet PCs.

#### 3.3.15 Ports

The interfaces that let `eileen-core` stay free of every other crate, and let the runtime compose components it does not know. They use only std types, so `eileen-contracts` needs no async runtime.

```rust
// eileen-contracts::ports
/// Implemented by eileen_intent::Resolver. Pure and deterministic; called inside Core::step.
pub trait IntentResolver: Send {
    fn rebuild(&mut self, skills: &[SkillMeta], phrasings: &[PhrasingFact]);
    fn resolve(&self, utterance: &str, ctx: &ResolveCtx) -> Resolution;
    fn resolve_semantic(&self, utterance: &str, embedding: &[f32], ctx: &ResolveCtx) -> Resolution;
    fn introspect(&self, tools: &[ToolSpec], app_caps: &[AppCapability], focused: Option<&str>) -> Capabilities;
}
/// Implemented by eileen_dictation::Cleaner. Pure.
pub trait DictationCleaner: Send {
    fn clean(&self, raw: &str, category: FieldCategory, vocabulary: &[VocabularyEntry]) -> Cleaned;
}
/// Implemented by eileen_config::Validator. Pure: validation, diff and safety bounds, no file IO.
pub trait SettingsValidator: Send {
    fn validate(&self, current: &Config, changes: &[SettingChange]) -> Result<Proposal, ValidationError>;
    fn specs(&self) -> &[SettingSpecView];
}

/// A component the runtime starts: audio, speech client, harness, sidecar clients, learning, HUD client.
pub trait Component: Send + 'static {
    fn name(&self) -> &'static str;
    /// Which effect families this component consumes (the runtime routes accordingly).
    fn accepts(&self) -> &'static [EffectFamily];
    fn start(self: Box<Self>, ctx: ComponentCtx) -> Pin<Box<dyn Future<Output = Result<(), ComponentError>> + Send>>;
}
pub struct ComponentCtx {
    pub inputs: Box<dyn InputSink>,       // bounded; returns Backpressure instead of growing
    pub effects: Box<dyn EffectSource>,
    pub data_dir: PathBuf,
    pub session: SessionId,
    pub shutdown: Box<dyn ShutdownSignal>,
}
pub trait InputSink: Send + Sync { fn send(&self, input: Envelope<Input>) -> Result<(), Backpressure>; }
pub trait EffectSource: Send { fn recv(&mut self) -> Pin<Box<dyn Future<Output = Option<Envelope<Effect>>> + Send + '_>>; }
pub trait ShutdownSignal: Send + Sync { fn wait(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>; }
```

Each component crate exposes one constructor, for example `eileen_harness::component(cfg: &HarnessConfig) -> Box<dyn Component>`, `eileen_audio::component(cfg)`, `eileen_learn::component(cfg)`. WP0's stubs return a component that accepts its effects and does nothing, so `eileen-cli` wires everything from the first day.

### 3.4 Platform traits

`eileen-platform` holds the traits every OS implements. Windows implementations live in the `-win` crates; fakes for tests live next to each trait under `cfg(any(test, feature = "fake"))`.

```rust
pub trait AudioEndpoints: Send + Sync {          // eileen-audio-win; cpal does the streaming itself
    fn default_capture(&self, category: StreamCategory) -> Result<EndpointInfo, PlatformError>;
    fn set_communications_ducking(&self, opt_out: bool) -> Result<(), PlatformError>;
    fn register_audio_thread(&self) -> Result<ThreadTicket, PlatformError>;   // MMCSS
}
pub trait Grounding: Send + Sync {               // eileen-desktop-win
    fn monitors(&self) -> Result<Vec<Monitor>, PlatformError>;
    fn windows(&self, include_minimized: bool) -> Result<Vec<WindowInfo>, PlatformError>;
    fn foreground(&self) -> Result<Option<WindowRef>, PlatformError>;
    fn focused_element(&self, deadline: Deadline) -> Result<Option<ElementInfo>, PlatformError>;
    fn selection(&self, deadline: Deadline) -> Result<Option<Selection>, PlatformError>;
    fn presentation_state(&self) -> Result<PresentationState, PlatformError>;
    fn mic_in_use_elsewhere(&self) -> Result<bool, PlatformError>;
    fn subscribe(&self, sink: Box<dyn Fn(DesktopChange) + Send + Sync>) -> Result<Subscription, PlatformError>;
}
pub trait Automation: Send + Sync {              // eileen-desktop-win (UIA on its own MTA thread)
    fn tree(&self, hwnd: i64, limits: TreeLimits, deadline: Deadline) -> Result<ElementTree, PlatformError>;
    fn invoke(&self, element: &ElementHandle, deadline: Deadline) -> Result<Verification, PlatformError>;
    fn set_value(&self, element: &ElementHandle, value: &str, deadline: Deadline) -> Result<Verification, PlatformError>;
    fn find(&self, hwnd: i64, selector: &Selector, deadline: Deadline) -> Result<Vec<ElementHandle>, PlatformError>;
}
pub trait Input: Send + Sync {                   // eileen-desktop-win
    fn user_idle_ms(&self) -> Result<u32, PlatformError>;
    fn take_foreground(&self, hwnd: i64) -> Result<ForegroundOutcome, PlatformError>;   // Taken | Refused
    fn send(&self, hwnd: i64, events: &[InputEvent], guard: ForegroundGuard) -> Result<(), PlatformError>;
    fn restore(&self, token: RestoreToken) -> Result<(), PlatformError>;
}
pub trait Capture: Send + Sync {
    fn capture(&self, target: &ShotTarget) -> Result<RgbaImage, PlatformError>;
}
pub trait Apps: Send + Sync {                    // index and launch
    fn index(&self) -> Result<Vec<AppEntry>, PlatformError>;
    fn launch(&self, app: &AppEntry) -> Result<u32, PlatformError>;     // pid
    fn open_url(&self, url: &str) -> Result<(), PlatformError>;
}
pub trait TextInsertion: Send + Sync {
    fn insert(&self, text: &str, target: &InsertTarget, method: InsertMethod) -> Result<InsertOutcome, PlatformError>;
}
pub trait MediaSessions: Send + Sync {           // eileen-apps-win (SMTC); Linux later: MPRIS
    fn sessions(&self) -> Result<Vec<MediaState>, PlatformError>;
    fn control(&self, session: &str, action: &MediaAction) -> Result<(), PlatformError>;
    fn subscribe(&self, sink: Box<dyn Fn(MediaState) + Send + Sync>) -> Result<Subscription, PlatformError>;
}
pub trait RecentSources: Send + Sync {
    fn recent(&self, limit: usize) -> Result<Vec<RecentItem>, PlatformError>;
    fn watch(&self, roots: &[String], sink: Box<dyn Fn(RecentItem) + Send + Sync>) -> Result<Subscription, PlatformError>;
}
pub trait ShellHost: Send + Sync {               // eileen-shell-win
    fn register_hotkey(&self, chord: &str, id: u32) -> Result<(), PlatformError>;   // fails if another app owns it
    fn set_tray(&self, state: TrayState) -> Result<(), PlatformError>;
    fn toast(&self, toast: &Toast) -> Result<(), PlatformError>;
}
pub trait HudSurface {                           // eileen-shell-win, driven by eileen-hud
    fn draw(&mut self, list: &DrawList) -> Result<(), PlatformError>;
    fn next_input(&mut self) -> Option<HudInput>;
}
```

COM automation of Office, Explorer, Photoshop and the Resolve process gate are Windows-specific adapters inside `eileen-apps-win` behind the `apps_*` tool contracts; a Linux port replaces them per app, not through a generic trait.

### 3.5 The command line

`eileen.exe` subcommands, wired in `eileen-cli` by WP0's stub and implemented in the owning crate's `cli` module:

| Command | What it does | Implemented in |
|---|---|---|
| `eileen run [--data-dir D] [--audio-source wav:F] [--audio-sink capture:F] [--stub-harness SCRIPT] [--muted] [--no-hud]` | Start everything | runtime (WP2) |
| `eileen say "<text>"` | Typed request into the running instance | runtime |
| `eileen status`, `eileen jobs [list \| tail ID \| cancel ID]` | Status from the loopback endpoint | runtime |
| `eileen setup`, `eileen doctor [resolve \| claude \| audio]` | Models, checks, Start menu entry, self-test | runtime with speech, harness, apps parts |
| `eileen config [explain \| edit \| undo \| profiles \| export \| import]` | Settings | config (WP7) |
| `eileen skills [list \| show \| history \| rollback \| forget]` | Learned skills | learn (WP7) |
| `eileen voice [list \| learn \| use \| delete]` | Voices | speech (WP4) |
| `eileen scenario [save \| list \| show \| share \| submit \| run]` | Scenario corpus | e2e (WP9); `save` in runtime |
| `eileen test --fleet [--once \| --nightly] [--filter]` | Fleet runner | e2e (WP9) |
| `eileen trace ID \| last`, `eileen logs`, `eileen incident [list \| show \| capture \| pack]` | Debugging | runtime |
| `eileen record [start \| stop \| list \| trim]`, `eileen replay ID --level core` | Recording and replay | record (WP1) and sim |
| `eileen debug [stats \| tasks \| dump]` | Live internals | runtime |
| `eileen claude [install \| uninstall]` | Prints the official plugin commands and runs them after a yes | harness (WP3) |
| `eileen mcp-probe`, `eileen bench` | Developer probes | runtime |

## 4. Pins, models, setup, budgets and tests

### 4.1 Dependency pins

WP0 writes this into the root `Cargo.toml`. The versions are the ones the rust-architecture, voice-stack, computer-use and observability reports verified on 2026-10-20 (the skeleton compiled and passed clippy, tests, cargo-deny and cargo-machete on the async, data, protocol and Windows rows). Several were released within two weeks of that date; they are the verified starting set, and the 14-day cooling-off rule applies to every update after WP0. Protocol and native-inference crates are pinned exactly because they change weekly.

```toml
[workspace.dependencies]
# Internal crates: one line each, path = "crates/<name>", for every crate in section 3.1.

# Async and plumbing
tokio = { version = "1.54.0", default-features = false }
tokio-util = { version = "0.7.20", default-features = false }

# Data and contracts
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
schemars = "1.2.3"
ulid = { version = "3.0.0", default-features = false, features = ["std", "serde"] }
toml = "1.1.8"
# toml_edit: the version toml 1.1.8 itself depends on, so the tree holds one copy (WP0 reads it from cargo tree)

# Errors, tracing, secrets
thiserror = "2.0.21"
anyhow = "1.0.104"
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", default-features = false, features = ["registry", "fmt", "json", "env-filter", "std", "tracing-log", "smallvec"] }
tracing-appender = "0.2.5"
tracing-error = "0.2.1"
secrecy = "0.10.3"
await-tree = { version = "0.3.1", features = ["tokio"] }

# Storage and recording
rusqlite = { version = "0.40.2", features = ["bundled"] }
blake3 = { version = "1.8.7", features = ["pure"] }
zstd = "0.14.3"
hound = "3.5.1"

# Protocols
rmcp = { version = "=3.6.0", default-features = false }
agent-client-protocol = { version = "=3.4.0", default-features = false }   # declared for the ACP slot; no v0.1 crate uses it

# Processes
process-wrap = { version = "10.0.1", default-features = false, features = ["tokio1", "job-object", "kill-on-drop", "creation-flags"] }

# Windows
windows = { version = "0.62.2", default-features = false }   # take core types from windows::core; never mix windows-core 0.100
windows-sys = "0.61.2"
tray-icon = { version = "0.26.1", default-features = false }   # drops the Linux libappindicator default
global-hotkey = "0.9.0"
tauri-winrt-notification = "0.8.1"
keyring-core = "1.0.1"
windows-native-keyring-store = "1.1.0"
crash-handler = "0.8.1"
minidumper = "0.11.0"

# Audio and speech
cpal = "0.18.2"
sonora = "0.2.0"
sherpa-onnx = { version = "=1.13.8", default-features = false, features = ["shared"] }   # static fails to link on MSVC 14.42
ort = { version = "=2.0.0-rc.13", default-features = false, features = ["std", "ndarray"] }   # rc.14 and rc.15 are too new
realfft = "3.5.0"
# rtrb: WP0 pins the newest non-yanked release crates.io lists (the observability report saw 0.4.0 and 0.3.5 listed inconsistently)

# Images
image = { version = "0.25.10", default-features = false, features = ["png", "jpeg"] }
fast_image_resize = "6.1.0"
jpeg-encoder = { version = "0.7.1", features = ["simd"] }   # license (MIT OR Apache-2.0) AND IJG: allow IJG in deny.toml, attribution in NOTICE
base64 = "0.23.1"

# Command line
clap = { version = "4.6.7", features = ["derive"] }

# Tests
insta = { version = "1.49.0", features = ["json", "redactions", "glob"] }
proptest = { version = "1.12.0", default-features = false, features = ["std"] }
proptest-state-machine = "0.9.0"
arbitrary = { version = "1.5.0", features = ["derive"] }
loom = "0.7.2"
```

Deliberately not used: `uiautomation` (it initialises COM on every construction; Eileen owns one MTA thread), `nucleo-matcher` (MPL-2.0; Eileen writes its own scorer), `xcap` (no gain over the own capture code), `gix` (skill history is in SQLite), `serde_yaml` and `serde_yml` (deprecated or unsound; config is TOML), `hyper` (the loopback endpoint is a small hand-written HTTP/1.1 server with strict limits and a fuzz target), the OpenTelemetry crates (v0.2, behind a feature). Any new dependency is an architectural change: say so in the pull request, and it must pass cargo-deny.

Tools, installed prebuilt (never `cargo install` from source; four tools took 59 minutes that way):

| Tool | Version | Use |
|---|---|---|
| cargo-nextest | 0.9.148 | tests |
| cargo-deny | 0.20.2 | licenses, advisories, bans, sources |
| cargo-machete | 0.9.2 | unused dependencies |
| cargo-insta | 1.49.0 | snapshot review |
| typos | 1.51.1 | spelling |
| sccache | 0.18.0 | local compile cache (owner installs once) |
| cargo-binstall | 1.26.0 | prebuilt installs (GPL-3.0-only; a tool we run, never link) |
| zizmor | 1.31.1, action 0.7.1 | workflow analysis |
| cargo-fuzz | 0.13.2 with a pinned nightly | fuzzing (Linux only) |
| cargo-mutants | 27.1.0 | mutation testing |
| cargo-llvm-cov | 0.9.1 | coverage |
| cargo-modules | 0.28.0 | nightly module maps (CI only) |
| dump_syms, minidump-stackwalk | 2.3.9, 0.27.0 | symbols and stack walks |
| Markdown linter, link checker | rumdl (Rust) and lychee: WP0 pins the newest release at least 14 days old and records the exact versions and licenses | docs checks |

GitHub Actions, pinned by commit SHA with the version in a comment: `actions/checkout` v7.0.1 `3d3c42e5aac5ba805825da76410c181273ba90b1`; `Swatinem/rust-cache` v2.9.3 `d1527c8f67ef9af357290475d4af4b0fab8f12e0`; `taiki-e/install-action` v2.87.29 `8cca0918f8471c33515202b73f56ab27f2bed283`; `actions/upload-artifact` v7.0.2 `cf430e030ddbb5b0abf93d22962f4752f3646cd9`; `dorny/paths-filter` v4.0.1 `fbd0ab8f3e69293af611ebaee6363fc25e6d187d`; `EmbarkStudios/cargo-deny-action` v2.1.1 `3c6349835b2b7b196a839186cb8b78e02f7b5f25`; `crate-ci/typos` v1.51.1 `7f9d0819ca1c008ade73f4b41fa9d258d48f605b`; `zizmorcore/zizmor-action` v0.7.1 `85b2b9fb8e3325e7a23d7d0a9d7ba5ae0f999e3e`. The toolchain comes from `rust-toolchain.toml` through `rustup show`. `windows-latest` is Windows Server 2025 with Rust 1.99.0, VS 2022 17.14 and SDK 10.0.26100.

Native libraries and the speech build. `.cargo/config.toml` sets `rustflags = ["-C", "target-feature=+crt-static"]` for `x86_64-pc-windows-msvc`. The speech and apps builds fetch `sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib.tar.bz2` (8,032,957 bytes, SHA256 `b8eedf41bd6d3779218887b48367bb7a3ece5aaa7667f01f69ee823a12b0a9e7`) into a cache directory through `cargo xtask fetch-native`, which verifies the hash, then build with `SHERPA_ONNX_ARCHIVE_DIR` pointing there, `ORT_LIB_LOCATION` at the extracted lib folder and `ORT_PREFER_DYNAMIC_LINK=1`, so `ort` links the same `onnxruntime.dll` and nothing downloads unverified binaries. Shipped beside the binaries: `onnxruntime.dll` 1.28.2 (17,799,168 bytes), `onnxruntime_providers_shared.dll` (104,960 bytes), `sherpa-onnx-c-api.dll` (4,605,952 bytes). Never rely on `PATH`: `System32` holds a different `onnxruntime.dll`. Before the first binary release, the espeak-ng license question (the prebuilt may contain GPL-3.0 espeak-ng) must be answered (section 7).

### 4.2 Models and `eileen setup`

`crates/eileen-speech/models.toml` (and `crates/eileen-apps/models.toml` for the face model) lists every model with URL, size, SHA256, license and residency. Full digests where the research recorded them; where only a prefix was recorded, the owning package fetches the full digest from the release API and pins it.

| Model | File | Size | SHA256 | License | Residency |
|---|---|---|---|---|---|
| Silero VAD v6.2 | `silero_vad.onnx` from snakers4/silero-vad | 2,327,524 B | `1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3` | MIT | always |
| Nemotron streaming EN 0.6B, 560 ms, int8 | `sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25.tar.bz2` | 463,945,051 B | `78e2b79fcf7271553a74402a76b771b09ea40117a39566a79f52235b23db6358` | NVIDIA Open Model License | always |
| Smart Turn v3.2 CPU | `smart-turn-v3.2-cpu.onnx` | 8,679,182 B | `2bb026316b14a660486a75b1733cd3fbab8c2fd0314dc9af7be49f8cca967e4f` | BSD-2-Clause | always |
| Kokoro v1.0 fp32 | `kokoro-multi-lang-v1_0.tar.bz2` | 349.9 MB | prefix `c5f7e2d2caf08`, full digest pinned by WP4 | Apache-2.0 | always |
| Pocket TTS int8 | `sherpa-onnx-pocket-tts-int8-2026-01-26.tar.bz2` | 98,336,520 B | `2f3b88823cbbb9bf0b2477ec8ae7b3fec417b3a87b6bb5f256dba66f2ad967cb` | CC-BY-4.0 (archive README says non-commercial; confirm, section 7) | while a cloned voice is selected |
| WeSpeaker CAM++ | `wespeaker_en_voxceleb_CAM++.onnx` | 29,292,684 B | `c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef` | per WeSpeaker repository, WP4 verifies | during enrollment, and always if speaker filtering is adopted |
| Parakeet TDT 0.6B v3 int8 | `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2` | 487,170,055 B | `5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf` | CC-BY-4.0 | on demand (multilingual dictation), unloaded after 5 idle minutes |
| Sentence embedding (candidate, all-MiniLM-L6-v2 class) | int8 ONNX plus `vocab.txt` | about 23 MB | pinned by WP4 if adopted | Apache-2.0, verified by WP4 | always if adopted |
| YuNet face detector | OpenCV Zoo `face_detection_yunet` ONNX | well under 1 MB | pinned by WP6 | MIT | loaded in `eileen-apps` on first use |

`eileen setup`, run once by the user (and safe to rerun):

1. Check Windows build, CPU, RAM and free disk; warn below 4 GB free disk or 8 GB RAM.
2. Find the installed `claude` binary (resolve to the real `.exe`; prefer the user's standalone install over copies bundled in other tools), check its version against the tested range, and run `claude auth status`. If it is not signed in, say "Please run `claude auth login` yourself, then run setup again" and stop that part. Never read credential files, never run a login.
3. Show every model's license and total size and ask once; download into `%LOCALAPPDATA%\eileen\models\`, verify each SHA256, and delete anything that fails.
4. Create the Start menu shortcut with Eileen's AppUserModelID (needed for toasts) and an optional "start at sign-in" entry.
5. Write the commented default `eileen.toml` if none exists.
6. Offer the Claude plugin (prints the two official commands, runs them after a yes) and the Playwright browser extension (link only).
7. Self-test: EcoQoS opt-out applied, capture and render devices open, models load, one Kokoro clause synthesizes, the brain session reaches `system/init`; then Eileen says hello.

`eileen doctor` repeats steps 1, 2 and 7 without changing anything, plus `eileen doctor resolve` (bridge `hello` when Resolve runs, and the exact preference to change if External scripting is not set to Local) and `eileen doctor audio` (devices, AEC mode, measured idle CPU).

### 4.3 Latency budget per stage

From the end of the user's speech, on a quiet machine. The voice-stack, brain-latency and computer-use reports are the sources; every row is measured again by WP9's benchmarks.

| Stage | Budget | Measured basis |
|---|---|---|
| Silence before the end-of-turn check | 200 ms (setting) | design |
| Smart Turn features plus model | 70 to 100 ms, overlapping the next row | four threads, 2026-10-11 |
| Nemotron tail flush (final transcript) | about 400 ms (740 ms if Parakeet is also loaded) | 2026-10-11, 2026-10-20 |
| Addressee gate plus L1 resolver | under 1 ms | matcher measurement |
| L2 semantic (embedding round trip plus comparison) | under 25 ms | to measure in WP4 and WP7 |
| Fast-path action | window move 8 to 10 ms; SMTC control 1 to 50 ms; app launch by AUMID to measure | computer-use, app-hooks |
| Cached phrase to first sound | one 10 ms buffer | 2026-10-20 |
| **Fast path, end of speech to first sound** | **about 0.5 to 0.6 s** | sum |
| Brain warm turn, first text | 1.0 to 1.5 s quiet (2.2 to 4.2 s busy) | brain-latency, connectors off |
| TTS first clause (three to six words) | 0.5 to 1.3 s | Kokoro RTF 0.54 quiet, 1.1 to 1.3 loaded |
| **Brain turn, end of speech to first word** | **about 2 to 3 s quiet** | sum |
| Barge-in, speech onset to silence | at most 150 ms (designed 70 to 110 ms) | measured parts |
| Mute, key to capture stopped | one 10 ms buffer | stop flag seen 1.5 ms after it was set |
| Dictation, key release to text in the field | 0.4 to 0.8 s (local path) | dictation report |
| Computer-use step (screenshot, model turn, action) | seconds per step, dominated by the model turn | claude read a screenshot in 3 turns and 6.2 s |
| Deep answer | tens of seconds; never blocks the conversation | design |

The owner asked for "within about a second" from the fast brain. The fast path and cached acknowledgements reach that; a synthesized brain answer takes about two seconds on this CPU because Kokoro's first clause costs 0.5 to 1.3 s. The levers, in order: a short first clause, cached phrases, the TTS experiments in section 7 (Pocket TTS v4 small models, Kokoro int8 under equal load), and speculative brain turns on stable partials (later).

### 4.4 Memory budget

| Process | Budget | Basis |
|---|---|---|
| `eileen.exe` | under 60 MB working set while idle | the debug skeleton idled at 26.6 MB |
| `eileen-speech.exe` | about 1.25 GB with VAD, Nemotron, Smart Turn and Kokoro; plus about 0.6 GB while a cloned voice is selected; plus about 0.75 GB while Parakeet is loaded | voice-stack |
| `eileen-desktop.exe`, `eileen-apps.exe`, `eileen-hud.exe` | under 60 MB each while idle | proof server 4.9 MB idle; to measure |
| Brain plus hot spare | two `claude -p` processes, 128 to 750 MB each as measured | harness and claude-mods reports |
| Each worker | one `claude -p` process | same |

On a 16 GB PC where the Claude app holds about 4 GB, a full v0.1 session with two workers fits but has little slack. Rules: one ASR model resident; the cloned voice engine and Parakeet load on demand; `brain.hot_spare` can be turned off; `routing.max_parallel_workers` defaults to 4; the HUD shows free memory in the developer panel.

### 4.5 Idle budget

Idle and listening, unmuted: under 2 percent of one core for the core process and under 8 percent of one core for the speech process (VAD alone measured 5.8 percent), no growth in working set, handles, GDI objects or threads after warm-up. The soak tests in section 4.7 enforce slopes, not absolute numbers.

### 4.6 What runs where

| Lane | Contents | Budget |
|---|---|---|
| Every push to a pull request | Linux: fmt, typos, cargo-deny, cargo-machete, zizmor, `cargo xtask lint-repo`, `cargo xtask codegen --check`, Markdown lint, internal links, clippy and nextest of neutral crates, simulator PR seeds and stored regressions, exhaustive turn-taking to depth 6, fuzz corpus replay, `cargo fuzz build` when a parser changed, loom when `eileen-audio` changed, scenario validation and L1 replay of every sim-lane scenario, `cargo mutants --in-diff` (advisory). Windows: clippy `-D warnings` on the workspace, nextest on the workspace, doc tests, end-to-end smoke scenarios with the stub harness and transcript injection. Plus `pr-meta` (commit messages, pull request body) and `claude-plugin` (validate and test, when the plugin directory exists) | under about 15 minutes with a warm cache |
| Merge to `main` | The pull request lane again (there is no merge queue on a user-owned repository), random simulator seeds for 10 minutes, a thin-LTO release smoke build | 20 minutes |
| Nightly | Simulator random seeds per profile (Linux, plus one Windows run), fuzzing 10 minutes per target, exhaustive depth 8, the full end-to-end set not marked `requires`, a 30-minute soak, flake hunt (`retries = 2`, `flaky-result = "fail"`), shuttle and loom suites, coverage, external link check, `cargo deny check --all-features`, build timings, fleet-results ingest and clustering. Failures open or update one issue per failing job, labelled `nightly`; ordinary test failures on `main` go to the `flake` issue | about 1 hour per job, in parallel |
| Weekly | Full mutation run (four shards), 4-hour soak, one hour per hot fuzz target, a dependency update pull request respecting the 14-day cooling-off period (protocol crates by hand with the probe subcommands as smoke tests) | hours |
| Release (tag `v*`) | Everything above on the tagged commit; the `dist` profile (fat LTO) on `windows-latest`; `.pdb` files and a Breakpad symbols zip; checksums; a license listing that includes the shipped native DLLs; end-to-end smoke from the built artifacts; a draft release the owner publishes | owner signs off |
| Owner's PC, nightly when idle (opt-in) | `cargo xtask nightly-local` from Task Scheduler with keep-awake: benchmarks B1 to B9 with real Claude within a nightly turn budget, the stream-json contract test, weekly an overnight soak; skips when free memory is under 2 GB or the owner used the PC in the last 30 minutes; results to the devhub store | the owner's plan limits |
| Contributors' PCs (opt-in) | `eileen test --fleet` (section 4.8) | the contributor's choice |

The owner's PC is never a self-hosted runner for this public repository: a fork's pull request could run code on it.

### 4.7 Test strategy

**Layers.**

| Layer | Proves | Tool | Runs on |
|---|---|---|---|
| Unit and property | One module, including invalid input | `#[test]`, proptest | every OS |
| Snapshot | Wire formats, tool schemas, HUD draw lists, user-facing text | insta (`--unreferenced reject`) | every OS |
| Integration per crate | A crate through its public API, one test binary at `tests/it/main.rs` | nextest | every OS for neutral crates, Windows for `-win` crates |
| Exhaustive | Turn-taking and barge-in for every input sequence up to a length | own enumerator | Linux |
| Deterministic simulation | The whole core under random inputs and injected faults, checked by invariants, replayable from a seed | `eileen-sim` | Linux on pull requests, both OSes nightly |
| Concurrency | The tokio glue and the audio ring buffer | paused tokio time, loom, shuttle (nightly) | Linux |
| Fuzzing | Untrusted parsers never panic, stay bounded, round-trip | cargo-fuzz, corpus replay tests | fuzzing on Linux, replay everywhere |
| End to end | Real Eileen processes, real desktop, recorded or synthesized audio in, event trace out | `eileen-e2e` | `windows-latest` with stubs; the owner's PC and fleet PCs with real Claude and apps |
| Timed scenarios | The owner's benchmark requests stay fast | `eileen-e2e --bench`, A/B against the base binary | the owner's PC |
| Soak | Memory, handles, GDI objects, threads, idle CPU, orphans, log and store sizes | `cargo xtask soak` | `windows-latest` nightly and weekly; the owner's PC weekly |
| Mutation and coverage | Tests check what they cover | cargo-mutants, cargo-llvm-cov | Linux, coverage also Windows |

**Simulation.** The core is built for it from the first commit (section 3.3.3): one `Core::step`, closed `Input` and `Effect` enums, time and randomness injected, ordered maps, every queue bounded, `buggify!` fault points activated per run (a runtime switch, never a Cargo feature), `never!` and `always!` soft assertions that log, count and dump the flight recorder in production and panic under tests. `eileen-sim` owns a `World` of simulated parts (microphone and speech, playback in 10 ms callbacks, brain with delay distributions and failures, workers replaying stub scripts with faults, sidecars with latency, hangs past the deadline and crashes, Job Object semantics, an in-memory model of the store that applies `Persist` records). Each run picks a profile, seeds a PRNG, generates inputs from an `arbitrary::Unstructured` buffer (so the same generator is a seeded test, a fuzz target and a minimiser), steps the core, applies effects to the world and checks every invariant after every step. A failure prints the replay command with the commit; `--doublecheck` runs a seed twice and compares trace hashes; minimised failures are stored as decoded input scripts in `crates/eileen-sim/regressions/` and replayed on every pull request; pull requests use a fixed seed set, `main` and nightly use fresh seeds. The simulator thread gets an explicit large stack (Windows' default main-thread stack is small).

Invariants checked after every step (each a named function):

1. The voice path is never starved: every voice input is consumed within two audio frames of virtual time.
2. Barge-in: after the first speech window while Eileen speaks, playback is ducked by the next output callback and stopped by the third window; onset to silence never exceeds 150 ms of virtual time.
3. What the brain is told the user heard equals the text mapped from samples actually handed to the device.
4. Every job ends in exactly one terminal state within its deadline plus the supervisor's kill bound; no job stays running without a live worker past one heartbeat timeout.
5. Job state changes follow the legal graph (also checked against a proptest-state-machine reference model).
6. Every bus event carries a turn ID or a job ID where it belongs; nothing is emitted for a job after its terminal event; per-job order is preserved; a slow subscriber gets an explicit `Lagged`, never silent loss.
7. A tool call above tier 1 is always preceded by an approval built from `UserInput`; a replayed skill never raises a tier; a learned skill never skips a confirmation.
8. A crashed or hung child restarts with bounded backoff, at most N times per window; killed children leave no descendants.
9. Mute stops audio reaching recognition within one frame; nothing is spoken while muted; "cancel" reaches the worker as an interrupt within its bound.
10. Two utterances never overlap; the announcement queue stays bounded.
11. Two speeds: the deep answer either replaces the fast one or goes to the HUD; the user never hears both in full; an interruption sends the deep answer to the HUD.
12. After a simulated core restart, running jobs are marked interrupted and no ID repeats.
13. The span-parent table holds for every span the run produced, and every log line inside a turn carries its turn ID.

Exhaustive tests enumerate every sequence of the turn-taking inputs (speech start, speech end, end of turn, clause ready, playback tick, mute, unmute, brain text) up to length 6 on pull requests and 8 nightly, checking invariants 2, 3, 9 and 10.

**End to end without a sound card or credentials.** `eileen-audio` defines `AudioSource` and `AudioSink` from day one: production uses WASAPI; tests use `WavSource { path, pace }`, a `CaptureSink` that writes what Eileen says to a WAV, and a `RoomMixer` that feeds a delayed, attenuated copy of playback back into the input so echo cancellation and barge-in see speaker bleed. `eileen run --audio-source wav:<file> --audio-sink capture:<file>` selects them. For pull requests, a transcript-injection speech stub replaces the models (the hosted runner has no audio device and the models are 1.3 GB); nightly runs the real speech process with cached models. `eileen-stub-harness` speaks the same stream-json as `claude -p` and plays scripts from `tests/harness-scripts/*.ndjson` with fault directives (delay, crash, malformed line, permission request); the scripts start as redacted recordings of real sessions on the owner's PC. A weekly contract test on the owner's PC runs the same prompts through the real CLI and compares event shapes (types and fields, not text) with the scripts, so CLI changes show up as a failing check. CI never holds vendor credentials. Desktop tests run in one nextest group with `max-threads = 1`, use only windows the test created (owned fixtures such as a corrected testpad, isolated Edge profiles, Notepad launched by AUMID and closed unsaved), and the first nightly proves the hosted runner gives UIA an interactive desktop before more depend on it.

**Benchmarks** (timed scenarios, phases measured from event-log spans, judged by A/B against the base binary with alternating runs and a Mann-Whitney test, reporting only differences that are significant and larger than about 10 percent; runs with under 2 GB free memory are recorded and excluded):

| ID | Request | Phases | Where |
|---|---|---|---|
| B1 | "Open Resolve, go to 15:30 and make the face of the girl on the right less blurry" | end of speech to Resolve focused; to playhead at 15:30 (read back through the bridge); to the correction applied; sharpness gain; total; worker turns | owner's PC |
| B2 | The Photoshop beach request | same shape | skipped until Photoshop is present (`requires = ["photoshop"]`) |
| B3 | "Launch three agents to ..." | each spawn; first narration per job; all finished; HUD shows all three | CI with stubs; owner's PC with real Claude |
| B4 | Writing code by speaking | end of speech to text in the editor; word error rate against the script | both |
| B5 | Interrupting Eileen mid-sentence | onset to silence; correctness of the heard prefix | both, strict on the owner's PC |
| B6 | "What's the best place to eat near me" | fast answer start; deep answer arrival; spoken length; HUD content | owner's PC (network, location opt-in) |
| B7 | Dictation into a field | key release to text; filler and self-correction accuracy | both |
| B8 | The same hard request twice | second run's time, tokens and worker turns against the first | owner's PC |
| B9 | "Open Premiere", "pause", "move this to the left screen" | end of speech to effect, fast path | both |

Every run appends one JSON line (scenario, phase, milliseconds, commit, binary hash, claude CLI version, free RAM, CPU load, whether the owner was active) to the devhub results store.

**Fuzz targets**, each calling a function in a neutral crate, each with a corpus that `cargo xtask fuzz-replay` runs as an ordinary test on every OS:

| Target | Input | Owner |
|---|---|---|
| `stream_json_line`, `control_message` | lines from `claude -p` | WP3 |
| `mcp_client_message`, `mcp_tool_arguments`, `dispatch_line` | JSON-RPC from sidecars, tool arguments from workers, shim pipe lines | WP2 |
| `worker_hook_payload`, `http_request` | hook bodies and the loopback HTTP parser (fail closed) | WP2 |
| `uia_text`, `desktop_state` | UIA strings through `sanitize`, DesktopState snapshots | WP0 (contracts) |
| `config_toml` | the user's config files | WP7 |
| `skill_record` | learned skill files and front matter | WP7 |
| `command_grammar` | transcripts into the fast path | WP7 |
| `speech_frame` | speech pipe frames | WP4 |
| `dictation_cleanup` | transcripts into the cleaner | WP4 |
| `hud_line` | HUD protocol lines | WP8 |
| `resolve_reply` | Resolve bridge replies | WP6 |
| `scenario_file`, `fleet_result` | contributed scenarios and fleet result lines | WP9 |
| `recording` | recording input lines | WP1 |
| `sim` | simulator scenarios from bytes, checked by every invariant | WP1 |

`cargo fuzz build` runs on every pull request that touches a parser crate; a crash is minimised into `fuzz/regressions/<target>/` with a focused regression test before its fix counts. A bug found any other way in a fuzzed parser is first treated as a gap in the fuzz target.

**Soak.** `cargo xtask soak` runs the real processes with WAV input and stub workers, plays a scripted request every few minutes, samples once a second, and judges slopes after a 10-minute warm-up: working set and private bytes (under 1 MB per hour for the core), handles, GDI and USER objects, threads, idle CPU (under about 1 percent of one core averaged), live children (exactly the expected set), log, SQLite and recorder sizes (within caps), and the scripted request's latency (no upward trend).

### 4.8 Real use becomes tests: the community scenario corpus and the test fleet

The owner's friends install Eileen on their own PCs and test it. Everything they and the owner try feeds the test suite, so the suite grows from real use instead of only from what developers think of. This is how it works end to end.

**Capture.** "Eileen, save that as a test", the HUD's Save button or `eileen scenario save [--last | --turn ID | --job ID] [--failed]` turns a request into a scenario folder under `%LOCALAPPDATA%\eileen\scenarios\saved\<id>\`:

1. The flight recorder's input lane is trimmed to that turn and its jobs, plus the checkpoint before it (`eileen record trim`), which gives the sim-lane replay (`replay/inputs.jsonl.zst`, `replay/expected.trace`).
2. The utterance is stored as text. The person's real voice is stored only if they explicitly allow it and record the consent sentence; otherwise runners regenerate audio with a stock voice.
3. The starting state becomes fixtures where Eileen can describe it generically (which app was focused, how many monitors, which app version), and steps come from the trajectory.
4. Expectations are inferred from what changed: a window moved becomes `window_on_monitor`; inserted text becomes `field_text`; a changed file becomes `file`; a Resolve playhead becomes `resolve_playhead`; a media seek becomes `media_position`; a setting becomes `setting_equals`; the dispatcher's calls become `call` expectations. The person can say "and it should also ..." to add one.
5. A failure is saved the same way with `outcome = "expected_fail"`; Eileen asks once "what should have happened?" and stores the answer as the scenario's `intent.expected`, and the incident bundle is attached locally.

**Generic and private.** Nothing leaves the PC until `eileen scenario share <id>`, which generalises and redacts: names, emails, account details, personal paths (replaced by `{HOME}`, `{DOCS}`, `{WORK}`), window titles (kept only as the app name), screen text (removed), secrets (the Redactor), screenshots (off by default). A preview shows exactly the files that would be shared, with every redaction highlighted, in the terminal and on the HUD. Only after the person approves is the result written to `scenarios\outbox\<id>\` with `origin = "synthetic-from-user"` (or `contributed-consented-audio` when audio is included with its consent record).

**Sharing.** `eileen scenario submit <id>` uses the contributor's own GitHub account through the `gh` CLI they signed in to themselves (Eileen never handles GitHub credentials, the same rule as for vendor CLIs): it forks if needed, creates a branch `scenario/<id>`, adds `scenarios/<area>/<id>/`, and opens a pull request with the scenario pull request template, after one confirmation (publishing is tier 2). Without `gh`, it copies the folder as a zip and opens the scenario issue form (`.github/ISSUE_TEMPLATE/scenario.yml`) so the contributor attaches it by hand. Maintainers move issue scenarios into pull requests.

**CI makes every shared scenario a permanent regression test.** On every pull request, `cargo xtask scenarios check` validates every scenario against the schema and the privacy rules (no `origin = "user"`, only placeholders in personal fields, no audio without a consent record and a synthetic or consented marker, size caps, an area that exists) and `cargo xtask scenarios replay` replays every sim-lane scenario at L1 in `eileen-sim`, checking its sim-checkable expectations. A scenario that cannot run in simulation is still validated and runs in the desktop lane.

**The fleet.** Contributors opt in once (`eileen test --fleet` asks for consent and writes `fleet.opt_in`). The runner pulls scenarios from the installed version's tag, keeps those whose `requires` the PC meets, runs them in a throwaway data directory and profile against owned fixtures (it never touches the user's documents or real browser profile), nightly through Task Scheduler or on demand, only when the user has been idle for 30 minutes and never while a call or full-screen app is active. Results go to `fleet\results\<date>.jsonl`: per scenario, pass or fail per expectation, phase timings, Windows build, monitor count, DPI per monitor, app versions, locale, CPU class, RAM and free RAM, whether the user became active, and a random install ID (never hardware IDs, user names or paths). The contributor sees the file and approves the upload, which posts it from their own account as a comment on the week's "Fleet results" issue (label `fleet`) through `gh`.

**Learning from it.** A nightly workflow ingests the week's fleet comments into the devhub store and clusters failures by signature (scenario, failing expectation, error kind, first divergent event, app version, Windows build); each cluster gets one issue with counts and machine classes. Triage by a Claude worker runs on the owner's PC (`cargo xtask triage <issue>`), reading the shared redacted data, the replay and the code map, and drafting an explanation that points at the code; the deterministic clustering stays in CI. A fixed bug always leaves its scenario behind: a pull request labelled `bug` must add or change a file under `scenarios/`, `fuzz/regressions/`, `crates/eileen-sim/regressions/` or a test file, or CI fails. Flaky scenarios (some failures and some passes on the same machine class over the last 20 runs) are listed in the generated catalogue and tracked in the `flake` issue; quarantine needs an issue link and an expiry date, and an expired quarantine fails CI. Learned skills will be checked across many machines through the fleet before they can ever be offered for sharing (v0.3).

**Visible.** `cargo xtask codegen` writes `docs/SCENARIOS.md`: scenarios grouped by app, each with the utterance people actually say, what it checks, its last result, how many machines passed it and whether it is flaky, plus a coverage view (app against capability, with scenario counts and machine counts). It doubles as a list of examples for new users.

**For testers.** `TESTING.md` (started by WP0, completed by WP9) tells friends how to install, what to try (a short list per app, taken from the catalogue), how to say "save that as a test", how to share a scenario, how to run the fleet suite, and exactly what leaves their PC.

**Who builds what.** WP0: the schema, the validation and privacy checks, the catalogue generator, the issue and pull request templates, the CI lanes, the first example scenarios. WP1: the L1 replayer and expectation checking in the simulator. WP2: `eileen scenario save` from the flight recorder. WP9: share, submit, the real-desktop runner, the fleet runner and upload, ingest, clustering, flake tracking, the triage command, TESTING.md. Every package ships scenarios for what it builds in its own `scenarios/<area>/`.

## 4b. The spotless bar as configuration

WP0 checks these files in; every later package leaves them green and never weakens them without a decision record.

### 4b.1 Toolchain, formatting, profiles, line endings

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.99.0"
components = ["rustfmt", "clippy"]
targets = ["x86_64-pc-windows-msvc"]
profile = "minimal"
```

`cargo xtask setup` installs it once (pinning `1.99.0` while `stable` is also 1.99.0 makes rustup install a second copy the first time). Fuzzing uses a nightly pinned by date in `fuzz/rust-toolchain.toml`.

`rustfmt.toml` (stable options only):

```toml
edition = "2024"
style_edition = "2024"
max_width = 100
use_small_heuristics = "Max"
newline_style = "Unix"
use_field_init_shorthand = true
use_try_shorthand = true
reorder_imports = true
reorder_modules = true
```

Profiles in the root `Cargo.toml`:

```toml
[profile.dev]
debug = "line-tables-only"          # useful backtraces, small PDBs

[profile.dev.package."*"]
opt-level = 2                       # audio, ONNX glue and SQLite are unusable at opt-level 0
debug = false                       # no debug info for dependencies: less link memory on a 16 GB PC

[profile.release]                   # local checks and CI smoke builds
opt-level = 3
lto = "thin"
panic = "unwind"                    # the core survives a panicking task
debug = "line-tables-only"
incremental = false

[profile.dist]                      # tagged releases only, built in CI, never on the owner's PC
inherits = "release"
lto = "fat"
codegen-units = 1
strip = "none"                      # MSVC keeps symbols in the .pdb, which ships to the symbol store
```

Once the speech crates exist, WP4 narrows `opt-level = 2` to the crates that need it if `cargo build --timings` shows proc macros built at opt-level 2 dominating.

`.cargo/config.toml`:

```toml
[alias]
xtask = "run --package xtask --"

[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

Build parallelism (`CARGO_BUILD_JOBS`) is not set in the repository, so contributors with bigger machines and CI are not throttled; the build guard sets it on the owner's PC (section 6.1).

`.gitattributes`:

```gitattributes
* text=auto eol=lf
*.rs text eol=lf
*.toml text eol=lf
*.md text eol=lf
*.json text eol=lf
*.ndjson text eol=lf
*.snap text eol=lf
*.lua text eol=lf
*.yml text eol=lf
*.ps1 text eol=crlf
*.cmd text eol=crlf
*.bat text eol=crlf
*.wav binary
*.onnx binary
*.png binary
*.jpg binary
*.zst binary
*.drx binary
*.drp binary
*.pdb binary
```

### 4b.2 Lints, clippy configuration and unsafe policy

Workspace lints (the rust-architecture report's table, checked against the stable lint index on 2026-10-20; every member has `[lints] workspace = true`; local builds warn, CI runs `-D warnings`):

```toml
[workspace.lints.rust]
unsafe_code = "deny"
unsafe_op_in_unsafe_fn = "deny"
missing_unsafe_on_extern = "deny"
non_ascii_idents = "forbid"
missing_docs = "warn"
missing_debug_implementations = "warn"
unreachable_pub = "warn"
unused_qualifications = "warn"
unused_lifetimes = "warn"
redundant_lifetimes = "warn"
single_use_lifetimes = "warn"
trivial_casts = "warn"
trivial_numeric_casts = "warn"
let_underscore_drop = "warn"
elided_lifetimes_in_paths = "warn"
explicit_outlives_requirements = "warn"
meta_variable_misuse = "warn"
unit_bindings = "warn"
unused_macro_rules = "warn"
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(loom)"] }

[workspace.lints.rustdoc]
broken_intra_doc_links = "deny"
private_intra_doc_links = "deny"
bare_urls = "warn"
unescaped_backticks = "warn"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
cargo = { level = "warn", priority = -1 }
missing_errors_doc = "allow"        # every fallible fn returns a typed error enum that documents itself
missing_panics_doc = "allow"        # panics are denied below
multiple_crate_versions = "allow"   # windows-sys has two majors in the tree; cargo-deny reports duplicates
cargo_common_metadata = "allow"     # publish = false
use_self = "warn"
or_fun_call = "warn"
redundant_clone = "warn"
needless_collect = "warn"
significant_drop_tightening = "warn"
significant_drop_in_scrutinee = "warn"
option_if_let_else = "allow"
missing_const_for_fn = "warn"
equatable_if_let = "warn"
debug_assert_with_mut_call = "warn"
useless_let_if_seq = "warn"
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "deny"
unimplemented = "deny"
unreachable = "warn"
dbg_macro = "deny"
print_stdout = "deny"
print_stderr = "deny"
exit = "deny"
mem_forget = "deny"
undocumented_unsafe_blocks = "deny"
multiple_unsafe_ops_per_block = "deny"
unnecessary_safety_comment = "deny"
unnecessary_safety_doc = "deny"
allow_attributes = "deny"           # use #[expect(...)] so a stale suppression fails the build
allow_attributes_without_reason = "deny"
get_unwrap = "deny"
indexing_slicing = "warn"
string_slice = "deny"
clone_on_ref_ptr = "warn"
rc_buffer = "warn"
rc_mutex = "deny"
str_to_string = "warn"
lossy_float_literal = "deny"
float_cmp_const = "deny"
empty_structs_with_brackets = "warn"
rest_pat_in_fully_bound_structs = "warn"
tests_outside_test_module = "warn"
unused_result_ok = "warn"
let_underscore_must_use = "deny"
let_underscore_untyped = "warn"
verbose_file_reads = "warn"
mod_module_files = "deny"           # foo.rs plus foo/, never foo/mod.rs
infinite_loop = "warn"
large_include_file = "deny"
fn_to_numeric_cast_any = "deny"
if_then_some_else_none = "warn"
renamed_function_params = "warn"
semicolon_outside_block = "warn"
unseparated_literal_suffix = "warn"
wildcard_enum_match_arm = "warn"
missing_assert_message = "warn"
create_dir = "warn"
filetype_is_file = "warn"
cfg_not_test = "deny"
```

`clippy.toml`:

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-panic-in-tests = true
allow-indexing-slicing-in-tests = true
allow-dbg-in-tests = true
allow-print-in-tests = true
avoid-breaking-exported-api = false
too-many-lines-threshold = 80
too-many-arguments-threshold = 6
cognitive-complexity-threshold = 20

disallowed-methods = [
  { path = "std::thread::sleep", reason = "blocks a runtime worker; use tokio::time::sleep or a named thread with a reason" },
  { path = "std::env::set_var", reason = "process-global and unsound with threads" },
  { path = "std::env::remove_var", reason = "process-global and unsound with threads" },
  { path = "std::process::Command::new", reason = "start children through eileen_supervisor (Job Object, kill on close)" },
  { path = "tokio::process::Command::new", reason = "start children through eileen_supervisor", allow-invalid = true },
  { path = "std::process::exit", reason = "skips destructors and log flushing; return from main" },
  { path = "std::time::Instant::now", reason = "core logic gets time from the clock port; drivers use eileen_stdx::clock" },
  { path = "std::time::SystemTime::now", reason = "core logic gets time from the clock port; drivers use eileen_stdx::clock" },
  { path = "ulid::Ulid::generate", reason = "IDs come from the Ids port so replay is deterministic", allow-invalid = true },
  { path = "tokio::sync::mpsc::unbounded_channel", reason = "every queue has a maximum", allow-invalid = true },
  { path = "tokio::spawn", reason = "use eileen_runtime::task::spawn: it names the task, keeps the span and registers it", allow-invalid = true },
  { path = "tokio::task::spawn", reason = "use eileen_runtime::task::spawn", allow-invalid = true },
]
disallowed-types = [
  { path = "std::sync::mpsc::Sender", reason = "use bounded tokio::sync channels" },
]
disallowed-names = ["foo", "bar", "baz", "tmp", "data2"]
```

The one real clock, ID generator, spawn wrapper and supervisor spawn each carry an `#[expect(clippy::disallowed_methods, reason = "...")]`. `crates/eileen-core/clippy.toml` is the root file plus a `disallowed-types` entry for `std::collections::HashMap` (use `BTreeMap` or `IndexMap` where order can reach an effect); clippy reads the nearest file and does not merge, so a tidy check keeps the two in sync. Lookup-only maps are fine elsewhere.

**Unsafe policy.** The workspace denies `unsafe_code`. Every crate without FFI adds `#![forbid(unsafe_code)]`. An FFI crate keeps every unsafe call in one private module declared as `#[expect(unsafe_code, reason = "...")] mod ffi;`, one unsafe operation per block, each with a `// SAFETY:` comment saying which precondition holds and why, exposing only safe functions and converting every `HRESULT` and `BOOL` into the crate's error type there. COM calls run on the crate's own apartment thread with a deadline, never on a tokio worker. Prefer crates that own their unsafe code (process-wrap, tray-icon, cpal). Write "safety comment" in prose docs (the word with a colon trips `unnecessary_safety_comment`) and never link a private `ffi` module from public docs.

### 4b.3 cargo-deny, nextest and CI

`deny.toml` is the rust-architecture report's file with two changes: `IJG` added to the license allow list for `jpeg-encoder` (with the attribution in `NOTICE`), and the graph targets extended to `x86_64-unknown-linux-gnu` so the Linux lane is checked too. Licenses allowed: MIT, MIT-0, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, 0BSD, Unicode-3.0, Unlicense, CC0-1.0, BSL-1.0, IJG. `openssl-sys` and `native-tls` are banned; unknown registries and git sources are denied; yanked versions are denied. cargo-deny cannot see downloaded native libraries, so the release lane lists their licenses separately.

`.config/nextest.toml`:

```toml
[profile.default]
slow-timeout = { period = "10s", terminate-after = 3 }
leak-timeout = { period = "500ms", result = "fail" }   # a test that leaves a child process running fails

[profile.ci]
fail-fast = false
retries = 0
failure-output = "immediate-final"
slow-timeout = { period = "15s", terminate-after = 4 }

[profile.ci.junit]
path = "junit.xml"

[profile.nightly]
retries = 2
flaky-result = "fail"
fail-fast = false

[test-groups]
desktop = { max-threads = 1 }

[[profile.default.overrides]]
filter = "test(/^desktop::/) | package(eileen-desktop-win) | package(eileen-apps-win) | package(eileen-shell-win) | package(eileen-e2e)"
test-group = "desktop"
```

Workflows (all with `permissions: contents: read` at the top, `persist-credentials: false` on checkout, actions pinned by SHA, zizmor on every change, Dependabot for action SHAs, `concurrency` cancelling superseded runs, `CARGO_INCREMENTAL=0`, the Windows target directory on the runner's `D:` drive if it measures faster, rust-cache saved only from `main`):

| Workflow | Trigger | Jobs |
|---|---|---|
| `ci.yml` | pull request, push to `main` | `plan` (changed paths decide optional steps), `hygiene` (ubuntu), `logic-linux` (ubuntu), `windows-clippy`, `windows-test`, `pr-meta`, `claude-plugin` (only if `integrations/claude-plugin/` exists; non-blocking until validation without sign-in is proven), `mutants-diff` (advisory), and `all-green`, the single required check, which runs with `if: always()` and fails unless every needed job succeeded or was skipped |
| `nightly.yml` | daily, manual | the nightly lane of section 4.6, then `report` (one issue per failing job) |
| `weekly.yml` | weekly | the weekly lane |
| `release.yml` | tag `v*`, manual | the release lane |

The engineering-practice report's `ci.yml` skeleton (section 6.2 there) is the starting point.

**One command.** `cargo xtask ci` runs locally exactly what the pull request lane runs (through the build guard), so green locally means green in CI. `cargo xtask lint-repo` runs only the fast repository checks below, in seconds. `cargo xtask pre-push` runs fmt, clippy, the changed crates' tests and `lint-repo`; `.githooks/pre-push` calls it (`cargo xtask setup` sets `core.hooksPath`).

**The conventions, each enforced by a check** (`cargo xtask lint-repo`, a std-only test suite in `xtask` in the rust-analyzer style; each failure prints the file, the line and the rule's name). This list is copied into `CONTRIBUTING.md` and the root `AGENTS.md`:

| # | Convention | Check |
|---|---|---|
| 1 | Every crate's guidance is in its crate doc with the required headings (section 4b.4), within 80 lines | parses `src/lib.rs` or `src/main.rs` doc comments |
| 2 | Nested `AGENTS.md` only where a folder's rules differ, each with a sibling `CLAUDE.md` whose first line is `@AGENTS.md` and a "Why this folder has its own rules" heading; root `AGENTS.md` at most 150 lines; root `CLAUDE.md` starts with `@AGENTS.md` | file scan |
| 3 | `ARCHITECTURE.md`'s code map lists exactly the workspace crates, both ways | `cargo metadata` against the code map table |
| 4 | Every `cargo xtask <command>` named in any guidance file exists | parses guidance files and xtask's command list |
| 5 | Markdown is linted, internal links resolve (external links nightly), spelling passes | rumdl, lychee offline, typos |
| 6 | House style: no em dash (U+2014) in Markdown, doc comments or user-facing strings; the product is only ever "Eileen"; no old working names; workers named only by route; no vendor product name used as part of an Eileen name, a feature name or a worker name; no film or trademark names from the denylist | `xtask/tidy/denylist.toml` with an exemption list (`xtask/tidy/exempt.toml`) naming dated historical records with a reason each |
| 7 | Commit messages: imperative subject under 72 characters, capitalised, no trailing period, a blank line, a body that says why for non-trivial changes; trailers allowed | `lint-repo --commits <range>`; CI checks the pull request's commits |
| 8 | Pull request description follows the template and every required section is filled (What, Why, Tests at which layer, Seen failing before the fix (for fixes), Scenario, Suggested guidance changes, Architectural changes) | `pr-meta` reads the body from the event payload through an environment variable, never a template expression |
| 9 | Generated files are current: `docs/CODEMAP.md`, `docs/SCENARIOS.md`, `docs/reference/*` (telemetry names, events, MCP tools, config keys), the config schema and commented default, contract schemas, the scenario schema | `cargo xtask codegen --check` |
| 10 | Every unsafe block has a safety comment, and unsafe code appears only in `ffi` modules | clippy (`undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`) plus a tidy scan |
| 11 | Public items are documented | `missing_docs` under `-D warnings`, `cargo doc` with `RUSTDOCFLAGS=-D warnings` |
| 12 | Decision records follow the template: `## D<n>. <title>`, then Date, Status (accepted, proposed, or superseded by D<m>), optional Supersedes and Amends, Decision, Why, Consequences, Source; numbers sequential; every referenced record exists | parses `docs/DECISIONS.md` |
| 13 | Dependencies pass the license, advisory, ban and source policy; no unused dependencies; new crate versions respect the cooling-off period on update pull requests | cargo-deny, cargo-machete, `xtask deps-age` |
| 14 | The credential boundary: no source names a vendor credential path or the persistent MCP registration command; no process spawn outside the supervisor | tidy scan plus clippy `disallowed-methods` |
| 15 | Platform boundary: neutral crates do not depend on `-win` crates or on `windows`/`windows-sys` outside a `cfg(windows)` target table | `cargo metadata` |
| 16 | No `#[ignore]`, no `#[should_panic]`, no real-time sleeps in tests; modules over 800 lines listed in `xtask/tidy/allow.toml` with a reason | scan |
| 17 | Fuzz targets each have a corpus and a replay entry; simulator regression files parse | scan |
| 18 | Scenarios are valid and private (section 4.8); fixtures carry no `origin = "user"`, no unmarked audio, no personal paths | `cargo xtask scenarios check` |
| 19 | A pull request labelled `bug` adds or changes a regression (scenario, fuzz regression, simulator regression or test) | `pr-meta` |
| 20 | Workflows pass zizmor; actions are pinned by SHA | zizmor, scan |
| 21 | Every span and event name used in code is declared in `eileen_trace::names`, and the span-parent table covers it | test in `eileen-trace` plus a scan |
| 22 | Each crate's `Cargo.toml` has `[package.metadata.eileen]` with `process`, `owns` and `unsafe` | `cargo metadata` |
| 23 | Docs drift (advisory): a pull request that changes a crate's public API without touching its crate doc gets a warning annotation; on the owner's PC, `cargo xtask docs-drift --draft` asks a Claude worker to draft the update | compares `pub` item lists, parsed with `syn`, between base and head |

### 4b.4 Guidance for people and coding agents

Decision D33 records the form, chosen from the engineering-practice report's survey of the best-run repositories (rust-analyzer, ruff, uv, Zed, codex, Turso, TigerBeetle) and the owner's instruction not to assume a guidance file in every folder:

- **Root `AGENTS.md` is the canonical guide** for every coding tool (Codex reads only `AGENTS.md`; the claude CLI reads it directly and through imports). It holds the commands, the rules that do not bend, the PC rules, how to make a change, testing rules, code review rules, the convention checks of 4b.3, and pointers to `ARCHITECTURE.md`, `docs/BUILD.md`, `docs/OWNER-DIRECTION.md` and `docs/wp/`. The engineering-practice report's section 4.2 draft is the starting text, with "read the crate doc first" in place of "read the crate's AGENTS.md". At most 150 lines.
- **Root `CLAUDE.md`** is `@AGENTS.md` plus a short Claude-only section (project skills, the settings denials, the fresh-reviewer rule, the LSP note). Imports, not symlinks, because committed symlinks check out as one-line text files on Windows.
- **Every crate's guidance is its crate doc**, the `//!` comment at the top of `src/lib.rs` or `src/main.rs`, which rustdoc renders and every agent reads when it opens the crate. Required headings: `# Owns` (at most three bullets), `# Does not own`, `# Invariants` (each with how it is enforced), `# Boundaries` (called by, may depend on, must not depend on, wire types), `# Testing` (the fast command, the layers, what a bug here starts as), `# Traps`, `# Logs and tracing` (targets, spans it owns, `pii.` fields, the replay or probe command). At most 80 lines.
- **Nested `AGENTS.md` only where a folder's rules genuinely differ**, each paired with a `CLAUDE.md` that is `@AGENTS.md`: `crates/eileen-desktop-win/` and `crates/eileen-apps-win/` (COM apartments, deadlines, the desktop test group, owned fixtures only), `crates/eileen-speech/` (native libraries, model files, the exclusive build slot for model tests), `fuzz/` (nightly toolchain, Linux only, regression rule), `scenarios/` (the public corpus: privacy rules, no personal data, schema), `integrations/claude-plugin/` (TypeScript, the never-gating rule, plugin validation). Adding another needs a line in the pull request saying why the folder's rules differ.
- **`ARCHITECTURE.md` at the repository root**, short and in rust-analyzer's shape: bird's eye view, entry points, a code map with one entry per crate (named files and types, not links) marking architecture invariants and API boundaries, then cross-cutting concerns. WP0 moves today's `docs/ARCHITECTURE.md` there and adds the code map check.
- **Claude-only extras** in `.claude/`: `settings.json` permission rules that deny release builds, `cargo install`, `cargo clean` and reads of vendor credential files (verify the rule syntax against the settings reference); path-scoped `rules/ffi.md` for `ffi` modules; skills `sim-debug`, `add-fuzz-target`, `e2e-scenario`, `work-package` and `save-scenario`.
- **Work package files** `docs/wp/WP<n>-<topic>.md` in the living-plan shape (goal, done when, progress, surprises, decisions, outcome), updated by the agent doing the work.
- **Rules hygiene** (Zed): do not edit guidance files during feature work; propose changes under "Suggested guidance changes" in the pull request; a rule is added only if it is non-obvious, came up more than once and is specific.

### 4b.5 `cargo xtask`

A std-only binary (it compiles in seconds), subcommands: `setup`, `check`, `test`, `clippy`, `build`, `ci`, `pre-push`, `lint-repo`, `codegen` (with `--check`), `sim` (PR seeds; `--seed`, `--profile`, `--replay`, `--minimize`, `--doublecheck`), `fuzz-replay`, `fuzz` (Linux), `scenarios check|replay|catalogue`, `e2e`, `bench`, `soak`, `nightly-local`, `fetch-native`, `lock`, `deps-age`, `symbolicate`, `triage`, `docs-drift`. Every subcommand that runs cargo goes through the build guard (section 6.1). `tools/codemap` is a separate package (it parses with `syn`) so `xtask` itself stays std-only.

## 5. Why this is not already done

The prior-art report (2026-10-11) is blunt, and this spec keeps its honesty: the halves of Eileen exist, and the README's old "nobody has built this" overstated it.

- **ChatGPT desktop** is the closed, single-vendor version on Windows. Its GPT-Live voice directs several Codex and ChatGPT Work tasks at once while you talk, and Computer Use runs on Windows. But it is OpenAI only, voice is a session you start and stop (one at a time), its screen context ("appshots") is macOS only, and on Windows its computer use takes over the foreground; OpenAI suggests a VM or a second device.
- **Paseo** (about 20,000 stars, Apache-2.0, a Windows installer) runs Claude, Codex, Copilot, OpenCode and others in parallel worktrees, with local Parakeet, Kokoro and Silero voice whose brain is a hidden session on the user's installed harness. It proves two of Eileen's choices (local CPU speech is good enough; the brain can be a warm subscription session). It does not see or drive the desktop.
- **Hermes Agent** (about 250,000 stars, MIT) ships a Windows app with a wake word, continuous voice with barge-in, background UI Automation through cua-driver, and a skill curator that learns. But it is its own harness rather than a router to vendor harnesses, it runs vendor OAuth itself and imports the Codex CLI's credential file, its wake flow takes one utterance at a time, and its skills have no deterministic replay with step checks, no per-line provenance and no permission story.
- Also: qwen-audio-agent (full-duplex voice over ACP backends, cloud realtime APIs by default), Jarhead (the closest single design, macOS only, voice billed per minute), Microsoft's local Copilot actions (Copilot+ PCs only), the Claude desktop app (Claude only; Windows background computer use undocumented).
- **Learning from an agent's own work is well studied and partly shipped** (Voyager, Agent Workflow Memory, SkillWeaver, ASI, UFO2, Hermes, Letta). Eileen does not claim the idea. What nobody ships is the whole loop on a Windows desktop for one person: capture from every job, native skills the vendor's own harness loads, a promotion ladder that ends in model-free replay with a check after every step, handoff the moment a check fails, local history with rollback, and voice and HUD control.
- **Dictation**: Handy (MIT, Rust) is the open-source leader on Windows and Wispr Flow the paid bar; neither knows the focused field, its app or what is on screen.

So Eileen's novelty is the integration on Windows, done to a higher standard than any half. That is a composition moat, not a technology moat: Paseo could add desktop control in weeks. Eileen has to win on things that can be measured and published:

1. Always listening with no wake word, local, at near-zero idle cost (VAD measured at 5.8 percent of one core), with addressee detection measured by a scripted room-talk test.
2. DesktopState as cheap context, so "this", "that window" and "screen two" resolve without a screenshot.
3. Background-first, deterministic-first control with honest foreground escalation, and safety rules in the server (password fields, blocked combos, explicit targets) that a learned skill cannot bypass.
4. Real pro-app adapters for the owner's examples (Resolve through its own scripting interpreter, SMTC, Office COM), which no competitor demonstrates.
5. Route-named workers on unmodified vendor CLIs with an auditable credential boundary.
6. A learning loop the user can see, steer and erase, with model-free replay that never skips a confirmation.
7. A test suite that grows from real use on many friends' PCs, with published benchmarks and A/B timing.
8. Rust where it buys something measurable: start-up in tens of milliseconds, a small idle footprint, hook and UIA latency, single binaries. Model latency is the vendor's, and the docs say so.

## 6. Work packages

### 6.1 Building on the owner's PC without thrashing it

The rust-architecture report measured why this needs rules: one clean dev build of the small skeleton under contention took 21 minutes and drove free RAM to 462 MB; a fat-LTO build beside other agents took free RAM to 0 MB and ended the session; one `rustc` was seen at 417 MB; four tools built from source took 59 minutes. The rules, enforced by `cargo xtask` and repeated in every brief:

1. **At most two engineer agents compile at once, `CARGO_BUILD_JOBS=6` each.** Only one compiles while a release build, a speech-model process (model tests, voice benchmarks) or a soak runs. `cargo xtask` takes one of two slot locks (`%LOCALAPPDATA%\eileen\build-slot-0.lock`, `-1.lock`, held with `File::try_lock` so the OS releases them if the process dies); `--exclusive` work takes both. It waits when the slots are taken, refuses to start with less than 2 GB free RAM, sets `CARGO_BUILD_JOBS` to 6 if unset (8 when it holds both slots), and lowers its own priority class to below normal so compilers inherit it and the Claude app and the voice loop stay responsive. Agents build only through `cargo xtask`.
2. **sccache, shared by all worktrees**, if the owner has installed it (prebuilt 0.18.0): `RUSTC_WRAPPER=sccache`, `SCCACHE_DIR=%LOCALAPPDATA%\sccache`, `SCCACHE_CACHE_SIZE=8G` with at least 30 GB free disk, else `4G`. If it is not installed, build without it; never install it from source.
3. **A target directory per worktree** (Cargo's default), never a shared `CARGO_TARGET_DIR` (Cargo locks a target directory for the whole build). Delete a worktree's target directory when its branch merges. At most three live worktrees with build output while `C:` has less than 25 GB free (19.4 GB were free on 2026-10-20).
4. **No debug info for dependencies** in dev, line tables for our crates (section 4b.1).
5. **Inner loop on `cargo xtask check -p <crate>` and `cargo xtask clippy -p <crate>`**; build only the binary you need.
6. **No local `dist` (fat LTO) builds.** Local release builds use the thin `release` profile with the exclusive lock.
7. **Never `cargo install` from source.** `cargo xtask setup` fetches prebuilt tools.
8. **Do not start a long build, simulation, soak or benchmark at the end of a session**, kill every process you start before you finish, and never benchmark during a build.
9. **Desktop packages never run at the same time.** A package marked `needs_desktop` drives real windows, audio devices or apps on the owner's PC; no two are scheduled in the same wave, and their desktop tests additionally take `%LOCALAPPDATA%\eileen\desktop.lock` through `cargo xtask` and only touch windows the test created. Tests that need real Claude count against the owner's plan: keep them few, mark them `requires = ["real-claude"]`, and run them by hand or in the owner's nightly lane.

| Wave | Packages | Compiling at once | Desktop | Ends with |
|---|---|---|---|---|
| W0 | WP0 | one (may use 8 jobs) | no | Workspace, gates, contracts, stubs, CI green on both OSes |
| W1 | WP1, WP3 | two | no | Core in simulation; Claude brain and workers from Rust |
| W2 | WP2, WP4 | two, one during speech-model tests | WP4 | **Slice S1** (WP2 wires it once WP4 merges) |
| W3 | WP5, WP7 | two | WP5 | Computer use; intent, settings, learning |
| W4 | WP6, and WP7 if it is still running | two | WP6 | App adapters, Resolve benchmark parts |
| W5 | WP8 | one | WP8 | HUD, tray, hotkeys, Claude plugin |
| W6 | WP9 | one | WP9 | Benchmarks, fleet, M6 demo |

Every package: branch named in its section, draft pull request from the first push (CI runs on drafts), small pull requests (logic changes under about 500 lines), merge only with `all-green`, its `docs/wp/` file kept current, its crate docs and the code map current, at least one scenario for what it builds, a fuzz target for every new parser of untrusted input, a simulator invariant or scenario for every new core behaviour. CI checks every package must pass: everything `cargo xtask ci` runs (fmt, clippy `-D warnings` on Windows and Linux, nextest on both, doc tests, docs with `-D warnings`, cargo-deny, cargo-machete, `lint-repo`, `codegen --check`, simulator PR seeds and regressions, fuzz corpus replay, scenario validation and replay), plus `pr-meta` and `all-green` on GitHub.

### 6.2 WP0: Foundation (workspace, contracts and quality gates)

- **Branch** `wp0/foundation` (land it as several pull requests: `wp0/workspace`, `wp0/contracts`, `wp0/trace`, `wp0/xtask`, `wp0/ci`, `wp0/guidance`, `wp0/stubs`). **Depends on** nothing. **Desktop** no. **Wave** W0.
- **Owns** the root files and directories of section 3.1 (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`, `_typos.toml`, the Markdown and link-check configs, `.gitattributes`, `.gitignore`, `.cargo/`, `.config/`, `.githooks/`, `.github/`, `.claude/`), `AGENTS.md`, `CLAUDE.md`, `ARCHITECTURE.md` (moved from `docs/`), `CONTRIBUTING.md`, `TESTING.md` (skeleton), `SECURITY.md`, `NOTICE`, `README.md`, `docs/DEV-SETUP.md`, `docs/NAMING.md`, `docs/wp/WP0-foundation.md`, `xtask/`, `tools/codemap/`, `tools/claude-plugins/eileen-dev/`, `fuzz/Cargo.toml`, `fuzz/README.md`, `fuzz/rust-toolchain.toml`, the fuzz targets `uia_text` and `desktop_state`, `scenarios/README.md`, `scenarios/_schema/`, `scenarios/core/`, and the crates `eileen-contracts`, `eileen-stdx`, `eileen-trace`, `eileen-platform`, `eileen-supervisor`. It deletes `src/`, `tests/*.py`, `pyproject.toml` and the Python CI job after tagging `python-scaffold-final`. It also creates every other crate of section 3.1 as a stub and every other fuzz target as a stub calling its stub parser; ownership of those passes to their packages when WP0 merges.
- **Builds.**
  1. The workspace of section 3.1 with the pins of section 4.1 and the config files of section 4b, profiles included.
  2. `eileen-contracts`: every type of section 3.3 with serde and schemars, snapshot tests of every wire shape and generated tool schema (sorted maps; snapshots must not depend on feature-unified `preserve_order`), `desktop::sanitize`, the ports traits (`IntentResolver`, `DictationCleaner`, `SettingsValidator`, `Component`, `InputSink`, `EffectSource`) and the scenario types.
  3. `eileen-stdx`: the clock and ID ports with their one real implementation each, a seeded `CoreRng` (a small in-house generator, no `rand`), `never!`, `always!` and `buggify!` (activated per site per run, enabled only when the simulator installs its context), bounded-channel helpers.
  4. `eileen-trace`: `init(ProcessKind, &TraceConfig) -> TraceGuard` with the JSON file, console, ring and slow-span layers of section 2.13; `names` seeded with the spans of the observability report section 2.5; the span-parent `schema` and its checker; the `pii.` policy; the `Redactor` (API-key and token patterns, JWTs, private-key blocks, Luhn-valid card numbers, password-field values, with a literal prefilter); the Chrome trace converter; `traceparent` formatting and parsing; `test_support` (an in-memory capture layer with `assert_schema_ok`).
  5. `eileen-platform`: the traits of section 3.4 with fakes.
  6. `eileen-supervisor`: port `supervisor.rs` from the ws-next scratch workspace (process-wrap with a Job Object, `CREATE_NO_WINDOW`, kill-on-drop, stderr forwarded into tracing, deadlines, heartbeat helpers, restart with bounded backoff), the one `#[expect]`ed `Command::new`, plus `qos::opt_out_of_ecoqos()` and `qos::boost_audio_thread()` in its `ffi` module, and a spawn variant outside the Job Object for the guard.
  7. `eileen-sim` skeleton: a `World`, profiles in TOML with a generated schema, seeded runs, the step log, `--doublecheck`, the regression folder, and three invariants (6, 10, 13) against the stub core.
  8. `xtask` with every subcommand of section 4b.5 (those whose target is not built yet run and report zero items), the build guard of section 6.1, and `lint-repo` with all 23 checks of section 4b.3, each with a failing fixture test; `tools/codemap` writing `docs/CODEMAP.md` (crate graph by process from `[package.metadata.eileen]` and `[workspace.metadata.eileen.processes]`, module trees, the event map of which functions construct and match each `BusEvent` and `Input` variant).
  9. CI: `ci.yml`, `nightly.yml`, `weekly.yml`, `release.yml` per section 4b.3, with the lanes of section 4.6 present from day one (jobs whose inputs do not exist yet skip cleanly), plus Dependabot for action SHAs.
  10. Guidance per section 4b.4: root `AGENTS.md` and `CLAUDE.md`, the crate-doc template filled for every crate from section 3.1 and this spec, the nested `AGENTS.md` and `CLAUDE.md` pairs listed there, `.claude/` settings, rules and the five skills, `docs/wp/` files for all ten packages, `ARCHITECTURE.md` moved to the root with its code map, `CONTRIBUTING.md` (the check list of section 4b.3, branch and commit rules, review rules), `docs/DEV-SETUP.md` rewritten for Rust (owner installs sccache and cargo-binstall prebuilt; `cargo xtask setup`), the pull request template (What, Why, Tests at which layer, Seen failing before the fix, Scenario, Suggested guidance changes, Architectural changes), issue forms (bug, task, scenario, fleet results), `TESTING.md` skeleton, `NOTICE` (third-party attributions: IJG, Handy, SpeakoFlow, Pocket TTS, models).
  11. The scenario schema, `cargo xtask scenarios check` with the privacy rules, the catalogue generator writing `docs/SCENARIOS.md`, and three example scenarios in `scenarios/core/` (validation only until WP1 adds replays).
  12. House-style cleanup the new checks require: `docs/NAMING.md` rewritten without the old working names and the franchise; dated historical records (`docs/DECISIONS.md` D15 and D16, `docs/RESEARCH.md` section 8, `docs/research/`) listed in `xtask/tidy/exempt.toml` with a reason unless the owner decides to scrub them (section 7).
- **Acceptance.**
  1. `cargo xtask ci` passes on the owner's PC under the build guard, and `all-green` passes on GitHub on `windows-latest` and `ubuntu-latest`.
  2. Every `lint-repo` check has a fixture test that fails on a seeded violation (an em dash in a fixture Markdown file, a crate doc missing `# Invariants`, a code map missing a crate, a bad commit subject, an unpinned action, a scenario with `origin = "user"`, an unsafe block outside `ffi`, an undeclared span name).
  3. Contract snapshots exist for every type in section 3.3 and every tool argument schema; `cargo insta test --unreferenced reject` passes.
  4. `cargo xtask sim` runs the PR seed set in under 90 s on the Linux runner; `--doublecheck` passes.
  5. A supervisor test on Windows: killing the Job Object kills a spawned grandchild; the guard-style spawn survives its parent.
  6. The redaction canary: fake secrets written through the file and ring layers never appear raw in the bytes on disk or in a ring dump.
  7. `eileen --version` and every stub binary build; every subcommand of section 3.5 exists and exits with a clear "not built yet" message where its package has not landed.
  8. The first CI run's Windows job times (cold and warm cache) and the sccache hit rate after a second worktree's build are recorded in `docs/wp/WP0-foundation.md`.

### 6.3 WP1: Core logic and simulation

- **Branch** `wp1/core`. **Depends on** WP0. **Desktop** no. **Wave** W1.
- **Owns** `crates/eileen-core`, `crates/eileen-sim`, `crates/eileen-record`, `scenarios/feel/`, `scenarios/jobs/`, fuzz targets `recording` and `sim`, `docs/wp/WP1-core.md`.
- **Builds.** `Core::step` (section 3.3.3) with modules for turn-taking and barge-in (states idle, hearing, thinking, speaking, dictating), the addressee gate, intent dispatch through the `IntentResolver` port (fakes in tests; the real resolver arrives with WP7), the brain turn orchestration (context blocks with their caps, the `---hud---` split, clause queueing, interrupt on new speech, recycle and spare swap policy), depth and the two-speed deep answer, jobs (registry state, names, slugs, pronoun resolution, focus job, fan-out planning, worktree specs, reroute, steer, cancel), the router (TOML policy as `CoreConfig`, spoken overrides, recorded reasons), the permission gate (`claude_tool_tier` and per-tool tiers, approvals only from `UserInput`, grants, the hook guard decisions), announcements (the policy table, focus awareness, narration rate limit), the input lock, the action journal and undo, the settings flow state machine (propose, check, confirm, apply, version, undo, profiles) over the `SettingsValidator` port, plan-usage policy, supervisor policy (restart backoff, hang handling decisions), incident triggers, the regret window and learning effects, HUD state assembly, and scenario-save requests. `eileen-record`: the recording format (section 3.3 types, versioned), the BLAKE3 and zstd blob store, the reader and writer, `trim`. `eileen-sim`: the full world of section 4.7 (speech, playback, brain, workers from stub scripts, sidecars with latency, hangs and crashes, Job Object semantics, an in-memory store model), all 13 invariants, profiles (`default`, `barge-in-heavy`, `many-jobs`, `flaky-sidecars`), exhaustive turn-taking to depth 6 (8 nightly), the L1 replayer with the first-divergence report, expectation checking for sim-lane scenarios (`cargo xtask scenarios replay`), and `eileen replay <id> --level core`.
- **Acceptance.**
  1. All 13 invariants hold on the PR seed set and on 30 minutes of random seeds per profile (nightly); each invariant has a hand-written scenario that breaks a fake to prove the invariant catches it.
  2. `turn::exhaustive` passes at depth 6 within the PR budget.
  3. `tests/it/job_model.rs`: a proptest-state-machine reference model of the job lifecycle agrees with the core on random command sequences.
  4. Barge-in in simulation: onset to `StopPlayback` at most 96 ms of virtual time plus one callback, and the heard prefix equals the text of the samples played.
  5. Approval safety: a scripted scenario where a tool result and a worker's text both contain "yes, approve" does not approve; only `UserInput` or in-window microphone speech does.
  6. Two-speed: a deep answer arriving while the user talks goes to the HUD only; arriving in silence is spoken once.
  7. A recorded run replays at L1 with no divergence; a deliberate change to the router makes the replay report the first divergent event with its sequence number.
  8. `scenarios/feel/` (half-finished sentence with a pause, room talk ignored, interrupt and resume, "no, the other one" within the visible-intent window) and `scenarios/jobs/` (three parallel jobs, reroute, cancel, approval by voice, deny by silence) replay green in simulation.
  9. `eileen-core` has no dependency outside `eileen-contracts`, `eileen-stdx` and `eileen-trace` (tidy).

### 6.4 WP2: Runtime, binaries and supervision

- **Branch** `wp2/runtime`. **Depends on** WP0 and WP1; integrates WP3 and WP4 for slice S1. **Desktop** no. **Wave** W2.
- **Owns** `crates/eileen-runtime`, `crates/eileen-mcp`, `crates/eileen-cli`, `crates/eileen-shim`, `crates/eileen-crash`, `crates/eileen-guard`, `docs/DEBUGGING.md`, `scenarios/runtime/`, fuzz targets `mcp_client_message`, `mcp_tool_arguments`, `dispatch_line`, `worker_hook_payload`, `http_request`, `docs/wp/WP2-runtime.md`.
- **Builds.** The driver loop (one task owns `Core`; inputs through a bounded channel; effects routed to components; `task::spawn` with names, span propagation and `await-tree` registration); the SQLite store (jobs, events with `seq` and `TraceCtx`, approvals, settings versions, journal, plan usage; migrations; WAL mode); the bus fan-out with explicit `Lagged`; the loopback HTTP/1.1 server (strict limits, bearer token, `/v1/hooks/claude/<event>`, `/v1/state`, `/v1/control/mute`, `/v1/say`, `/v1/handoff`, `bus.json`); the dispatcher pipe server and `eileen-shim` (section 3.3.9 protocol, per-session tokens, toolsets per role, `_meta.traceparent` passthrough, logging the `_meta` the CLI sends on the first call); sidecar MCP clients through rmcp over supervised stdio with deadlines, heartbeats, restart with backoff, and hang-dump-then-kill through the guard; the connector registry from manifests; the flight recorder wiring (input tap, checkpoints, incident assembly, the janitor, the runtime lag probe, the audio heartbeat watchdog); `eileen-crash` (panic hook with message cleaning, crash handler, guard client) and `eileen-guard` (minidump server outside the Job Object, core heartbeat, hang dumps, rate-limited core restart, fail-fast exit reporting); the `session` and `dev` toolset handlers; `eileen-cli` as the composition root wiring every component crate through its `Component` and every subcommand of section 3.5 that this package implements; `eileen scenario save` from the flight recorder; EcoQoS opt-out at start of every binary it owns.
- **Slice S1** (once WP3 and WP4 have merged): `eileen run --audio-source wav:s1.wav --audio-sink capture:out.wav` on the owner's PC with real speech and real Claude: the WAV says "Eileen, create a file called hello dot txt containing hi in a scratch repo"; the brain dispatches a Claude worker in a temporary git repository; the worker writes the file; the completion is announced; transcribing `out.wav` with Eileen's own recognizer contains the job's name. The same run in CI uses the transcript stub and the stub harness.
- **Acceptance.**
  1. S1 passes on the owner's PC (manual, recorded in the work package file) and its stub variant passes in CI.
  2. Killing `eileen-desktop.exe`'s stand-in mid-call yields a typed error to the caller, an incident with the sidecar's hang dump, and a restart within the backoff bound; the core keeps capturing.
  3. A hook guard test: the `PreToolUse` endpoint answers no decision, `ask` and `deny` per tier, never `allow`; with the endpoint down, a worker's guarded write is blocked (`onFailure: "block"`).
  4. The bus token cannot approve: every endpoint reachable with it is tested to reject approval-shaped requests.
  5. Idle footprint of the release (thin) `eileen.exe` with stub sidecars, measured with `scripts/measure-idle.ps1`: under 60 MB working set and under 2 percent of one core; numbers recorded.
  6. `eileen trace last` stitches one turn across the core, a stub sidecar and a stub worker by `turn_` ID, every line with file and line; `eileen incident show last` summarises a captured incident.
  7. The redaction canary extended to every sink this package writes (logs, incidents, share packs, recordings).

### 6.5 WP3: Claude harness and the brain

- **Branch** `wp3/harness`. **Depends on** WP0. **Desktop** no. **Wave** W1.
- **Owns** `crates/eileen-harness`, `crates/eileen-stub-harness`, `tests/harness-scripts/`, `scenarios/claude/`, fuzz targets `stream_json_line` and `control_message`, `docs/wp/WP3-harness.md`.
- **Builds.** A typed stream-json parser (line length capped before parsing; unknown `type` becomes `Unknown`; `system/init` capabilities recorded for feature detection); `ClaudeWorker` per section 2.5 (the real `.exe` resolved, spawned through `eileen_supervisor`, the constructed environment of section 2.12, the per-job settings file with HTTP hooks and `disableSkillShellExecution`, the generated MCP config pointing at `eileen-shim` with the job's role and token, plugin directories, `--permission-prompt-tool mcp__eileen__approve`, narration extraction from `> ` lines, cost from `result`, rate-limit parsing into `PlanUsage`, interrupt through `control_request`, cancel through the Job Object); `BrainBackend` with the warm session and hot spare, recycle with a summary, swap on crash or silence; model-id discovery from `system/init` written into the default config; git worktree management through the supervisor (`git worktree add`, branch creation, removal); `continue_session` jobs (`--resume <id> --fork-session`); `eileen claude install|uninstall` and `eileen doctor claude` (`claude auth status`, version range, never a credential file); the Codex app-server and ACP slots as types that return `RouteUnavailable`, with the mapping table of section 3.3.8 in the crate doc; the DeepSeek slot as configuration only; `eileen-stub-harness` (stream-json playback of `tests/harness-scripts/*.ndjson` with directives for delay, crash, malformed line, permission request and narration); the first scripts, recorded from real sessions on the owner's PC and redacted; `cargo xtask contract-claude` (owner's PC only: runs the scripts' prompts through the real CLI and compares event shapes).
- **Acceptance.**
  1. Against the stub harness in CI: a worker job runs to completion, narration lines become `JobNarration`, an injected permission request reaches the `approve` tool path, a crash marks the job failed with a reason, and cancel leaves no child process (nextest leak detection).
  2. Fuzz target `stream_json_line` runs clean for 10 minutes; its corpus replays on Windows.
  3. On the owner's PC (manual, `requires = ["real-claude"]`): the brain reaches `system/init` with connectors off; warm-turn time to first text measured over ten turns on a quiet machine and recorded; Haiku 5.5 with thinking off compared with Haiku 4.5 and the faster one written into the default config; a worker answers a learned-skill question the way learning-loop report row 7 did, now spawned by Eileen.
  4. A test asserts the spawned environment holds no `CLAUDE*`, `ANTHROPIC*` or `MCP_*` variable from the parent, and that no code path opens a path under the user's `.claude` directory (tidy plus a runtime file-access check in the stub run).
  5. `eileen doctor claude` reports a signed-out CLI clearly and suggests `claude auth login`, without running it.

### 6.6 WP4: Voice (audio, speech, dictation, voices)

- **Branch** `wp4/voice`. **Depends on** WP0. **Desktop** yes (audio devices). **Wave** W2. Speech-model tests and benchmarks take the exclusive build slot.
- **Owns** `crates/eileen-audio`, `crates/eileen-audio-win`, `crates/eileen-speech` (including `models.toml`), `crates/eileen-dictation`, `scenarios/voice/`, `scenarios/dictation/`, fuzz targets `speech_frame` and `dictation_cleanup`, `docs/wp/WP4-voice.md`.
- **Builds.** `eileen-audio`: cpal WASAPI capture and render at 48 kHz with 10 ms buffers, resampling, sonora AEC3 with the played PCM as reference, the SPSC ring buffer (loom-tested under `cfg(loom)`), mute that stops the stream, duck and stop with samples-played accounting, PCM playback from the speech pipe, `AudioSource` and `AudioSink` with `WavSource`, `CaptureSink` and `RoomMixer`, the audio heartbeat and tick ring (no tracing and no allocation in the callback, checked by a counting allocator in debug builds), the energy barge-in fallback; `eileen-audio-win`: endpoints, the Communications category option with ducking opted out, MMCSS. `eileen-speech`: the pipe protocol of section 3.3.7; Silero VAD, Nemotron streaming, Smart Turn with the Rust log-mel and the fusion rule, Kokoro with clause chunking and the phrase cache, barge-in window events, the `Synth` trait with Kokoro and Pocket TTS and the per-reply watchdog, CAM++ enrollment with the consent flow and the speaker check, the name-recognisability check for renames, the candidate embedding model with a WordPiece tokenizer (adopted only if it meets the latency budget of section 3.3.10), hotwords (adopted only if beam search measures acceptable), Parakeet v3 on demand, model download and verification for `eileen setup`, `eileen voice` subcommands, a transcript-injection stub mode for CI (`eileen-speech --stub <file>`), EcoQoS opt-out at start. `eileen-dictation`: tier-0 cleanup rules with SpeakoFlow's 79 conformance cases (MIT, attributed) and Eileen's typed-slot positives and negatives, implementing the `DictationCleaner` port.
- **Acceptance.**
  1. On the synthetic echo test scenario through `RoomMixer`: zero far-end words leak into the transcript and the user's command is recovered.
  2. Barge-in on the owner's PC with speakers: onset to silence at most 150 ms at p95 over 20 trials; the heard prefix is correct.
  3. Idle listening: speech process under 8 percent of one core and the full stack about 1.25 GB resident; recorded.
  4. End of turn: on a consented recording set of the owner's speech (kept local, never committed) the half-finished-sentence cases do not end early; until the owner records it, the synthetic set is used and the gap is noted.
  5. Kokoro first clause and RTF, Pocket TTS TTFA and RTF at two and four threads, Parakeet Unified against Nemotron (WER, partial latency, tail, RSS), Kokoro int8 against fp32 under equal load: measured with the exclusive slot and recorded with the machine state.
  6. Voice enrollment refuses a consent clip from a different speaker (two Kokoro voices as stand-ins in CI with the speaker model cached nightly); "delete Sam's voice" removes every file.
  7. Dictation cleanup passes all conformance cases and the negatives ("I actually enjoyed the movie" untouched).
  8. Mute: capture stopped within one buffer, and the Windows microphone-in-use indicator goes off (manual check recorded).

### 6.7 WP5: Desktop control

- **Branch** `wp5/desktop`. **Depends on** WP0, WP2. **Desktop** yes. **Wave** W3.
- **Owns** `crates/eileen-desktop`, `crates/eileen-desktop-win` (including the `testpad` fixture as an example binary), `scenarios/desktop/`, `docs/wp/WP5-desktop.md`.
- **Builds.** Port the computer-use proof (`C:\Users\shubh\Projects\eileen-workfiles\2026-10-scratch\computer-use\proof\` on the owner's PC) into the neutral server logic (`eileen-desktop`: tool handlers, safety rules, element indexing, selector identity and similarity, DPI mapping math, set-of-mark drawing on image buffers, the app-name scorer with the margin rule, the insertion ladder's decisions, the manifest and app capabilities) and the Windows backend (`eileen-desktop-win`: per-monitor-v2 first, UIA cache requests on a dedicated MTA thread with deadlines, `SetWinEventHook` and the UIA focus handler feeding `DesktopDelta`s, `BitBlt` and `PrintWindow` capture, JPEG and PNG encoding at 1280 px, `SendInput` with the foreground guard and cursor and foreground restore, `GetLastInputInfo`, the `shell:AppsFolder` index with AUMID launch and change watching, DWM frame rects, window moves and snaps against work areas, `EM_REPLACESEL`, the port of Handy's clipboard transaction with attribution, terminal detection, presentation state, microphone-in-use detection, bounded cross-process window calls). All 17 tools of section 3.3.9; the foreground lease from the core's input lock checked on every foreground call; `DesktopIntent` with the configurable wait; text through `sanitize`; self-healing `find` by selector similarity. Fix the `testpad` fixture so its controls stay inside its window at any DPI.
- **Acceptance.**
  1. On owned fixtures (testpad, Notepad launched by AUMID and closed unsaved, an isolated Edge profile): background Invoke and SetValue verified by the fixture's own title state, with the user's cursor and foreground unchanged; password fields refused; every blocked combo refused; a foreground action with the user idle succeeds and restores the previous foreground; with the user active it asks or refuses honestly, never loops.
  2. `desktop_state` p95 under 100 ms on the owner's desktop; the `uia_tree` of Notepad and Explorer within the proof's numbers; recorded.
  3. A deliberately hung UIA provider (a fixture that blocks) makes the call fail with a typed error within its deadline, and the supervisor's kill and restart leave the core listening.
  4. A Claude worker (real, owner's PC) opens Notepad, types a sentence through `desktop_set_value`, and reads it back, via the shim and the dispatcher; the trajectory has every step with a verification hint.
  5. Dictation insertion: exact text in a RichEdit fixture by `EM_REPLACESEL`, in Notepad and an isolated Edge text area by the clipboard transaction, with the user's clipboard restored (a full-format snapshot test with an image on the clipboard).
  6. `scenarios/desktop/` covers open app (including the ambiguous DaVinci case), move to monitor, snap, what am I looking at, insert text; they replay in simulation and run in the desktop lane.
  7. DPI mapping unit tests for 100, 125, 150 and 200 percent and for monitors left of and above the primary (fakes); the fleet covers the real multi-monitor cases.

### 6.8 WP6: App adapters

- **Branch** `wp6/apps`. **Depends on** WP0, WP2. **Desktop** yes. **Wave** W4.
- **Owns** `crates/eileen-apps` (including `models.toml`), `crates/eileen-apps-win`, `adapters/resolve/`, `scenarios/apps/`, fuzz target `resolve_reply`, `docs/wp/WP6-apps.md`.
- **Builds.** The SMTC manager held for the process lifetime with event subscriptions, derived live position and `apps_media`; `apps_open_url` with `ShellExecuteW`, the YouTube watch-URL builder with `&t=` and the wait for the expected media title (ad handling); the Resolve Lua bridge (from the app-hooks report, about 130 lines with its JSON encoder), its supervision under `fuscript.exe`, the `Resolve.exe` gate, drop-frame-aware absolute timecode math in Rust, every `apps_resolve` op, and an investigation of whether a DRX can carry a window placed at a given box (if yes, a generator; if not, recorded, and the correction stays with the worker's computer use); YuNet through `ort` against the shipped DLL, face picking by position, the variance-of-Laplacian sharpness metric; the late-bound `IDispatch` helper for Word, Excel, Explorer and Photoshop on per-target STA threads with timeouts and a message filter; the recent-work index (jump lists, `.lnk` parsing, Office MRU, the folder watcher); the Photoshop ops tested against a fake COM target; the manifest and app capabilities; `eileen doctor resolve`.
- **Acceptance.**
  1. On a dedicated test project that the owner agrees Eileen may create in Resolve Studio 21.0.1 (section 7): `goto` 15:30 lands at the computed timecode for 24, 25, 29.97 drop-frame and 30 fps timelines starting at 01:00:00:00; `export_still` writes a frame; the measured latencies of `SetCurrentTimecode`, `GetCurrentVideoItem` and `ExportCurrentFrameAsStill` are recorded.
  2. With Resolve closed, no call stalls: the gate answers "Resolve is not running" in under 50 ms.
  3. YuNet finds the faces in a committed synthetic test image (generated, no real person), picks left and right correctly, and sharpness rises after a known sharpen filter.
  4. SMTC: pause, play and seek on a test media session (silent WAV player fixture), confirmed by the next event; on the owner's PC, a YouTube URL at `&t=200` plays from 3:20 and a seek to 60 s through `apps_media` is confirmed once the session returns.
  5. Word selection and Excel active cell read from fixtures the test creates and closes without saving.
  6. Fuzz target `resolve_reply` clean for 10 minutes.
  7. `scenarios/apps/` covers Resolve goto, YouTube play-at and seek, pause and play, Word selection; B1's deterministic phases run end to end with a stub worker for the correction step.

### 6.9 WP7: Intent, settings and learning

- **Branch** `wp7/understanding`. **Depends on** WP0, WP1, WP2, WP3. **Desktop** no. **Wave** W3, may continue into W4.
- **Owns** `crates/eileen-intent`, `crates/eileen-learn`, `crates/eileen-config`, `scenarios/learning/`, `scenarios/settings/`, fuzz targets `config_toml`, `skill_record`, `command_grammar`, `docs/wp/WP7-understanding.md`.
- **Builds.** `eileen-intent`: the grammar table of section 2.4 and its slot resolvers, the template matcher (port the std-only matcher from `C:\Users\shubh\Projects\eileen-workfiles\2026-10-scratch\learning\matcher\`), BM25 hints, semantic comparison and confirmed phrasings, `Ambiguous`, depth overrides, `introspect`, implementing `IntentResolver`. `eileen-learn`: the recorder, the store (`learn.sqlite`: trajectories, skills, skill versions, uses, facts, ledger, answers cache, tombstones), the plugin folder writer, the linter (every rule of the learning-loop report section 3.11 and acceptance test 4), the ladder as pure functions over `LadderPolicy`, the replay engine issuing `ToolCallRequest`s with origin `Replay`, the librarian scheduler and prompt and the `learn` toolset handlers, the user model with delta updates, the ledger with off-the-record mode, the regret-window signal handling, forget with tombstones, per-skill measurement, `eileen skills` subcommands. `eileen-config`: `load`, `validate` (the `SettingsValidator` port), comment-preserving `apply` with `toml_edit`, the `SettingSpec` registry for every key in section 3.3.11, safety bounds, profiles, `explain`, the generated schema and commented default, keyring secrets, the file watcher, `eileen config` subcommands.
- **Acceptance.**
  1. The learning-loop report's eleven CI acceptance tests (section 6.5 there) pass as written, with skill versions in SQLite in place of git.
  2. The matcher golden file: the ten utterances of learning-loop report row 15 give exactly those results; p99 under 1 ms with 5,000 skills.
  3. The resolver golden file covers every grammar phrase of section 2.4, ten paraphrases per phrase that must not trigger an action unconfirmed, and the ambiguity cases; a fuzz run of `command_grammar` is clean.
  4. Settings: "call you Nova" runs the three checks (with a fake speech check in CI), applies, updates the addressee name and the hint list, and "undo that" restores Eileen; a hotkey owned by another process is rejected before applying; a tier-3 relaxation is refused; a tier-2 relaxation requires the logged decision; a hand edit with a syntax error keeps the previous version and reports the line.
  5. On the owner's PC (manual, real Claude): the librarian distills one real trajectory into a draft that passes the linter; the second run of the same kind of request uses fewer model turns than the first (the B8 shape, recorded).
  6. `scenarios/learning/` (what have you learned, forget that, rollback, replay with a failed check handing off) and `scenarios/settings/` replay in simulation.

### 6.10 WP8: HUD, shell and Claude integrations

- **Branch** `wp8/hud`. **Depends on** WP0, WP2, WP3, WP4. **Desktop** yes. **Wave** W5.
- **Owns** `crates/eileen-hud`, `crates/eileen-shell-win`, `integrations/claude-plugin/`, `.claude-plugin/marketplace.json`, `scenarios/hud/`, `scenarios/claude-plugin/`, fuzz target `hud_line`, `docs/wp/WP8-hud.md`.
- **Builds.** The neutral HUD layout from `HudState` to a draw list with snapshot tests (every corner, scale and state); the Win32 renderer with the overlay and the strip, `WDA_EXCLUDEFROMCAPTURE`, per-monitor DPI, high contrast; the HUD protocol; tray, hotkeys (with the conflict test used by settings), the dictation key's low-level hook, toasts, and the Start menu shortcut with the AppUserModelID used by `eileen setup`; the `eileen` Claude plugin per the claude-mods report (handoff skill, `.mcp.json` launching `eileen-shim --role session` by absolute path written at install, the display-only mod with its tests, types regenerated for the installed CLI version) and the marketplace file.
- **Acceptance.**
  1. Draw-list snapshots for every `HudState` variant; the HUD survives being killed and restarted with the current state.
  2. On the owner's PC: the HUD never takes focus, clicks pass through the overlay, Allow and Deny on the strip resolve an approval, and a screenshot from `desktop_screenshot` does not contain the HUD.
  3. Idle footprint of `eileen-hud.exe` (raw Win32) measured with `measure-idle.ps1` while idle and while updating ten times a second; recorded.
  4. Mute hotkey and HUD mic button both stop capture; changing the mute key to one another process owns is refused.
  5. `claude plugin validate --strict` and `claude plugin test` pass on the plugin; the validation report shows no gating hook and only the reviewed host calls.
  6. On the owner's PC (manual, opt-in install): `/eileen:handoff` from a Claude session creates a `continue_session` job that resumes a fork of that session.

### 6.11 WP9: End to end, benchmarks and the test fleet

- **Branch** `wp9/e2e`. **Depends on** WP0 to WP8. **Desktop** yes. **Wave** W6.
- **Owns** `crates/eileen-e2e`, `scenarios/bench/`, `TESTING.md` (completing WP0's skeleton), fuzz targets `scenario_file` and `fleet_result`, `docs/wp/WP9-e2e.md`.
- **Builds.** The real-desktop scenario runner (fixtures; steps through the loopback endpoint, typed input or audio synthesized with the scenario's stock voice and fed through `WavSource`; every desktop-lane expectation evaluated through Eileen's own sidecars; timing from the event trace with `ci_factor` on CI and strict limits on the owner's PC); benchmarks B1 to B9 with A/B runs and devhub JSON lines; the soak runner behind `cargo xtask soak`; `eileen scenario share` (redaction, preview, outbox) and `submit` (through the contributor's own `gh`, tier 2, or the issue-form fallback); `eileen test --fleet` (opt-in, schedule through a per-user Task Scheduler entry created only after an explicit yes, owned fixtures, throwaway data directory, results file, preview, upload as a comment on the week's fleet issue); ingest, clustering, flake tracking and the catalogue data used by `cargo xtask codegen`; `cargo xtask triage` (owner's PC, Claude worker drafting); the completed `TESTING.md`; the M6 integration demo.
- **Acceptance.**
  1. Every desktop-lane scenario in the repository runs on the owner's PC with a recorded result; failures are issues, not skips.
  2. B1 runs end to end for real on the owner's PC (Resolve Studio, real Claude) and its phases are recorded; B8 shows the second run cheaper than the first in time, tokens and worker turns.
  3. A friend's PC (or a second Windows 11 machine) installs Eileen from `TESTING.md` without asking the author anything, saves a scenario by voice, previews it, and submits it as a pull request from their own account; CI validates and replays it.
  4. A fleet run on two machines produces result files with no personal data (a test greps the files for the user name, machine name and home path), uploads them, and the nightly ingest clusters a deliberately failing scenario into one issue.
  5. The soak runs 4 hours on `windows-latest` with flat handles, GDI objects and threads and the core's memory slope under 1 MB per hour.
  6. The M6 demo script (ROADMAP.md) runs end to end with no restart and no typed command after `eileen run`.

## 7. Owner decisions, open questions and risks

Decisions only the owner can make (also returned to the workflow):

1. Accept or overturn the new decision records D17 to D38, in particular D21 (Eileen's own desktop server instead of Cua Driver), D27 (skill history in SQLite instead of git per skill) and D33 (the guidance form).
2. Repository settings: a ruleset on `main` (pull request required, `all-green` required, no force pushes, squash merges), auto-merge and branch deletion; and whether to move the repository to a free GitHub organisation for a merge queue, since several agents will merge in parallel.
3. Where the devhub results store lives: an orphan `devhub` branch in this repository or a separate small repository.
4. Whether fleet results are uploaded as comments on a weekly issue from each contributor's own account (the default here), or somewhere else.
5. Install sccache 0.18.0 and cargo-binstall 1.26.0 from their prebuilt releases, and optionally set `jobs = 6` in the user-level Cargo config; consider freeing disk (19.4 GB free).
6. Resolve: set External scripting to Local; allow Eileen to create one dedicated test project for WP6 and WP9; decide whether and when to move to Studio 21.1 or later (native MCP server, Python only in Studio).
7. The owner's nightly lane: opt in, set the nightly turn budget, allow the Task Scheduler entry and keep-awake.
8. Record a consented set of the owner's own speech (commands, half-sentences, pauses, side talk) for real-speech tests, kept on the PC and never committed.
9. Before the first binary release: accept GPL obligations for release binaries if the sherpa-onnx prebuilt contains espeak-ng, or require a TTS front end without it.
10. Nemotron and Parakeet Unified are under NVIDIA's Open Model License (commercial use allowed, not an OSI license): acceptable as downloaded models, or open-licensed models only (Parakeet v3 CC-BY-4.0)?
11. Historic names: may the dated records (DECISIONS.md D15 and D16, RESEARCH.md section 8) be scrubbed of the old working name and the franchise, or stay exempt from the house-style check as history?
12. Install the `eileen` Claude plugin into the owner's own Claude app (opt-in), and whether learned skills should ever load into the owner's own sessions (default no).
13. Optional code review on pull requests by a Claude worker, in addition to the deterministic gates, set up on the owner's own account.
14. The license under which contributed scenarios and consented audio enter the repository (MIT like the code, with audio under CC-BY-4.0 and a speaker consent record, is the proposal).
15. During calls: listen only for the assistant's name and stay silent (default here), or mute entirely.
16. Pocket TTS: the sherpa-onnx archive's README says non-commercial while its LICENSE says CC-BY-4.0; confirm with the maintainer before shipping a downloader.

Open risks (each has an owner package that measures it):

- Real-speech accuracy, Smart Turn on real pauses and addressee detection are untested (all corpora were synthetic). WP4.
- Echo cancellation with real speakers, music and video is untested; a WASAPI loopback reference may be needed. WP4.
- TTS speed is marginal on this CPU; the "about a second" ask is met only by the fast path and cached phrases. WP4 measures the alternatives.
- Foreground escalation while the owner is active is unsolved beyond asking; a signed UIAccess helper needs a certificate. WP5.
- Multi-monitor and mixed-DPI behaviour is untested on the owner's PC. The fleet.
- Whether a DRX can place a window at the face box is unknown; the worker's computer use is the fallback for B1's correction. WP6.
- Hosted Windows runners may not give UIA an interactive desktop; if not, desktop scenarios run only on the owner's and fleet PCs. WP0 and WP9.
- The claude CLI's stream-json control protocol, skill discovery flags and plugin behaviour are observed on 2.1.304 and are not a stable API; the weekly contract test and `eileen doctor` version range guard them. Anthropic has said `--bare` may become the default for `-p`; the driver must detect the auth failure and say so. WP3.
- Plan usage of the brain, the hot spare, the librarian and nightly lanes is unmeasured; caps start small. WP3 and WP7.
- Memory: a full session is close to the PC's limits; the residency rules and `brain.hot_spare` are the levers. WP2 measures.
- Windows CI time is unmeasured; partitioning and path filters are the levers. WP0.
- The candidate embedding model, hotword decoding cost and Parakeet Unified are unmeasured; each is adopted only on measurement. WP4.

## 8. Revision log

- 2026-10-20: first architect pass (this document). Pending critique by the workflow's five reviewers; declined suggestions and their reasons will be recorded here.

