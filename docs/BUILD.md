# Eileen v0.1 build specification

Written 2026-10-20 by the architect pass of the design workflow and revised the same day after five reviews (feasibility on the owner's PC, the owner's asks and the feel, safety and best-in-class, engineering quality, and the learning loop). This is the document parallel engineer agents build v0.1 from. It turns [OWNER-DIRECTION.md](OWNER-DIRECTION.md) (binding) and the twelve October research reports into one design, one Cargo workspace, one set of contracts and ten work packages. Section 8 lists what the reviews changed and which suggestions were declined, with reasons.

How to use it:

- Engineers: read section 0, then the work package you were given in section 6, then every section that package names. The contracts in section 3 are the only interface between packages. If a contract here is wrong, say so in your pull request and propose the fix as a separate small amendment pull request (section 6.1); do not quietly diverge.
- Reviewers: section 4b lists every check CI runs, and section 7 lists what is still the owner's call.
- Numbers come from the research reports, which say how and under what load each was measured. Almost every measurement on the owner's PC was taken under heavy contention, so read them as pessimistic, and read section 4.3's "typical load" column as what the owner will actually see while he works.

Reports this spec is built on (all present on 2026-10-20, in `docs/research/`): prior-art, subscription-harnesses, app-hooks, brain-latency, voice-stack, computer-use, rust-architecture, learning-loop, dictation-and-voices, claude-mods, engineering-practice, observability.

Documents no package owns: this file, `docs/DECISIONS.md` and `docs/OWNER-DIRECTION.md`. A package that needs to change them, or a file another package owns that has already merged, opens a separate pull request labelled `amendment` that touches only that file, and the owner reviews it.

## 0. Summary

v0.1 is one always-on Rust program for Windows 11 that listens whenever the microphone is not muted, understands what the user means, and either acts at once on the PC or starts Claude jobs that run in parallel while the conversation continues. Everything it does is recorded locally so the next request of the same kind is faster, and everything the user and the owner's friends try can be saved as an automated test.

The shape, in one paragraph. A core process (`eileen.exe`) owns the microphone, mute, the tray and hotkeys, the event bus, the job registry, permissions, the learning loop's decisions and a supervisor. Everything that can hang or crash runs in a supervised child process the core can kill: speech models (`eileen-speech.exe`), UI Automation and input (`eileen-desktop.exe`), app scripting such as the Resolve bridge (`eileen-apps.exe`), the on-screen layer (`eileen-hud.exe`) and every Claude session. The brain is a warm `claude -p` session on the owner's plan with a hot spare; it answers, dispatches jobs and may run a small set of deterministic actions (open an app, move a window, play or seek media, jump in Resolve), but it never moves the mouse or types. Common commands skip it entirely through a deterministic fast path. Long or hands-on work goes to Claude worker sessions, taken from a small warm pool where memory allows, that drive the PC through Eileen's own MCP tools under a permission gate, a path guard and an input lock. Claude is the only route in v0.1; Codex, Grok and DeepSeek are designed slots behind the same trait. Every OS-touching piece sits behind a platform trait, and the neutral crates build and test on Linux from the first commit.

Build order, thin slice first:

| Wave | Packages (at most two compiling at once) | What exists at the end |
|---|---|---|
| W0 | WP0 phase A | Workspace with every pin and native dependency declared, contracts, the build guard, basic gates green on Windows and Linux, the required check switched to `all-green`, every crate as a documented stub |
| W1 | WP1 core and simulation, WP3 Claude harness; WP0 phase B (the remaining repository checks and the nightly, weekly, long and release workflows) and WP6's Resolve correction spike (no compiling) alongside | The core decides everything in simulation, starting from a minimal core in the first pull request; Claude brain and workers run from Rust; the tier-2 approval path is proven against the real CLI; the Resolve edit route is chosen |
| W2 | WP2 runtime, WP4 voice | **Slice S1, live:** the real microphone and speakers, mute key and tray, a console view, one fast-path command, one barge-in, one brain answer and one Claude job, latencies recorded. No W3 package starts until it passes on the owner's PC |
| W3 | WP5 desktop control, WP7 phase A (intent, settings, capture and the first rungs of learning) | Open any app, move windows, computer-use jobs; settings by voice; skills drafted and loaded by workers |
| W4 | WP6 app adapters, WP7 phase B (recipes, replay, the full ladder) | Resolve, YouTube and media, Office, Explorer, recent files; model-free replay with step checks |
| W5 | WP8 HUD and Claude integrations | The visual layer, the first-run surprise path, the opt-in `eileen` Claude plugin |
| W6 | WP9 benchmarks and the test fleet | The owner's benchmarks as timed scenarios; friends' PCs as a test fleet; the M6 demo |

## 1. The target experience

Each demo below is a script a tester can follow. "Under the hood" names the mechanism and the section that specifies it. Every demo (E1 to E18) becomes at least one scenario in `scenarios/` (section 4.8), so these scripts are also the first tests. The owner's PC has one 1920x1200 monitor, so every demo works on one screen; steps that need two monitors are marked and run on fleet PCs that have them.

Timing words: "at once" means under about 0.6 s from the end of speech to the first sound, which only the fast path and cached phrases reach. "Quickly" means a brain turn: the target is 1.5 s to the first spoken word at p50 on a quiet PC, and the honest typical-load number is 2 to 4 s (section 4.3). Whenever a turn goes to the brain or a job, a short earcon plays within 150 ms so the user knows Eileen heard. "Actions" through the brain (a paraphrase of a known command) take about 3 s to the first effect. Worker jobs start in about 1 to 2 s from the warm pool, or 30 to 90 s on a cold start under load, and Eileen says which. Deep answers take tens of seconds and arrive while the user keeps talking.

### E1. Always listening, mute one key away

| You do or say | What happens | Under the hood |
|---|---|---|
| Sign in to Windows | Eileen starts from the sign-in entry. The HUD strip shows "loading speech" for 10 to 20 s while the speech models load, then the mic as listening, and "brain warming up" until the Claude session is ready (cold start measured 39 to 48 s on a loaded PC). Fast-path commands work as soon as the speech models are loaded | 2.1, 2.3 |
| "Eileen, what time is it?" | Spoken answer at once, no model | fast path, 2.4 |
| (to a friend) "Want coffee?" | Nothing spoken. The HUD shows the words greyed as heard but not for Eileen, for a few seconds; they are never written to disk | addressee gate, 2.8; privacy, 2.12 |
| "That was for you" (or tap the greyed line) | The last greyed utterance runs as if it had been addressed | 2.8 |
| Press the mute key (default Ctrl+Alt+M), click the mic on the HUD or the tray | The capture stream itself stops; the Windows mic indicator goes off; the HUD mic turns red | 2.8, 2.9 |
| "Eileen, are you there?" while muted | Nothing. Muted means nothing is captured | invariant 9 in 4.7 |
| Press the mute key again | Listening resumes; a short earcon | |

### E2. Open any app

| You say | What happens | Under the hood |
|---|---|---|
| "Open Premiere" | Premiere launches; "Opening Premiere Pro." from the phrase cache (launch phrases are rendered in the background after the app index is built) | fast path, app index, `desktop_launch_app` (2.6) |
| "Get Premiere up for me" | A paraphrase the grammar does not know: the brain calls the same `desktop_launch_app` tool directly, so the app starts about 3 s after speech with no worker. Once the regret window closes clean, the pairing becomes a phrasing candidate (2.10) | brain `act` toolset, 2.3 |
| "Open the video editor" | "DaVinci Resolve or Premiere Pro?" Both are video editors, so it asks once | one clarifying question, 2.2 |
| "Resolve" | Resolve launches; "the video editor" is stored as a phrasing fact, so next time it opens Resolve directly | user model, 2.10 |
| "Snap it to the left half" | Once Resolve's main window exists (not the splash), it snaps to the left half of the work area. With two monitors, "put it on the left screen" moves it there (`requires = ["monitors>=2"]`) | `desktop_window` snap, fast path |
| "Close Notepad" | Closes it if Notepad reports nothing unsaved; otherwise asks first | `WindowAction::Close`, tier rule in 2.12 |
| "Turn it down a bit" | System volume down one step, journaled so "undo that" restores it | `desktop_volume` |

### E3. A specific video, at a time

| You say | What happens | Under the hood |
|---|---|---|
| "Play the Mount Rainier timelapse on YouTube" | If the video is known (played before, mentioned in the conversation, or in the recent-work index), the built-in grammar resolves it and its watch URL opens in the default browser at once, no model. If not, a Claude worker from the warm pool opens the YouTube results page in the user's own browser, reads the results from a screenshot, and opens the matching video. Narration: "Found it, playing." The URL and title go into the media history | `apps_recent` (media), `apps_open_url`, worker computer use, 2.7 |
| "Go to 3:20" | Playback jumps to 3:20 through the system media controls, no keystrokes | `apps_media` seek over SMTC, 2.7 |
| "Pause" / "Play" | Instant, through SMTC | fast path |
| Another day: "play that timelapse at 1:10" | The built-in grammar matches "play <known media> at <time>" and resolves "that timelapse" through the media history; the URL opens at `&t=70s` with no model call. This needs no learning; the learning loop covers request kinds the grammar does not know (E11) | 2.4 |

A preroll or mid-roll ad can follow a load or a seek; Eileen waits for the SMTC session to come back with the expected title before saying it worked (app-hooks report section 5).

### E4. "What am I looking at?"

| You say | What happens | Under the hood |
|---|---|---|
| "What am I looking at?" | "Resolve, the Color page of 'Beach Edit', on your left half. Nothing is selected." Answered from DesktopState in the brain's context block, with no screenshot | DesktopState, 2.6; context blocks, 2.3 |
| "What's in this picture?" (a photo is open) | The brain calls `desktop_look`, gets one downscaled screenshot, and describes it | 2.3 |
| "Copy the second paragraph into my email" | A worker job reads the page and the email field through the desktop tools and inserts the text. Typing into a mail app is allowed; pressing Send is a separate tier-2 step | 2.6, 2.12 |
| (while a job works in a window behind others) | The HUD shows a small live thumbnail of the window the job is working in, without moving it | `HudState.watch`, 2.9 |

### E5. "Launch three agents to ..."

| You say | What happens | Under the hood |
|---|---|---|
| "Launch three agents to fix the flaky login tests in the demo repo" | "Starting three Claude workers on flaky login tests." Three git worktrees and branches are created, three jobs start, and the HUD shows rows "agent one" to "agent three". If memory would drop below the reserve, the third waits: "I'll start agent three when memory frees up." If the plan's five-hour window is above 80 percent, Eileen asks first | fan-out and admission, 2.5; plan-limit awareness, 2.3 |
| (keep working; say nothing) | Short narration only for the focus job, at most one line every 20 s, attributed: "Claude: agent two found the race in the session cache." | narration, 2.5 |
| "How are they doing?" | "Two of three pass the tests; agent three is still running them." | jobs block, 2.3 |
| "Show me agent two's changes" | The HUD shows the diffstat and test results; "open it" opens the diff in the editor | 2.5 |
| "Keep agent two's fix" | Agent two's branch is merged into the user's branch (local, reversible); the other worktrees are removed after a one-sentence confirmation because they hold unmerged work. In compete mode Eileen first says how the three results differ in one sentence | tiers, 2.12 |

The demo repo is a fixture the scenario creates (`scenarios/bench/_fixtures/flaky-login/`, a small Python project whose flaky tests run in seconds), so the demo never needs three cold Rust builds at once.

### E6. Coding by voice

| You say | What happens | Under the hood |
|---|---|---|
| "In the eileen repo, add a json flag to the status command and run the tests" | One Claude worker job edits the user's checkout in place, after recording a snapshot ref so "undo that" can restore it; a worktree is used only when the user's uncommitted files overlap or several agents run. The worker runs the tests with the user's own toolchain. Approvals for anything beyond the repository come back by voice | worker layer, 2.5; environment, 2.12 |
| Hold the dictation key in VS Code and say "if result is none return early" | The text lands at the caret in code style (no capital, no period), local only | dictation, 2.8 |
| "Eileen, rename that variable to session id" | "That" is resolved from VS Code's window title (file and workspace) and a small crop around the caret; if unsure, Eileen asks which one. A worker job edits the file; the change is in git and can be undone | 2.5, 2.6 |

### E7. The Resolve benchmark, done for real

"Open the video editor, go to 15:30 and make the face of the girl on the right less blurry." (The owner's own words; B1 uses them literally.)

| Step | How | Rung |
|---|---|---|
| Which editor | The first time, one question ("DaVinci Resolve or Premiere Pro?"); the answer becomes a phrasing fact | 2.2, 2.10 |
| Resolve open and scriptable | If `Resolve.exe` is not running, launch it (tier 1) and poll the bridge's `hello` until it answers, saying "Resolve is starting" if that takes more than 3 s. If External scripting is not set to Local, Eileen says exactly which preference to change | apps, 2.7 |
| The right project and timeline | If Resolve shows the Project Manager, Eileen asks which project, offering the recent ones ("Beach Edit or Wedding?"), and loads it; if the project has several timelines and none is current, it asks once | scripting |
| Go to 15:30 | Open the Edit page (the playhead cannot move on the Media or Fusion pages); the bridge reads the timeline's start timecode and frame rate; Rust computes the absolute timecode (01:15:30:00 for a 01:00:00:00 start, drop-frame aware) and calls `SetCurrentTimecode`; the playhead is read back | scripting |
| The girl on the right | Export the current frame as a still; the local YuNet detector finds faces; take the right-most. The HUD shows the crop and Eileen asks "This face, on the right?" before any edit | scripting plus local vision; confirmation, 2.7 |
| Keep it undoable | Before changing anything, Eileen adds a color version on the clip (`eileen-<job>`), so "undo that" loads the previous version | scripting |
| Less blurry | The route WP6's spike picks (section 2.7), in this order of preference: a Fusion composition on the clip with an ellipse mask keyed from per-frame face boxes (Eileen tracks the face itself by running YuNet on stills every few frames) feeding a sharpen node, imported from a generated file; a generated DRX grade with a window, if the format allows placing it; and only as a last resort a Claude worker on the Color page with screenshots and the input lock. Narration: "Adding a sharpen mask on her face." | scripting first, computer use last |
| Check | Export the frame again; sharpness inside the face box (variance of the Laplacian) must rise by at least 15 percent; otherwise try once more, then report honestly and offer to undo | local vision |
| Learn | The trajectory becomes a draft skill `resolve-sharpen-face` with slots for the time and which face (2.10) | |

Measured as benchmark B1 (section 4.7) on the owner's PC. B1 passes only when the sharpness gain is at least 15 percent and the owner confirms the result. The second run must use fewer model turns and less time than the first.

### E8. The Photoshop equivalent (designed; Photoshop is not installed on the owner's PC)

"Open Photoshop, open the image where I was editing the girl at the beach, and make her face less blurry." The design is in section 2.7: candidates from Photoshop's recent files over COM plus the recent-work index and the ledger's record of which documents the user actually spent time in, ranked by name, recency and editing time. If two stay close, or the file is named like `IMG_4412.psd`, the HUD shows a contact sheet of up to nine thumbnails and Eileen asks in one sentence. Open over COM; duplicate the layer (keeps it reversible); detect the face locally and show the crop for confirmation; feathered elliptical selection; Smart Sharpen on the duplicate. Neural Filters and Generative Fill spend Adobe credits and are tier 2. The code is written and unit-tested against a fake COM target; the real-app scenario is marked `requires = ["photoshop"]` and is skipped until someone with Photoshop runs it on the test fleet.

### E9. Many jobs, one conversation

| You say | What happens |
|---|---|
| "How's the render?" while a coding job and a Resolve render run | "The render is at 60 percent; the test agent found two failures." The render is an app task the core tracks through the Resolve bridge with no Claude session; pronouns resolve against the jobs block (2.5) |
| (Eileen is mid-sentence) "Stop." | Playback ducks within one 32 ms window and stops by the third; the transcript records only what was actually played; nothing after "Stop" is answered. Any job acting on the desktop pauses too |
| (Eileen is mid-sentence) "Mm-hm." | Eileen pauses for a moment, hears that it was only a backchannel, and carries on from where she paused |
| "Go on" | After a real interruption, plays the part the user did not hear |
| "Actually, tell the test agent to only look at the parser" | The instruction is steered into that job's session (interrupt, then a new user turn) |
| "Cancel the render" | Cancelled; the job ends in exactly one terminal state |
| (chime; any video playing is paused) "Claude wants to run git push on the login branch. Allow?" "Yes, push." | A tier-2 approval by voice needs the action word, or a click on the HUD's Allow. The worker resumes. "No" denies. If nobody answers within 20 s the voice window closes but the request stays on the HUD (and a toast) until the job's approval deadline, 30 minutes by default; then it is declined |
| "Send this one to a stronger model instead" | Reroute: the job is cancelled and restarted on the deep model with the old transcript as context |

### E10. Thinking together, and the best answer at conversational speed

| You say | What happens |
|---|---|
| "What's the best place to eat near me?" | The brain answers quickly with something useful that does not pretend to know current facts ("You liked the ramen place on 5th; I'm checking what's open now."), and in the same turn starts one deep worker (strongest model, thinking on) that runs several web searches in parallel in one turn. When it lands, two sentences are spoken ("Better answer: ...") and the full list with links, hours, distance and reasons appears on the HUD. If the user is talking, it goes to the HUD only |
| "Closer." / "I loved that place." | Kept as user-model facts; the next answer uses them. The sources the user accepted are remembered for this kind of question |
| "I'm trying to decide between SQLite and a flat-file store for skills; think it through with me" | Quick take spoken, a considered worker explores both (reads the code, writes a small benchmark in a scratch worktree, compares), and Eileen talks through the trade-offs while it works. "Quick answer" or "think hard about this" overrides the depth |

### E11. It learns

| You say | What happens |
|---|---|
| (E7 again, a day later: "go to 42:10 and sharpen the guy on the left") | The worker's session has the learned skill and is told to follow it, so there is no exploration: fewer turns, no dead ends. Section 2.10 has the table of what each run of each benchmark should look like; B8 measures it |
| "Learn from that now" | The librarian distils the last job at once instead of waiting for idle time |
| "What have you learned?" | The newest two or three items, one sentence each; the HUD shows the full list with level, success rate and time saved |
| "What do you know about me?" | The user-model facts, grouped; each can be corrected or forgotten |
| "Show me how you do that" | The HUD shows the skill's steps and checks, each line coloured by where it came from |
| "No, my timelines start at zero" | The fact is corrected; a skill that relied on it is queued for repair and drops one level |
| "Forget that" | "Forget the Resolve sharpen skill and its four recordings?" "Yes." The skill, its versions, its evidence and every copy in incidents and recordings are deleted, and a tombstone stops the librarian relearning it |

### E12. Dictation better than the paid tools

Hold Right Ctrl (configurable) in any app and speak; words appear in the HUD while speaking; on release, clean punctuated text lands at the caret: fillers, stutters and "scratch that" removed, "at 3, no, 4 pm" becomes "at 4 pm", code fields get literal text, chat drops the final period. The user's own clipboard is preserved. "Undo that" removes the last insertion. Corrections the user makes afterwards teach the vocabulary. An optional Claude "scribe" polishes tone when the user turns it on (section 2.8). The key is read through Raw Input, so Eileen never sits in the keyboard path.

### E13. Any voice, including a friend's

| You say | What happens |
|---|---|
| "Eileen, learn a new voice" | "Whose voice? They need to be here, or you can drop in a clip." "Sam's." Sam reads a short consent sentence and a passage (about 30 s). A same-speaker check compares the two; the model's terms are shown once; the profile "Sam" is saved on the PC only. A dropped clip is accepted only with Sam's own recorded consent sentence |
| "Use Sam's voice for announcements" | Announcements use Sam's cloned voice. The engine is chosen per voice: Pocket TTS by default, Chatterbox if the user picks it and it measured fast enough on this PC. An engine slower than real time renders a whole line before playing it ("rendering in Sam's voice" on the HUD) instead of stuttering; conversation stays on the fast default voice and Eileen says so once |
| "Back to normal" | Default voice |
| "Delete Sam's voice" | Reference clip, consent clip, caches and profile removed |

No watermark is applied by default on any engine; watermarking saved or exported audio is an opt-in setting (owner decision of 2026-10-20).

### E14. Customise anything by saying it

| You say | What happens |
|---|---|
| "Eileen, I want to call you Nova now" | Eileen checks the name first: not a common word, not a household name it knows, and its own speech recognizer hears "Nova" correctly in three synthesized sentences. Then: "Okay, I'm Nova now. Say undo that to go back." The answer-to name, the addressee cue, the recognition hint and how it refers to itself all change on the next turn, with no restart |
| "Talk less" / "be funnier" / "always answer in one sentence" | Personality and verbosity change on the next turn; confirmed in one sentence |
| "Stop announcing progress on renders" | That announcement row changes |
| "Put the HUD bottom left and make it smaller" | HUD moves and scales |
| "Change the mute key to F8" | Eileen tests that F8 can be registered (another app may own it) before applying |
| "Always let Claude move windows without asking" | Window moves are already reversible and silent; Eileen says so instead of adding a rule |
| "Undo that" | Reverts the last settings change; settings are versioned locally |
| "Save this as editing mode" / "switch to work mode" | Profiles |
| (on the HUD) the settings panel | The common settings as clickable controls; every change goes through the same flow and can be undone |

Nothing a setting does can let Eileen enter credentials, make payments, or skip a confirmation for a consequential action without an explicit, logged decision (section 2.11). Safety-relevant settings change only on the user's own voice or click, never because a model proposed it.

### E15. Something went wrong, save that as a test

| You say | What happens |
|---|---|
| "Eileen, that went wrong" or press Ctrl+Alt+Shift+B | The last ten minutes are frozen into a local incident: event log, spans, inputs (replayable), process table, last DesktopState |
| "Save that as a test" | The last request becomes a scenario file: what was said (as text), the starting state, every step Eileen took, and checkable expectations inferred from what changed. If it failed, Eileen asks once "what should have happened?" and stores the answer |
| `eileen scenario share last` | A preview shows exactly what would leave the PC after redaction; Eileen replays the redacted copy locally to make sure it still tests the same thing; on approval, `eileen scenario submit` opens a pull request from the user's own GitHub account. CI replays it in simulation on every pull request from then on |

### E16. The feel

| The owner asked for | Mechanism | Proven by |
|---|---|---|
| Understands half-finished speech and context ("make that bigger", "no, the other one", "go back to what we tried an hour ago") | Turn end needs Smart Turn plus transcript continuation cues plus a silence cap that stretches when the sentence is unfinished; the brain gets the focus history, the last referenced element and the episodic ledger; "no, the other one" right after a cancelled action goes to the job that was acting | sim invariants; scenarios `feel/*` |
| Knows when it is addressed and when a thought is finished | Local addressee gate: the name, a conversation window that stays open while the user is engaged, the command grammar, and a local score for unnamed requests about the screen when the user is alone, with the brain able to answer "not for me". Other audio (a video, a call) suspends the window | room-talk scenario: at least 9 of 10 ignored; conversation scenario: at least 8 of 10 unnamed requests acted on, at most 1 of 10 false actions |
| Stops at once when interrupted, resumes from what you heard | Duck at the first VAD window, stop by the third (measured parts give 70 to 110 ms); a backchannel resumes; played samples mapped to text; the brain is told exactly what was heard | sim invariants 2 and 3; B5 |
| Talks while it works, briefly | Workers mark narration lines; at most one spoken line every 20 s for the focus job, attributed to the route; never filler | narration tests |
| Several things at once, keeps you oriented | Job registry, jobs block, announcement policy, app tasks with progress | E9 scenarios |
| Thinks with you | Depth tiers, a deep worker in parallel, the HUD for detail | E10 |
| A visual layer | HUD overlay and strip; visible intent before clicks; live thumbnail of background work | E17 below |

### E17. The visual layer

The HUD shows, in a corner the user chooses: the mic state and level; the words being heard, greyed when not addressed to Eileen; whether the conversation window is open; what Eileen is saying; each job with route, name and progress; pending approvals with Allow and Deny; what Eileen is watching (the app in focus, a live thumbnail of a background window a job works in); a countdown while a job holds the screen, and "you took over, I stopped" when the user touches the mouse or keyboard during it; learning, recording and off-the-record indicators; plan usage when it is high. Before Eileen clicks or types on the user's screen, the HUD outlines the target for 400 ms and names it, so "no, the other one" works mid-action. Before a hard-to-undo edit it shows the frame or crop and asks. A small settings panel and the first-run choices are clickable. The HUD excludes itself from screen captures and ignores synthetic clicks on its own buttons.

### E18. First run, with a surprise path

| You do | What happens | Under the hood |
|---|---|---|
| Finish `eileen setup` and start Eileen | The HUD strip offers three choices: "Take the tour", "Skip", and "Show me something cool". Under the third is an unticked box: "Let Eileen use my voice to personalise how she sounds and listens. You can delete it any time." | first-run state machine, 2.8 |
| Pick "Show me something cool" with the box ticked | "Say anything you like for about half a minute. Tell me about your day." Nothing hints at what comes next. The recording also sets the mic level, tunes echo cancellation, learns how the user says "Eileen" and, if adopted, enrols the speaker check | 2.8 |
| (a short "getting ready" bar while the scripted lines render) | Eileen, speaking in the user's own cloned voice: what is on screen; opens an app and plays a bundled clip; answers a question; starts two small helpers in parallel; repeats a request to show it is faster the second time | 2.8, E1 to E5 |
| (the reveal) | "That was your voice, by the way. Keep it, pick another voice, or go back to the default?" If it is not kept, the clip, the clone and every cache are deleted at once | 2.8 |
| The box not ticked, or the PC too slow to clone in time | The same demo in the default voice, or the normal tour, and "Show me something cool" stays in the tray menu for later | 2.8 |

### Roadmap spine: what moves

ROADMAP.md keeps its milestones M1 to M6 as the spine, adding an M0 for the foundation WP0 builds. The October direction changes their content and pulls some later items into v0.1:

| Item | Was | Now | Why |
|---|---|---|---|
| HUD with mute | v0.2 | v0.1: mute key and tray in M3 with slice S1 (WP2), the overlay in M3 (WP8) | The owner asked for mute one click away and a visual layer as part of the feel; the prior-art report lists a status surface as table stakes |
| Resolve hook | v0.2 | v0.1, in M2 | The owner's first benchmark is a Resolve request; the Lua bridge already runs under a Rust supervisor on the owner's PC |
| Learning loop | not planned | v0.1, in M4 | The owner's main point (2026-10-20) |
| Dictation | not planned | v0.1, in M3 | Owner direction 2026-10-20 |
| Voice cloning with consent, engine per voice (Pocket TTS, Chatterbox) | not planned | v0.1, in M3 | Owner direction 2026-10-20, including the Chatterbox head-to-head |
| First-run surprise path | not planned | v0.1, in M3 and M6 | Owner direction 2026-10-20 |
| Settings by voice, rename | not planned | v0.1, in M4 | Owner direction 2026-10-20 |
| Community scenario corpus and test fleet | v0.3 evaluation loop | v0.1: capture and sharing with slice S1, the fleet in M6 | The owner's friends test on their own PCs now |
| Claude plugin (handoff, display mod) | not planned | v0.1, opt-in, in M4 | Owner direction on Claude's extension points |
| Phone channels (Telegram, ntfy) | v0.1 M5 | v0.2, as the first outward connector | The owner wants the whole PC first, then reach outward through connectors |
| Codex, Grok, DeepSeek routes | v0.1 M4 (Codex) | slots in v0.1, built in v0.2 | Claude is the only v0.1 route; Codex is not installed |
| Gemini | v0.2 | removed | No Gemini key; no permitted subscription route |
| Linux | non-goal | v0.2 target; neutral crates tested on Linux from day one | Owner direction "Windows first, not Windows only" |

### Proposed ideas: adopted now, scheduled later

From the "Proposed 2026-10-20" section of OWNER-DIRECTION.md:

| Idea | v0.1 | Later |
|---|---|---|
| Undo for everything | Adopted for what Eileen fully controls: window moves, volume, dictation insertions, settings, skills (rollback), job worktrees and in-place edits (snapshot refs), Resolve edits (color versions). Each action is journaled with its inverse; "undo that" reverses the last one or says honestly that it cannot | App-level undo inside other pro apps (v0.2) |
| Visible intent | Adopted: HUD outline and label 400 ms before any click or keystroke on the user's screen; frame or crop confirmation before hard-to-undo edits | "Show me first" whole-plan preview (v0.2) |
| Teach by showing | Scheduled | v0.2: "watch me do this" through Eileen's own hooks, same consent rules |
| Discoverability | Adopted: "what can you do here?" by live introspection; two or three HUD suggestions from the same data; the first-run tour and surprise path | Richer tour (v0.2) |
| Focus awareness | Adopted: quiet during calls, presentations and full-screen apps, announcements held and summarised afterwards; other media paused during approval questions | |
| Plan-limit awareness | Adopted: rate-limit events drive a HUD chip, warnings before fan-out, routing small things to the fast model, and caps on the librarian by share of the plan | |
| Accessibility | Adopted as a design rule: every task works with no mouse; speech rate and HUD scale are settings; everything on the HUD is also available by voice and from `eileen status` | Screen-reader exposure of the HUD (v0.2) |
| Memory you can ask about | Adopted minimally: the episodic ledger records app, window title, document identity and focus time for 14 days, locally, searchable by the brain; an off-the-record mode records nothing | Page-content recall (v0.2) |
| Routines | Scheduled | v0.2 |
| Shared skills, opt-in | Scheduled; v0.1 lays the ground by turning trusted skills into local regression scenarios and offering to share those as tests | v0.3: signed packs, untrusted until verified locally |
| Languages | Partly: multilingual dictation (Parakeet v3, on demand); typed requests in any language go to the brain | Spoken conversation in other languages (owner decision, section 7) |
| One-step install | Partly: `eileen setup` downloads and verifies models, checks the Claude CLI sign-in without touching credentials, creates the Start menu and sign-in entries and says hello; a nightly prebuilt zip if the owner allows it (section 7) | winget and a signed MSI (v0.2, needs a signing certificate) |

## 2. The architecture as it will be built now

### 2.1 Processes

```
 mic, speakers, system loopback      hotkeys, tray, Raw Input             HUD (eileen-hud.exe)
      |                                  |                                      ^  JSON lines (stdio)
      v                                  v                                      |
+--------------------------------------------------------------------------------------------------+
| eileen.exe (core, always on; links nothing that can hang)                                         |
|  audio thread: WASAPI capture, render and loopback, AEC (sonora), ring buffer, mute, ducking      |
|  shell thread: tray, global hotkeys, Raw Input (dictation key, physical activity), toasts         |
|  Core::step (sans-IO): turns, addressee, intent resolver, jobs, router, permission gate,          |
|    approvals, announcements, barge-in, journal, admission, skill engine (ladder, replay)           |
|  drivers (tokio): store (SQLite), bus, loopback HTTP (hooks, state, control), dispatcher,         |
|    supervisor, learning store and librarian scheduler, settings, flight recorder                  |
+--------------------------------------------------------------------------------------------------+
   | named pipe (frames)    | MCP stdio          | MCP stdio          | stream-json stdio      ^ named pipe
   v                        v                    v                    v                        | (per-session token,
 eileen-speech.exe     eileen-desktop.exe    eileen-apps.exe     claude -p (brain, spare,   eileen-shim.exe   checked PID)
 VAD, ASR, end of      UIA, capture, input,  SMTC, Office COM,   pooled and job workers,    (one per Claude
 turn, TTS, voices,    app index, windows,   Explorer, recents,  librarian, scribe)         session; serves the
 speaker check,        DesktopState,         Resolve Lua bridge  |                          `eileen` MCP tools)
 text embedding        text insertion        (fuscript.exe child) +-- HTTP hooks to the loopback endpoint
 eileen-guard.exe (outside the Job Object): minidumps of crashed or hung processes, restarts a hung core
```

| Process | Contains | What a failure costs | Transport to the core |
|---|---|---|---|
| `eileen.exe` | Everything that decides, plus audio I/O, the ring buffer, the tray, hotkeys and Raw Input | Everything, so it links no UIA, no app COM, no ONNX Runtime and no web view; `panic = "unwind"` so a panicking task fails one job, not the process | n/a |
| `eileen-speech.exe` | sherpa-onnx and ort sessions: Silero VAD, Nemotron streaming ASR, Smart Turn, Kokoro, Pocket TTS, Chatterbox when selected, speaker embedding, the intent embedding model if adopted | Recognition pauses; the core keeps capturing into its ring buffer and replays it after the restart; the HUD says "speech restarting" | Binary frames over a named pipe (section 3.3.7) |
| `eileen-desktop.exe` | UI Automation on its own MTA thread with deadlines, window management and app launch on a separate thread, capture, input, the app index, DesktopState, text insertion | Grounding pauses; computer-use calls fail with a typed error and the job falls back or waits | MCP over stdio |
| `eileen-apps.exe` | One STA thread per COM target, the SMTC manager, the recent-work index, the Resolve bridge as its child, YuNet and the sharpness metric | App hooks pause; desktop tools still work | MCP over stdio |
| `eileen-hud.exe` | The overlay, the clickable strip, the live thumbnail | The overlay disappears and restarts; listening and work continue | JSON lines over stdio |
| `claude -p` sessions | Brain (warm), hot spare, pooled worker sessions, one per bound job, the librarian, the optional dictation scribe | One job or one brain turn; the brain spare takes over | stream-json over stdio, plus HTTP hooks |
| `eileen-shim.exe` | A small rmcp stdio server that each Claude session spawns from its generated MCP config; it forwards every tool call to the core's dispatcher over a named pipe | One session's tools | Named pipe with a per-session token; the dispatcher checks the caller's process |
| `eileen-guard.exe` | Embark `minidumper` server, core heartbeat watchdog | Crash dumps are not written | Local socket |

Rules (from the rust-architecture and observability reports, with the reviews' corrections):

1. Every child starts through `eileen_supervisor` (process-wrap 10.0.1 with a kill-on-close Job Object, `CREATE_NO_WINDOW` and kill-on-drop). Clippy bans `Command::new` everywhere else. The guard is the one child started outside the Job Object: it is spawned with `CREATE_BREAKAWAY_FROM_JOB` (falling back to a plain spawn when breakaway is refused, which matters only when Eileen itself runs inside someone else's job) and with its own null standard handles, so it survives the core.
2. Every call to another process has a deadline, and every sidecar has a heartbeat. A UIA call that passes its deadline (default 5 s) first gets its thread abandoned: the desktop server starts a fresh MTA thread with a fresh `IUIAutomation` instance and answers the caller with a typed error. Only after three abandoned threads in ten minutes, or a missed heartbeat, is the sidecar hang-dumped by the guard, killed and restarted. One UIA call on this PC blocked for 38.7 minutes, far past UIA's own 20 s timeout, which is why this rule exists; a kill and restart costs 6 to 8 s plus the app index, which is why the thread is abandoned first.
3. Sidecars and Claude sessions are long-lived. Spawning per request costs seconds on this PC (bare Python 4.5 to 6.9 s, claude cold start 11 to 111 s under load), which is why workers come from a warm pool where memory allows (section 2.5).
4. Stdout carries the protocol; stderr carries logs, which the supervisor forwards into the core's tracing.
5. Separate binaries, so an agent working on UIA never links the speech stack and a HUD change relinks only `eileen-hud`.
6. At startup each process opts out of EcoQoS execution-speed throttling (`SetProcessInformation` with `ProcessPowerThrottling`), and audio threads register with MMCSS. Without the opt-out one Kokoro run took 27.8 s instead of 2.4 s.
7. Claude sessions run in Job Objects at `BELOW_NORMAL_PRIORITY_CLASS`, so a worker's build never starves the voice loop, and are started only through memory admission (section 2.5).
8. "Start at sign-in" is an `HKCU\...\Run` entry that `eileen setup` creates on request, never a Task Scheduler task (a task's default 72-hour execution limit would stop an always-on process).

### 2.2 One request, end to end

1. **Audio.** The core's audio thread captures 48 kHz from WASAPI in 10 ms buffers, resamples to 16 kHz mono, and runs sonora's AEC3 with the system loopback capture of the default render endpoint as the far-end reference: that is the mix the speakers actually play, including Eileen's own voice and any video or music, so a YouTube clip playing beside Eileen is cancelled too. Eileen's own PCM stays as the fallback reference when loopback is unavailable. When the loopback signal has been silent for 500 ms, AEC3 is bypassed (noise suppression and the high-pass filter keep running), which keeps idle cost low. The audio thread also derives "other audio is playing" from loopback energy that Eileen's own stream does not explain. Frames of 32 ms (512 samples) go to the ring buffer and the speech pipe. Muted means the capture stream is stopped, not that samples are discarded.
2. **Speech.** `eileen-speech` runs Silero VAD on every frame (one thread, 300 ms pre-roll). While VAD reports speech it feeds Nemotron and streams partials. On 200 ms of silence it runs Smart Turn on the last 8 s and fuses the result with transcript continuation cues and a silence cap (1.2 s, stretched to 2.5 s when the partial ends on a continuation word) into an end-of-turn event carrying the final transcript and the recognizer's confidence. The speech process mints the `turn_` ID at the first speech onset after the previous end of turn.
3. **Addressee gate** (core, local, no model), section 2.8: the assistant's name; an open question or approval; the conversation window (8 s after Eileen speaks, 45 s while the user is engaged), suspended while other audio plays or another app uses the mic; a full match in the command grammar; and, when the user is alone, a local score for unnamed requests about the screen, with the brain allowed to answer "not for me". Anything else is shown greyed on the HUD for a few seconds, never stored, and "that was for you" runs it.
4. **Intent resolver** (`eileen-intent`, section 3.3.10), layered so it stays instant where it can:

| Layer | What it does | Latency budget | When it answers |
|---|---|---|---|
| L1 grammar and learned templates | Deterministic grammar for Eileen's own verbs (safety and control phrases first, then the built-in grammar, then learned skill templates) with typed slots | under 1 ms (measured matcher: 4 to 6 us median, under 1 ms p99 at 5,000 skills) | A full match in which every content word is accounted for and every slot resolves unambiguously |
| L2 local semantic matcher | BM25 over skill names, descriptions and triggers, plus cosine similarity from a small local sentence-embedding model run in the speech process, if WP4 adopts it | under 25 ms including the embedding round trip | Hints into the brain's context; a confirmed phrasing runs directly only when its target has no slots |
| L3 brain | The warm Claude session with live context (focused app, its tree summary, selection, jobs, user facts, L2 hints) and the `act` tools | first text 1.0 to 1.5 s quiet, 2.2 to 4.2 s busy | Everything else |
| L4 worker | When no capability exists yet, the brain dispatches a worker that figures it out; the learning loop turns the result into a skill that feeds L1 and L2 | seconds to minutes | New capabilities |

   Unclear requests get one short clarifying question, never an error: the resolver returns `Ambiguous` with the options, the core asks, and the answer resolves through the open question. "What can you do here?" is answered by introspection (section 3.3.10), not from a static list.
5. **Fast path** (L1 hit): the core emits the action as a tool call through the same dispatcher and permission gate a worker uses, and replies from the phrase cache.
6. **Brain** (L3): an earcon within 150 ms; the core sends one stream-json user message with the context blocks; text streams back, is cut at clause boundaries and spoken; tool calls run deterministic actions (`act`) or start workers (`jobs_dispatch`).
7. **Workers** run in parallel as jobs, bound to a pooled session when one is free. Their narration, approvals and completions come back as events; the announcement policy decides when anything is spoken.
8. **Learning**: every tool call from every origin is recorded at the dispatcher; when a job ends, a 120 s regret window listens for "no", "undo that", "that's wrong", a retry or praise, attributed only when it is clear which job the words are about (section 2.10).

### 2.3 The brain

A warm `claude -p` process on the owner's plan, started when Eileen starts, with a hot spare started once the first is ready (`brain.hot_spare = true`; dropped automatically under memory pressure, section 2.5). Flags, from the subscription-harnesses, brain-latency, claude-mods and learning-loop reports and the reviews:

```
claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages
  --model <brain.fast model id> --effort low
  --system-prompt <brain prompt, under 1,500 tokens>
  --tools "" --allowedTools "mcp__eileen__*"
  --strict-mcp-config --mcp-config <run\brain\mcp.json>       (only the eileen shim, toolsets brain and act)
  --setting-sources "" --settings <run\brain\claude-settings.json>
  --no-session-persistence --no-chrome [--restricted]         (--restricted if WP3's test passes; the brain has no built-in tools anyway)
environment: constructed allowlist (section 2.12); ENABLE_CLAUDEAI_MCP_SERVERS=false; CLAUDE_CODE_DISABLE_AUTO_MEMORY=1
settings file: {"alwaysThinkingEnabled": false, "disableSkillShellExecution": true}
```

Never `--bare` (it drops subscription sign-in) and never `--safe-mode` (it suppressed `--mcp-config` servers in tests). `--no-chrome` keeps the account-level Claude in Chrome connection away from Eileen's sessions unless the user opts a worker in. Connectors are off: with claude.ai connectors and user MCP servers loading, warm turns were two to four times slower. The model id is a full id, never an alias, because aliases drifted between CLI builds; WP3's first task is to read the resolved ids for `haiku`, `sonnet` and `opus` from `system/init` once and write them into the default config. Every flag Eileen passes is listed in one constant in `eileen-harness`, which the nightly CLI drift job checks against `claude --help` of the pinned and the latest CLI (section 4.6).

**Thinking off, measured.** Thinking must be off for the brain. WP3 tries, in order, `alwaysThinkingEnabled: false` in the settings file, `MAX_THINKING_TOKENS=0` in the environment, and `--effort low` alone, records time to first text for each with the resolved model id beside every number, and keeps the first that works. The brain-latency report measured the `haiku` alias (which resolves to Haiku 5.5 on 2.1.304) at 1.0 to 1.5 s to first text on a quiet run, so the earlier worry of about 10 s of thinking before the first word stands only if WP3 reproduces it. Decision rule: if no Claude model reaches first text in about 2 s on a quiet PC, `eileen doctor` says so and the brain stays on the fastest one measured; Haiku 4.5 is a stopgap only, since it is deprecated.

**Turn input.** One user message per turn, built by the core, with blocks appended after the cached prefix so prompt caching keeps working:

```
[assistant] name Nova; personality "dry, brief"; verbosity one_sentence; humor 2 of 5      (settings, so changes apply next turn)
[heard]     what the user said (final transcript); after a barge-in, [interrupted] and the exact prefix they heard;
            [addressee: unsure] when the local gate passed it on a score
[desktop]   monitor 1 (1920x1200, 144 dpi): DaVinci Resolve "Beach Edit - Color" (foreground)        <- untrusted text is fenced
            focused: none; selection: none; recent: window Resolve, job beach-face
[jobs]      * beach-face  computer use  Claude  running 2m  "adding a sharpen mask"   (focus)
              login-tests coding        Claude  done 1m ago "3 of 3 pass"
[hints]     known skills that may apply: resolve-sharpen-face (trusted, 4 uses)
[facts]     "the video editor" means DaVinci Resolve; Resolve timelines start at 01:00:00:00, 24 fps
[context]   2026-10-21 14:02, Tuesday; location: Austin (set by the user); depth: auto; plan: 41 percent of 5 h window
```

Caps: windows 12, selection 300 characters, jobs 8, hints 3, facts 8, ledger lines 6; the whole block stays under about 1,200 tokens (600 for desktop and jobs, 600 for learning). Window titles, selected text and anything else read from the screen or a page sit inside a fenced, labelled data block the prompt tells the brain never to follow as instructions.

**Turn authority.** A turn that began with addressed user input (the microphone after the addressee gate, typing in Eileen's own UI, the HUD) gets the brain's full toolset. A turn that began from a job event, an announcement, the `eileen say` command line or a Claude session gets read-only tools only (`jobs_status`, `desktop_state`, `hud_show`, `learned_query`); the dispatcher refuses the rest with a message the brain can relay. So text that reaches the brain through a job result or a window title can never start a job, change a setting or act.

**Turn output.** The brain prompt asks for spoken style: at most two sentences (or what `assistant.verbosity` says), then optionally a line `---hud---` followed by Markdown for the screen. The streamer speaks everything before the marker clause by clause and sends the rest to the HUD. If the brain decides the utterance was not meant for Eileen (it was passed on with `[addressee: unsure]`), it answers only `---ignore---`, nothing is spoken and the HUD greys the line. Actions are tool calls on the `eileen` MCP server, toolsets `brain` and `act` (section 3.3.9).

**The brain can act, but never with the mouse.** The `act` toolset holds the deterministic tier-0 and tier-1 tools that never move the mouse or type: `desktop_launch_app`, `desktop_window`, `desktop_volume`, `apps_open_url`, `apps_open_uri`, `apps_open_path`, `apps_media`, `apps_recent`, and the navigation ops of `apps_resolve` (hello, timeline information, projects and timelines, go to, open a page, markers, render status). They go through the same dispatcher, permission gate and recorder as everything else. This is how "get Premiere up for me" or the first steps of the Resolve benchmark run in about 3 s instead of waiting for a worker. When a brain-chosen action's regret window closes clean, the core stores the pairing as a phrasing candidate (section 2.10). Clicks, typing, drags, key presses and every edit are worker jobs.

**Depth tiers.** Instant (L1 or cache, no model), quick (the brain alone), considered (a worker on the mid model with light thinking), deep (a worker on the strongest model with thinking and web research). The brain picks by calling `jobs_dispatch` with `kind = "deep_answer"` and a depth; "quick answer" and "think hard about this" are L1 overrides that set the depth before the brain sees the turn.

**Two speeds.** For considered and deep questions the brain answers at once with what it knows and dispatches the deep job in the same turn. The brain prompt forbids stating current facts (opening hours, prices, news) without a source: the fast answer says what it is checking instead. When the deep answer lands: if the user is not speaking and has not moved to an unrelated topic (no new turn for 20 s, or the new turns are about the same job), two sentences are spoken ("Better answer: ...") and the full answer goes to the HUD; otherwise only the HUD changes and a short line waits for a pause. The user never hears both answers in full (simulation invariant 11). By default one deep worker runs several web searches and fetches in parallel within one turn; splitting into several research sessions plus a combiner happens only when memory admission allows it and the brain passes sub-questions.

**Context makes it right.** Location is opt-in and local (`privacy.location`, set by voice: "I'm in Austin"; reading Windows' location service is v0.2). Time comes from the PC; preferences, past choices and which sources gave accepted answers for each kind of question come from the user model; what the user is doing comes from DesktopState. Slow-changing results (opening hours, a restaurant list) are cached with an expiry in `learn.sqlite` and reused for follow-ups ("closer"). Anticipation (looking things up before the user asks) is off by default because it spends plan limits.

**Lifecycle.** The session serves many turns. The core recycles it when the context reported in the `result` usage passes a threshold (default 60,000 tokens), after a learned skill or a phrasing changes, or after a crash; never on idle time. The spare takes over with a summary of at most 300 tokens of the conversation, and a fresh spare starts. A crash, or a turn with no stream event for 30 s, swaps to the spare at once. If the user speaks again while a turn is running, the core sends a `control_request` interrupt and a new turn holding both utterances. Speculative turns on the spare from a stable partial are an opt-in experiment WP3 measures (`brain.speculative = false` by default), because they spend plan and occupy the spare that guards against a crash.

**The brain is swappable; Claude is only the default** (owner direction 2026-10-20). Everything above talks to the `BrainBackend` trait (section 3.3.8), and the conversational brain and the deep workers are configured separately (`routing.brain` and `routing.deep`, by voice or in the file, with "undo that"). In v0.1 only Claude is built; Codex, DeepSeek, Grok and a local model are slots. When a switch to another backend becomes possible, Eileen first runs the brain benchmark on this PC (time to first spoken word, the right action on the resolver golden file, interruption handling, reported beside the default's numbers) and warns if the new brain would be noticeably slower or less reliable; if the chosen brain stalls on a turn, the reply falls back to the default backend, by the same rule that swaps to the spare. Model names are aliases in config resolved to full ids, never hard-coded.

**Plan usage.** The harness parses the stream's rate-limit events into `PlanUsage` bus events. Above 80 percent of the five-hour window the HUD shows a chip, fan-out above two workers asks first, and considered questions drop to quick unless the user asked for depth. Eileen never silently burns the allowance: the librarian has caps by share of the plan (section 2.10), and the nightly lanes have their own turn budget.

### 2.4 The deterministic fast path

L1 of the resolver. The grammar lives in `eileen-intent` as data: a table of patterns with typed slots, each producing a tool call or a core command. Utterances are normalised (lower case, fillers dropped, spoken numbers and times parsed by the slot resolvers `Offset`, `Count`, `Text`, `App`, `File`, `Url`, `Media`, `Position`, `Choice`, `Monitor`, `Voice`, `Name`, `Depth`). A slot resolver returns `Ambiguous` instead of guessing: a bare "go to 15" with no unit is "15 seconds, 15 minutes or 15:00?", never silently 15:00.

Precedence, first match wins:

1. **Safety and control**, which nothing learned can shadow: mute, unmute, stop, cancel (a job by name, ordinal or "that"), "go on" and "continue", "that was for you", "undo that", "forget that", "something went wrong", "save that as a test", "stop recording", "off the record", "pause the agents".
2. **Built-in grammar**: pause, play, next, seek ("go to 3:20" when media or Resolve has focus), "play <known media> [on YouTube] [at <time>]" (resolved through the media history and the recent-work index), open app, close app (tier 2 when unsaved state is unknown), move or snap a window to a side or a monitor, volume, "open the file I was working on", "what time is it", "how's it going" (reads the jobs block), "what have you learned", "what do you know about me", "learn from that now", "record this session", "use <voice>", "back to normal", "call you <name>", "take dictation", "stop dictating", "quick answer", "think hard about this", "show me how you do that", "what can you do here", "show me agent <n>'s changes".
3. **Learned templates** at level 4 (section 2.10).

A full match must account for every content word: each non-filler token is a literal, a slot, or a known descriptor that maps to a slot. "Go to 15:30 and make the face of the girl on the right less blurry" is therefore not a fast-path command (it goes to the brain, which can act on the first half and dispatch the rest), and an unknown descriptor ("the girl", "red shirt") turns a learned-template hit into a hint. Replies come from the phrase cache: stock phrases synthesized once per voice into PCM, plus templates whose variable part is one short clause ("Opening <app>."), with the launch phrase of every indexed app rendered in the background after indexing.

### 2.5 Workers, routing and jobs

**The driver trait.** Every worker route implements `WorkerDriver` (section 3.3.8). v0.1 implements the Claude driver only. The Codex app-server driver and the generic ACP driver (for Grok's `grok agent stdio` and others) are designed slots: the trait, the mapping table and the configuration keys exist, and constructing them returns `RouteUnavailable` until v0.2. DeepSeek is designed as the Claude driver pointed at DeepSeek's Anthropic-compatible endpoint with its own `CLAUDE_CONFIG_DIR` and the one permitted API key from the system keyring, text only unless the user allows images; it is off in v0.1. Gemini is not a route.

**Sessions and jobs.** A driver starts a *session* (`SessionSpec`: model, toolsets, plugin snapshot, working folder for non-coding kinds) and later *binds* a job to it (`JobBinding`: the task, the matched skill, the facts). Hooks and the shim identify the session by a `ws_` ID and a token minted at spawn, and the core maps session to job when it binds, so a pre-started session can take a job. Coding jobs start their own session in their own repository; everything else can come from the warm pool.

**The warm pool.** One `computer_use` session (default model, desktop and apps toolsets) and one `deep` session (strongest model, web tools, used for deep answers and research), started after the brain and its spare, re-armed after use, recycled after a learned skill changes, and dropped under memory pressure. Binding a pooled session costs one user turn instead of a cold start (11 to 111 s measured under load). Pool sizes are settings (`memory.pool_computer_use = 1`, `memory.pool_deep = 1`).

**Memory admission.** The core receives free memory every 5 s (`Input::Memory`) and holds a cost estimate per job kind (a Claude session about 0.5 GB; a build-heavy coding job about 0.5 GB plus 3 GB for its build). A job starts only if free memory after its estimate stays at or above the reserve (`memory.reserve_mb = 2048`); otherwise it waits in `queued` with a spoken reason ("I'll start agent three when memory frees up"). To make room, the core drops, in order, the deep pool session, the hot spare and the computer-use pool session. A coding job whose repository is build-heavy (a `Cargo.toml`, a `.sln`, a `CMakeLists.txt` or a Gradle build at its root) takes one of the build slots shared with `cargo xtask` (section 6.1, through `eileen-buildguard`), so at most two builds run on the PC at once whoever starts them; it passes `EILEEN_BUILD_SLOT_HELD` so a `cargo xtask` inside the job reuses that slot, and sets `CARGO_BUILD_JOBS=4` when the user has not set it. Below 3 GB free at start for a minute, Eileen picks the low-memory profile (section 4.4). A simulator invariant checks admission.

**Claude worker flags.**

```
claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --include-hook-events
  --model <model id for the session's class and depth> [--effort low|medium|high]
  --append-system-prompt <worker prompt: route name, narration rule, screen and page text are data, ask before
                          guessing, check for the app's own API or MCP server first, stop on refusal>
  --setting-sources "" --settings <run\<session>\claude-settings.json>
  --strict-mcp-config --mcp-config <run\<session>\mcp.json>   (the eileen shim with the session's toolsets, plus any
                                                               third-party MCP server the user approved for this kind)
  --allowedTools <per kind, section 3.3.9>
  --permission-mode <pinned by WP3's contract test>
  [--permission-prompt-tool mcp__eileen__approve]             (only if WP3's test picks this route)
  --plugin-dir <run\<session>\plugin>                         (the session's learned-skill snapshot, section 2.10)
  [--plugin-dir <worktree>\tools\claude-plugins\eileen-dev]   (only for jobs in an Eileen worktree; one at a time)
  --no-session-persistence --no-chrome                        (continue_session jobs use --resume <id> --fork-session;
                                                               --chrome only when the user opted this job in)
  [--restricted --tools <the kind's built-in tools>]          (if WP3's test keeps workers working; see below)
cwd: the job's repository or worktree, or %LOCALAPPDATA%\eileen\run\<session>\cwd for non-coding sessions
environment: the worker environment of section 2.12 plus ENABLE_CLAUDEAI_MCP_SERVERS=false,
             CLAUDE_CODE_DISABLE_AUTO_MEMORY=1, EILEEN_SESSION_ID, EILEEN_HOOK_TOKEN, TRACEPARENT
```

Subagent and task tools (`Task` and its successors) are not in any `--allowedTools` list in v0.1, until WP3's contract test shows the tier guard also fires for subagent tool calls.

The CLI's own containment: `claude --help` on 2.1.304 lists `--restricted`, which removes the code-running tools (Bash, PowerShell and others) and `WebFetch` unless `--tools` names them, ignores user, project and local settings files (`--settings` still applies), confines the file tools to the working folders (`--add-dir` included), refuses `bypassPermissions`, and lets only a person or the configured permission handler approve writes to settings, git and tool-configuration files. WP3 tests it for every session kind (with `--tools` naming exactly the kind's built-in tools, and `--add-dir` for toolchain folders coding jobs read); if it keeps workers working, it becomes the default for every session Eileen starts, as a second fence behind Eileen's own path guard. `--permission-prompt-tool` is no longer listed as a flag on 2.1.304, though `--permission-prompts host` still names it; `--session-id` lets Eileen choose the Claude session id at spawn, which WP3 uses to tie the CLI's own session to the `ws_` id.

The per-session settings file holds `disableSkillShellExecution: true` and the HTTP hooks of the claude-mods report: `PreToolUse` to the tier guard with `onFailure: "block"` and a 10 s timeout, `PermissionRequest` to the approval endpoint (section "Approvals" below), plus `PermissionDenied`, `PostToolUseFailure`, `Notification`, `SubagentStart`, `SubagentStop`, `Stop` and `StopFailure` to the loopback bus, each with an explicit timeout (the HTTP default is 600 s). The bearer token and session ID travel in the worker's environment (`EILEEN_HOOK_TOKEN`, `EILEEN_SESSION_ID`) and are interpolated through `allowedEnvVars`, so they never sit in the file. The guard answers no decision for tiers 0 and 1, `ask` for tier 2 and `deny` for tier 3; it never answers `allow`, and with `onFailure: "block"` a dead Eileen stops risky work instead of waving it through.

**Narration.** The worker prompt asks for one line starting with `> ` before each significant step. The harness turns those lines into `JobNarration` events; the core speaks at most one line every 20 s, only for the focus job, only when the user is not speaking and not in a call, only lines of 12 words or fewer, and always attributed ("Claude: ..."). Lines that ask for an approval or a credential are dropped and logged. Everything else goes to the HUD.

**Routing.** Policy is TOML (section 2.11): each job kind maps to a route, a model id and a depth. v0.1 defaults: every kind goes to Claude; quick uses the fast model, considered the mid model, deep the strongest. Spoken overrides beat policy at once ("use the strongest model for this", "send this to a stronger model", later "send this to Codex") and are recorded on the job with the words that caused them. "Why did you do that?" is answered from the recorded `router.decide` reason.

**Fan-out.** `jobs_dispatch` takes `fanout` (1 to 8) and `mode` (`compete`: every worker gets the whole task; `split`: the brain supplies one sub-task per worker). Children get ordinals in accept order, so "agent two" resolves. For coding jobs each child gets its own git worktree under `%LOCALAPPDATA%\eileen\worktrees\<repo-hash>\<job-slug>` on a branch `eileen/<job-slug>` created from the user's current branch. The repo comes from the utterance, the focused editor's folder, or a `project` fact; if none fits, Eileen asks once and remembers. "Show me agent two's changes" puts the diffstat and test results on the HUD; a compete run ends with one spoken sentence on how the results differ. Optionally a Windows Terminal pane per worker runs `eileen jobs tail <id>` (tier 1). Merging a worker branch into the user's branch is local and tier 1; removing a worktree that holds unmerged commits, and any push, are tier 2.

**One coding job edits in place.** A single coding job works in the user's checkout, after recording a snapshot (`git stash create` plus the current HEAD, kept as a ref under `refs/eileen/`) so "undo that" restores exactly what was there. It uses a worktree instead when the user's uncommitted files overlap the files the task names, or when several agents run.

**Jobs.** The registry is SQLite (`eileen.db`, section 2.15). States: `queued`, `running`, `blocked`, `awaiting_approval`, `completed`, `failed`, `cancelled`, `interrupted` (after a core restart). Every job ends in exactly one terminal state. Names are two to four words generated at accept time and never regenerated after the user has heard them; slugs are collision-checked against seven days of jobs. Pronoun resolution, first match wins: an explicit name, slug or ordinal; a kind word ("the render"); a route word; the focus job; the most recent; two remaining candidates mean one question; "it" right after an announcement means the announced job.

**App tasks.** `JobKind::AppTask` is a job the core runs with no Claude session: a Resolve render the user started by voice, or one Eileen notices already running (the bridge's `is_rendering`, polled every 2 s while true). Progress arrives as connector events and becomes `JobProgressed` with a percent, so "how's the render?" and "the render is at 60 percent" work, and "stop announcing progress on renders" has a row to change.

**General jobs.** `JobKind::General` is for tasks that mix files and apps (download, unzip, rename, then attach): a computer-use session that also gets `Bash`, `PowerShell`, `Read`, `Edit` and `Write` under the path guard and the shell policy of section 2.12. The brain picks it when a task needs both.

**Announcements.**

| Event | At the PC | In a call, presentation or full-screen app |
|---|---|---|
| `ApprovalRequested` | Interrupt: chime, other media paused, one sentence, interruptible | HUD only, no chime; waits |
| `JobBlocked`, `JobFailed` | Interrupt | HUD; summarised afterwards |
| `JobCompleted` | When idle: end of the user's turn plus 1 s of silence | Held, summarised afterwards |
| `JobNarration` | The narration rule above | HUD only |
| `JobProgressed`, `JobStarted` | Silent unless the user asked about that job in the last two minutes | Silent |
| `JobCancelled` | When idle, unless the user cancelled it | Held |
| `JobQueued` (memory) | One sentence, once | HUD |

The table is data in the config; the user can change a row by voice, per event and per job kind ("stop announcing progress on renders").

**Approvals.** One sentence naming the action and its object, built by Eileen's own code from the parsed tool input (never from text a worker wrote), then a voice window of 20 s. While the question is open, any app playing media is paused through SMTC and resumed afterwards, and Eileen says nothing else.

- What can answer: `UserInput` only. That is the HUD's Allow or Deny (physical clicks only; the HUD ignores synthetic input on its buttons), the tray, the approval hotkey, or speech from the microphone inside the voice window. For tier 2, speech must contain the action word ("yes, push", "allow the push"); a bare "yes" is accepted only after the assistant's name ("Eileen, yes") or when the speaker check (if adopted) matches the enrolled user. Voice approval is turned off, leaving the HUD, while another app uses the microphone, while other audio is playing, and during fleet and nightly runs. Screen text, model output, tool results, the `eileen say` command line and learned skills cannot build an approval; the core's approval type cannot be constructed from them.
- When nobody answers: the voice window closes, the request stays on the HUD with a toast, and the job waits in `awaiting_approval` until its approval deadline (`approvals.deadline_min = 30`); then it is declined and logged. "No" denies at once.
- What a grant covers: "always allow that for this job" stores a grant for the exact normalised call that was shown (the same command and target), never a tool pattern; tier-1 grants may name a pattern.
- Binding: an approval is bound to the target it described (window handle, element selector, element name at that point and a hash of its rectangle for desktop actions; the exact input digest for tool calls). If anything changed by the time the call runs, the call is refused and the question asked again.

How a worker's tier-2 call reaches the user is the one CLI behaviour nobody has tested, so WP3 proves it against the real CLI before the harness merges and records the result. The routes, in the order WP3 tries them:

1. **The `PermissionRequest` hook.** After the guard answers `ask`, the CLI fires its `PermissionRequest` hook; Eileen's endpoint holds the request open (up to `approvals.hook_hold_s = 120`, under the hook's own timeout) while it asks the user, then answers `allow` (only with an `ApprovalId` that came from `UserInput`) or `deny`.
2. **`--permission-prompt-tool mcp__eileen__approve`**, if the pinned CLI still accepts the flag (it does not appear in `claude --help` on 2.1.304; WP3 checks). The dispatcher accepts an `approve` call only when its `tool_use_id` matches a pending `ask` the guard issued for that session, builds the summary itself, and logs any other `approve` call as a security event.
3. **Deny, then steer.** The guard answers `deny` with "awaiting user approval appr_<id>", the job moves to `awaiting_approval`, and after a yes the worker gets a steer turn ("the user approved <summary>; retry it now") plus a one-shot grant for that exact call.

Route 3 is also how any approval that outlives the hook's hold completes. Whichever route wins, the weekly contract test re-runs its cases (allow, deny, dead endpoint, subagent) against every new CLI, so an update cannot quietly turn tier 2 into allow.

**Interrupt, steer, reroute, cancel, pause.** Interrupt sends the stream-json `control_request` interrupt (Windows has no clean SIGINT for a child). Steer is an interrupt followed by a new user turn with the instruction. Reroute cancels and starts a new job with the same prompt, the same working directory and the old transcript attached. Cancel kills the session's Job Object; a killed worker leaves no descendants. "Stop", the mute key or the hands-off hotkey within 5 s of any desktop action pauses every job that acts on the desktop: each gets an interrupt and the input lock is refused until the user says "continue"; the HUD shows them as paused.

**Route discovery and third-party MCP servers.** The worker prompt asks a computer-use worker, before driving an app it does not know, to check whether the app has an official API or MCP server (the MCP registry through `WebFetch`) and to propose it with `routes_propose` rather than install anything. Adding one is a tier-2 settings change (`mcp.servers.<app>`, by voice or `eileen apps add-mcp`); any account sign-in is the user's. Approved servers are merged into the generated MCP config of the job kinds the user chose; their calls pass the same `PreToolUse` guard (unknown third-party tools are tier 2 until the user relaxes one). Each recorded step names the rung it used (app API, app MCP server, deep link, UI Automation, page structure, vision), and the learning loop keeps per-app statistics of which rung worked. Windows App Actions are a v0.2 rung (section 7).

### 2.6 Computer use: Eileen's own desktop server

Decision D21 replaces Cua Driver with Eileen's own Rust server, ported from the proof in the computer-use report (1,720 lines of library code plus a 389-line server, faster than Cua 0.37.0 at grounding, with safety refusals Cua lacks). Workers reach it through the `eileen` shim and the core's dispatcher, so the input lock, the permission gate and the learning recorder sit in one place.

**Tools** (snake_case, valid under Anthropic's tool-name rules; exposed to Claude as `mcp__eileen__<name>`; schemas in section 3.3.9): `desktop_state`, `desktop_windows`, `desktop_tree`, `desktop_screenshot`, `desktop_look` (the brain's one-image variant), `desktop_click_element`, `desktop_set_value`, `desktop_type_text`, `desktop_press_keys`, `desktop_click`, `desktop_scroll`, `desktop_drag`, `desktop_window`, `desktop_launch_app`, `desktop_wait_for`, `desktop_selection`, `desktop_insert_text`, `desktop_volume`. The worker-visible list stays under about 15 KB.

**Action order** for one action: a UIA pattern on an indexed element (Invoke, Toggle, SelectionItem, ExpandCollapse, Value, RangeValue, Scroll), in the background; for text, `ValuePattern.SetValue` when replacing the whole value is right, else the clipboard transaction plus Ctrl+V, else Unicode `SendInput` paced at 30 ms per character, always with a read-back (unpaced synthetic typing into Windows 11 Notepad arrived garbled); coordinates, key combos, drags and the wheel through `SendInput`, which needs the foreground; vision last (a screenshot with set-of-mark indices, the model picks, then a coordinate action with a post-condition). Every action returns a verification hint (value read back, title change, new focused element), which the learning loop uses as the step check. UIA Invoke on a classic Win32 button can activate its window (seen in the proof), so the server checks the foreground before and after every Invoke; if it moved while the user was active, it restores it at once and treats the call as a foreground action from then on.

**The user's hands always win.** `GetLastInputInfo` also counts synthetic input, so after Eileen's first `SendInput` it measures Eileen, not the user. The desktop server therefore tracks the last *physical* input itself through Raw Input on a message-only window (`RIDEV_INPUTSINK`; injected input arrives with no device handle), timestamps only, and `Input::user_idle_ms` reports physical idle time. Foreground work runs under a lease:

1. A job asks for the screen once per phase, with an estimated duration ("May Claude use the screen for about a minute to fix the face?"). If the user has been physically idle for `desktop.lease_idle_s` (default 5 s) the lease is granted silently; otherwise Eileen asks by voice and on the HUD.
2. While the lease runs the HUD shows a countdown. Before every synthetic event the server re-checks the foreground and stops at once if focus moved.
3. Any physical input during the lease aborts the remaining events within one event, releases the lock and shows "you took over, I stopped". The cursor is restored only if the user did not move it. The job's desktop actions pause until the user has been idle for 5 s and says "continue" (or clicks it on the HUD).
4. When Windows refuses the foreground, Eileen says so and stops; it never loops on workarounds. It restores the previous foreground window and hands the screen back with one short line.

One input lock across all workers, held in the core's dispatcher; background UIA actions on different windows run in parallel. The HUD shows who holds the lock. A signed UIAccess helper is a later decision (it needs a code-signing certificate and a Program Files install).

**Visible intent.** Before any `SendInput` or Invoke on a window the user can see, the desktop server emits a `DesktopIntent` event (target rect and label) and waits `desktop.visible_intent_ms` (default 400, settable to 0). Addressed speech or a stop word ("no", "wait", "stop", "the other one") during the wait cancels the action and releases the lock; room talk does not. For a few seconds after such a cancel, the next utterance goes to the job that was acting as a steer, with the cancelled target and the next candidates from the tree, which is what makes "no, the other one" work.

**DPI and coordinates.** Per-monitor-v2 awareness is the first call in `main` (DPI-unaware code sees 1280x800 on the owner's 1920x1200 display). One coordinate system everywhere: physical pixels in virtual-screen coordinates, negatives allowed. Window rects are DWM extended frame bounds. Every screenshot reply states its mapping (`image 1280x800 covers screen rect x=0 y=0 w=1920 h=1200; screen = origin + image_px * 1.5`). Screenshot replies carry text and image content only, never `structuredContent`, because the claude CLI dropped images when both were present. Multi-monitor and mixed DPI are untested on the owner's PC; the test fleet covers them.

**Browsers and Electron.** Chromium and Electron expose only browser chrome over UIA until a UIA client asks for more, and refuse background input. Eileen never installs a global UIA focus handler that would switch every Chromium and Electron app (Chrome, VS Code, the Claude app) into accessibility mode on this memory-tight PC; for those windows it uses `SetWinEventHook` events and UIA on demand. Before a worker types into a Chromium window, the server asks UIA for the element at the target point once, which tells it whether the target is a password field; WP5 measures what that costs Chrome (private bytes, CPU) and how long web content takes to appear. Page content otherwise goes through screenshots and foreground input in v0.1, or the Playwright MCP extension when the user installs it (section 2.7). `Chrome_WidgetWin_1` also covers the Claude desktop app and VS Code, so Eileen's and Claude's own windows are excluded by process, never by title. Every cross-process window call is bounded (`ShowWindowAsync`, `SendMessageTimeout`): `ShowWindow` on a paged-out Edge blocked for over 60 s.

**Safety rules in the server**, so they apply to every worker, every learned skill and every future connector:

- Never read or write a password field (`IsPassword`) and never type while one has focus. For every coordinate action the server resolves the element at the point (`ElementFromPoint`) and applies the same rules to it. Where the element is unknown (custom-drawn apps, page content UIA does not expose), typing is tier 2 unless the user has allowed desktop input in that app.
- Refuse a coordinate action that lands on another process's window than the target, or on any Eileen or Claude window (`WindowFromPoint`), so a worker can never click Eileen's own Allow button.
- Refuse blocked combos (`Win+L`, `Win+R`, `Win+X`, `Ctrl+Shift+Esc`, `Win+Ctrl+Shift+B`, Ctrl+Alt+Del variants, `Alt+F4` on a window the job did not open). `Shift+Delete` is tier 2.
- Tier by target: any input to a terminal window class, the Run dialog, the registry editor, Task Manager or the security pages of Settings is tier 2 (or, for a terminal, the typed command is tiered like a shell command, section 2.12). Enter, Ctrl+Enter or a send-like click in a mail, chat or social app is tier 2.
- Invoking a control whose name or automation id reads Send, Delete, Buy, Pay, Submit, Publish, Install, Place order, Checkout, Confirm, Transfer, Post, Share, Allow, Authorize, Accept or Uninstall (and their localized forms, from a list WP5 maintains) is tier 2.
- Text a worker types passes the Redactor and a Luhn check first; a hit is tier 3 ("please type that yourself").
- Desktop input in an app is allowed per app: the first time a worker drives an app, Eileen asks once ("May Claude control Resolve?") and stores the answer as a tier-1 grant (`permissions.desktop_apps`).
- Explicit targets only, refusing when a different window has the foreground at send time; never touch Eileen's, Claude's or user-marked private windows; never elevate; third-party components run with telemetry off (`DO_NOT_TRACK=1`); "stop" and mute cancel queued input and release the lock. Tests drive only windows the test created.

**App index.** `shell:AppsFolder` enumeration (222 apps here, 1.9 to 3.1 s idle, up to 41 s under load) at startup and on Start menu or package changes, never per call, persisted to `%LOCALAPPDATA%\eileen\cache\apps.json` so a restart serves the last index at once and refreshes in the background. Launch by AUMID (packaged built-ins such as Notepad are stubs in System32). `desktop_launch_app` returns once the app's main window exists (not a splash), narrating if that takes more than 3 s, so a follow-up such as "snap it left" waits for the real window. Matching is Eileen's own scorer (subsequence, prefix and word-start bonuses; no MPL dependency); it launches only when the top match beats the next by a margin, otherwise it asks. Window management and launch run on their own thread, not the UIA thread, so a hung UIA provider never blocks "open X" or "snap it left".

**DesktopState** answers "what is the user looking at" as one object, served from a cache the event hooks keep current: monitors with Eileen's own stable ordinals (left to right, matched by device path across reboots), visible windows with frame rects, z-order and the executable's file version, the foreground window and process, the focused element (control type, name, automation id, rect, patterns, `is_password`), the selection, the caret, recent references, the presentation state (`SHQueryUserNotificationState`) and whether another app is using the microphone. Fields that timed out or are stale are listed in `degraded`, never reported empty. Text leaving the server goes through `eileen_contracts::desktop::sanitize` (valid UTF-8, bounded, control and bidirectional-override characters removed, wrapped as `Untrusted`).

**The selection and the clipboard.** The selection comes from TextPattern. Only when the utterance refers to something copied ("what I just copied") does the server fall back to the clipboard, by sequence number and never by a synthetic copy, and only for clipboard data that does not carry `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory = 0` or `Clipboard Viewer Ignore` (the formats password managers set); the fallback text passes the Redactor. Clipboard snapshots taken by the insertion transaction never enter the input lane, logs or incidents.

**Dictation insertion** lives here too, behind the same input lock (section 2.8).

### 2.7 App adapters

`eileen-apps.exe` is one resident Rust process: a thread per COM target with call timeouts and an `IMessageFilter` for "call rejected by callee", one SMTC manager held for the process lifetime and fed by events (a fresh manager request per read reached 2.2 s p95 under load, a held one about a microsecond), and the Resolve bridge as a supervised child. Tools: `apps_media`, `apps_open_url`, `apps_open_uri`, `apps_open_path`, `apps_resolve`, `apps_office`, `apps_explorer`, `apps_recent`, `apps_image`, `apps_photoshop` (section 3.3.9). Op-style tools declare a tier, a read-only flag and idempotence per op, so a recipe can use `apps_resolve` `timeline_info` as a check while `apply_drx` stays an action.

**Media and YouTube.** SMTC from Rust: subscribe to `SessionsChanged`, `CurrentSessionChanged`, `PlaybackInfoChanged` and `MediaPropertiesChanged`; control with `TryPause`, `TryPlay` and `TryChangePlaybackPosition`, confirmed by the next event (the status returned with an action is the old one). The live position is derived from `Position`, `LastUpdatedTime` and the playback rate. YouTube: open `https://www.youtube.com/watch?v=<id>&t=<s>s` with `ShellExecuteW` in the default browser (tier 1; measured to land at 15:30 after a preroll ad); for unknown videos, a worker opens the results page in the user's own browser and picks from a screenshot. Never search YouTube headlessly (robots.txt disallows it) and leave yt-dlp out of the default path. After a load or seek, wait for the SMTC session to return with the expected title before reporting success. Every URL Eileen opens and every media title SMTC reports go into the media history of the recent-work index (title, URL, last position, when), which is what "play that timelapse" resolves against. The Playwright MCP extension (Node sidecar, Apache-2.0) is supported when the user installs it, as the DOM rung for workers. Claude in Chrome is used only inside a Claude worker started with `--chrome`, by user opt-in, and never by Eileen talking to its native host.

**Files, folders and deep links.** `apps_open_path` opens a file or folder with its default app or a named one (`ShellExecuteW`, tier 1), which is how "open the file I was working on" ends once `apps_recent` has found it. `apps_open_uri` opens a registered URI from an allowlist (`ms-settings:`, `mailto:` composing only, and app protocols the user has approved), tier 1; any other scheme is refused.

**Resolve.** A Lua bridge (about 200 lines with its JSON encoder, `adapters/resolve/eileen_resolve_bridge.lua`) run by Resolve's own `fuscript.exe -q -l lua`, spawned once and supervised, gated on `Resolve.exe` running (otherwise every call stalls about 4 s). Line protocol: request `<id>\t<method>\t<arg>...`, reply one JSON object per line; parse, never compare strings (Lua key order varies). Methods:

| Group | Methods |
|---|---|
| Readiness and navigation | `hello`, `projects_recent`, `load_project`, `timelines`, `set_timeline`, `timeline_info` (start timecode, start frame, frame rate, drop frame, current page), `open_page`, `goto_tc` (an absolute timecode computed in Rust, drop-frame aware, never the spoken offset), `current_item`, `markers`, `marker_add` |
| Stills and versions | `export_still`, `add_version` (a local color version on the current clip), `load_version`, `versions` |
| Edits | `import_fusion_comp` (a generated `.setting` or `.comp` file), `apply_drx` (with the all-node-layers option on 21.1.1 and later) |
| Render | `render_start` (a named preset, to a new file name), `render_status` (completion percent), `is_rendering` |

After a launch the bridge's `hello` is polled until it answers, and Eileen says "Resolve is starting" if that takes more than 3 s. `SetCurrentTimecode` refuses on the Media and Fusion pages and before the timeline start with a bare `False`, so `goto` opens the Edit page first. Known limits: no API for nodes, OFX, power windows, trackers, Face Refinement's Analyze, play or blade. When the owner moves to Studio 21.1 or later, `ResolveMCP.exe` is offered to workers as an approved MCP server and measured against the bridge before anything else changes. Python fusionscript is a fallback only and is not built in v0.1.

**The benchmark's edit, and how to undo it.** Before any grade or composition change on a clip, Eileen calls `add_version` (`eileen-<job>`) and records the previous version in the journal, so "undo that" is `load_version`. Without a recorded version an edit op is tier 2, not tier 1, because Resolve's API cannot remove what it added. The correction route is chosen by WP6's spike (section 6.8, run in wave W1 on the owner's PC with only the Lua bridge), in this order:

1. **A Fusion composition on the clip.** Eileen generates a small `.setting` file (plain-text Lua tables): an ellipse mask whose centre and size are keyframed from face boxes, feeding a sharpen or unsharp-mask node, and imports it with `ImportFusionComp`. Eileen tracks the face itself: YuNet runs on stills exported every few frames across the shot and the boxes in between are interpolated. Importing a file avoids the live `Comp.Lock()` writes the app-hooks report found may not render. This route is fully deterministic, so it can become a model-free recipe.
2. **A generated DRX grade with a window** at the face box, if the spike shows a DRX can carry a window placed at a given rectangle.
3. **A Claude worker on the Color page**, with screenshots and the input lock, only if neither works. This route cannot become a model-free recipe, so the skill stays at level 3 with a delegated step.

Before any of these runs, the HUD shows the face crop and Eileen asks "This face, on the right?" (`preview_before` on the op). The spike's result, with the per-frame still and tracking costs, is recorded in `docs/wp/WP6-apps.md` before WP0's contracts are relied on by WP6; if it needs different ops, WP6 amends section 3.3.9.

**Face and sharpness.** `apps_image` runs YuNet (MIT, OpenCV Zoo, a small ONNX model) through `ort` against the shipped `onnxruntime.dll` to find faces in a still, picks by position (left, right, center, largest), returns a crop for confirmation, and measures sharpness as the variance of the Laplacian inside a box.

**Office and Explorer.** One late-bound `IDispatch` helper (about 120 lines, from the app-hooks probe) serves Word `Selection.Text`, Excel `ActiveCell`, Photoshop and `Shell.Application`. Attach through the running object table with `CLSCTX_LOCAL_SERVER`; bind to the foreground window's document through `AccessibleObjectFromWindow` when several instances exist. Explorer selection through `Shell.Application`, handling tabs that share one top-level window.

**Recent work.** An in-memory index fed by jump lists for known AppIDs, the Recent folder (parsing `.lnk` bytes, never the 30 s cold COM resolve on the voice path), Office MRU from the registry, app-native recents through each adapter, a `ReadDirectoryChangesW` watcher on the user's folders, and the media history. Windows Search only when the index has nothing. Ranking uses name, recency and, from the ledger, how long the user actually had each document in focus. When names say nothing (`IMG_4412.psd`) or two candidates stay close, the result carries up to nine thumbnails for a contact sheet on the HUD. The index stays local.

**Photoshop** (designed, compiled, tested against a fake COM target only): attach `Photoshop.Application`, `DoJavaScript` for `app.recentFiles`, open, duplicate the layer, a feathered elliptical selection at the face box (after the crop confirmation), Smart Sharpen. Generative Fill and Neural Filters are tier 2.

**Later (v0.2):** Premiere through an Eileen UXP plugin connecting out to a loopback WebSocket, a VS Code extension for editor state (pulled into v0.1 only if B4 shows the window-title-and-crop route for "that variable" is unreliable, section 7), Photoshop UXP for batchPlay-only features, Windows App Actions.

### 2.8 Voice

**Pipeline** (voice-stack report, all CPU, one shipped `onnxruntime.dll` 1.28.2):

| Stage | Choice | Process |
|---|---|---|
| Audio I/O | cpal 0.18.2, WASAPI shared mode, 48 kHz, 10 ms buffers, plus loopback capture of the default render endpoint; the `windows` crate for the endpoint, the optional Communications category (with Windows ducking of other apps opted out) and MMCSS | core (`eileen-audio`, `eileen-audio-win`) |
| Resample | 48 kHz stereo to 16 kHz mono, and TTS output (24 kHz) up to the device rate; WP4 picks `rubato` or its own polyphase filter by measurement and records the choice | core |
| Echo cancellation | sonora 0.2.0 (pure Rust WebRTC AEC3 with noise suppression and high-pass; ERLE 33.3 dB on the synthetic test, 592 us p50 per 10 ms frame). The far-end reference is the system loopback (speakers' whole mix), with Eileen's own PCM as the fallback; AEC3 is bypassed after 500 ms of loopback silence. Windows' Communications-category AEC, which also uses the real speaker output, is a selectable alternative that WP4 compares with real speakers, music and video | core |
| VAD | Silero VAD v6.2 through sherpa-onnx 1.13.8 (`shared` feature), 32 ms windows, one thread, 300 ms pre-roll; idle listening cost 5.8 percent of one logical core and 31 MB | speech |
| ASR | Nemotron speech streaming EN 0.6B, 560 ms chunks, int8, four threads, fed only while VAD reports speech; WER 2.35 percent on the synthetic corpus, final about 0.4 s after speech ends, about 740 MB. In the low-memory profile, Kroko streaming zipformer (about 155 MB, WER 8.2 percent synthetic) if its license checks out | speech |
| End of turn | Smart Turn v3.2 CPU through `ort` =2.0.0-rc.13 linked to the same DLL, Whisper log-mel in Rust (matches the reference within 1.1e-5), run after 200 ms of silence. The turn ends when Smart Turn says complete and the partial does not end on a continuation (a conjunction, an article, a preposition, "and", "the", "to"), or after a silence cap of 1.2 s, stretched to 2.5 s when the partial does end on one. Smart Turn alone ended unfinished synthetic utterances early, so it is one vote | speech |
| TTS | Kokoro v1.0 fp32 from `kokoro-multi-lang-v1_0`, voice `af_heart`, four threads (int8 in the low-memory profile if WP4's equal-load test passes); replies cut at clause boundaries with a first clause of three to six words; the next clause synthesized while the current one plays; stock phrases and app launch phrases cached as PCM | speech |
| Barge-in | While Eileen speaks, VAD on the echo-cancelled signal with a stricter threshold (two windows over it while other audio plays); duck about 20 dB at the first speech window, pause at the third consecutive window (96 ms), keep the synthesized clauses, and record the samples handed to the device; about 70 to 110 ms from onset to silence. Within 600 ms the core decides: a backchannel ("mm-hm", "yeah", "right") or speech that turns out not to be addressed resumes from the paused sample; anything else cancels synthesis through the generation callback (returning `false` in the Rust crate; the Python API is inverted). An energy detector in the core is the fallback while the speech process is down | speech decides, core acts |
| What was heard | Samples handed to the device are mapped back to the clause being played and, within it, to characters in proportion to time; the brain is told the heard prefix; "go on" plays the rest | core |

Resident memory with models loaded is about 1.25 GB. Model loads take 10 to 20 s, so they load at startup, never per utterance. One ASR model stays resident. NPU and iGPU paths are out of v0.1: DirectML failed on Kokoro and Smart Turn, and only Smart Turn ran on the NPU. Windows trims the speech process's working set during long silences; WP4 measures the first command after 30 idle minutes with Chrome, VS Code and Resolve open, and `speech.pin_working_set` (off by default) keeps a hard minimum working set for users who want the first word fast after a quiet spell.

**Addressee gate.** In order, first match wins:

1. The name (from settings, matched phonetically with Soundex plus normalised Levenshtein, so a renamed assistant works at once).
2. An open question, clarifying choice or approval window: the reply is addressed for that question's grammar.
3. The conversation window: 8 s after Eileen spoke, extended to `conversation.window_s` (default 45 s) while the user is engaged (they addressed Eileen in the last 45 s, or a job they started in this conversation is running and was mentioned in the last two minutes, or a question or approval is open). The HUD shows the window as a thin ring around the mic. The window is suspended, and the name required, while other audio plays or another app uses the microphone, unless the speaker check (if adopted) matches the enrolled user.
4. A full match of a built-in command (not a learned template), unless suspended as above.
5. When the user is alone (no call, no other audio, and one voice if the speaker check is adopted), an unnamed request that refers to the screen ("make that bigger", "this window", imperative form, vocabulary of the focused app from introspection) gets a local addressee score; above the threshold it goes to the brain marked `[addressee: unsure]`, and the brain may answer `---ignore---`.
6. Otherwise the HUD shows the words greyed for a few seconds, and they are never stored. "That was for you", or a tap on the greyed line within 30 s, runs it.

The CAM++ speaker embedder (to ignore other voices and to strengthen voice approvals) is adopted only if it measures well in WP4; the first run's recording (below) is its enrollment. A name keyword spotter is evaluated the same way.

**Dictation.** Triggers: hold the dictation key (default Right Ctrl, read through Raw Input on the core's shell thread, never a low-level keyboard hook, because Windows silently removes a hook whose callback is slow and every keystroke system-wide would wait on Eileen), tap to toggle, "Eileen, take dictation" until "stop dictating" or a long pause, or one-shot "Eileen, type: ...". The Raw Input module looks only at the configured key and records only its press and release; a tidy check fails if it stores or forwards any other key code. While dictation is active, speech goes to the field, not the brain, except an escape grammar (stop, scratch that, new line, undo that, paste it). Speech model: the already-loaded streaming recognizer (no extra memory, live words on the HUD); Parakeet TDT 0.6B v3 int8 is loaded on demand for other languages and unloaded after 5 idle minutes. Whether Parakeet Unified EN 0.6B should replace Nemotron as the single English model is one measurement in WP4.

Cleanup tier 0, always, in Rust (`eileen-dictation`, a pure crate with a fuzz target, called by the core through the `DictationCleaner` port): vocabulary replacement, scratch-that, fillers per language, stutters, typed-slot self-corrections ("at 3, no, 4 pm"), spoken punctuation and layout, context formatting by app category (email, chat, code editor, terminal, search box, document), the user's own rules last; SpeakoFlow's 79 conformance cases (MIT) plus Eileen's own positives and negatives run in CI. Tier 1, opt-in: a lazily started Claude "scribe" session (fast model, no tools, connectors off, closed after ten idle minutes) that receives each finished sentence while the user is still talking, guarded by a check that rejects output adding unspoken words, dropping content words or exceeding an edit-distance budget; never for terminals, code, search boxes, password fields or private apps.

Insertion ladder (desktop server): refuse password fields and secure desktops; check the target is unchanged; `EM_REPLACESEL` for Win32 Edit and RichEdit (5.8 ms measured, exact for emoji); otherwise a port of Handy's clipboard transaction (MIT, attribution kept: full-format snapshot, delayed rendering with a read receipt, guarded restore, clipboard-history and cloud opt-out formats) plus Ctrl+V, or Ctrl+Shift+V for terminal window classes; last resort paced Unicode `SendInput`; verify where UIA can read the field; record the insertion for "undo that". If focus moved, the text waits in the HUD until "paste it". `ValuePattern.SetValue` is never used to insert, because it replaces the whole field. Dictated text is recorded for learning only as its length and the field category, never the words.

Vocabulary: `vocabulary` facts in the user model, used as sherpa-onnx hotwords (if the beam-search cost measures acceptable), as a Soundex plus Levenshtein post-pass, and in the scribe prompt. New words come from explicit requests and from a read-only UIA watch of the field for 30 s after insertion, with Wispr Flow's exclusions (file names, URLs, numbers, code, everyday words); two occurrences or one HUD confirmation make an entry. Names visible in the focused window may bias one utterance and are never stored without consent.

**Voices.** A `Synth` trait (clauses in, PCM chunks out, cancel through the callback, a measured real-time factor) with three engines:

| Engine | Use | Runtime | Notes |
|---|---|---|---|
| Kokoro v1.0 | The default voice, live conversation | sherpa-onnx | always resident |
| Kyutai Pocket TTS | Cloning from a short reference clip | sherpa-onnx (`OfflineTtsPocketModelConfig` with `reference_audio`) | MIT code, CC-BY-4.0 weights, credited in the about screen and NOTICE; loaded while a cloned voice is selected |
| Chatterbox | The more expressive cloner, per voice when the user picks it | `ort` on the shipped ONNX Runtime: Chatterbox-Turbo from Resemble's official ONNX graphs (`speech_encoder`, `embed_tokens`, `language_model`, `conditional_decoder`) in the q4 or q8 export, and Chatterbox-Nano from a community ONNX export once its license and digest check out; no PyTorch and no Python | MIT; Perth watermark off |

The owner's head-to-head (owner decision 2026-10-20) is part of WP4's acceptance: Pocket TTS against Chatterbox-Nano and Chatterbox-Turbo, each cloning the owner's own voice, measuring time to first audio, real-time factor at 2, 4 and 8 threads, resident memory and a listening test, under the exclusive build slot with at least 6 GB free. Each voice profile stores its engine (`voices\<name>\voice.toml`). The engine's live use is decided by measurement: a per-reply watchdog keeps the real-time factor of the last clauses per engine; an engine at or under 0.8 may speak live; above 0.8 the next reply (never mid-reply) falls back to Kokoro for conversation and the HUD notes it, while the cloned voice keeps announcements, summaries and read-alouds. Any engine whose real-time factor is above 0.9 renders a whole line before playing it (the HUD shows "rendering in Sam's voice") instead of stuttering.

Profiles live in `%LOCALAPPDATA%\eileen\voices\<name>\`. Roles: `voice.live`, `voice.narration`, `voice.announcements`, `voice.fallback`. Enrollment: the voice's owner reads a consent sentence naming the date and this install, then a passage; the speaker-embedding model must score the two as one speaker above a threshold; the engine's terms are shown once and the showing is recorded; a clip dropped onto the HUD strip or given as a file is accepted only together with that person's own consent recording, and refused otherwise. No watermark by default on any engine; `voice.watermark_exports = true` applies one (AudioSeal or Perth, both MIT) to saved or exported audio. Speaking as the user to other people is tier 2 per use.

**First run, with a surprise path** (owner direction 2026-10-20). A state machine in `eileen-core` (`first_run`), the clickable choices on the HUD strip (WP8), the recording, checks and clone in the speech process (WP4), and the demo as a scenario (WP9):

1. On the first `eileen run` after setup, the strip offers "Take the tour", "Skip" and "Show me something cool" (`HudInput::FirstRun`). Under the third is an unticked box worded truthfully without spoiling the surprise: "Let Eileen use my voice to personalise how she sounds and listens. You can delete it any time." The tick covers only the user's own voice; cloning anyone else still needs that person's recorded consent.
2. Surprise with the box ticked: Eileen (default voice) asks for about half a minute of free speech, with no hint of what comes next. The same recording sets the mic level, tunes echo cancellation, records how the user says the assistant's name for the recognizer hints, enrols the speaker check if adopted, and becomes the Pocket TTS reference clip. The consent record (the tick, the time, the install ID) is stored with the profile.
3. Gate: setup measured how long cloning and one line of synthesis take on this PC. If the clone's time to first audio is above `first_run.max_clone_ttfa_ms` (default 3,000), or the scripted lines do not finish rendering within 40 s, Eileen runs the demo in the default voice instead (or the tour, if the user prefers) and keeps "Show me something cool" in the tray menu for later. While the scripted lines render, the HUD shows "getting ready", never what is being prepared.
4. The demo, in the user's cloned voice: what is on screen (live from DesktopState, rendered as a whole line), opening Notepad and playing a short CC0 clip bundled with Eileen in the default player, one question answered by the brain at quick depth, two small helpers started in parallel (fast model, one turn each, skipped with a sentence if the CLI is not signed in), and a request repeated to show the second time is faster (the first through the brain's `act` tools, the second from the fast path).
5. The reveal: "That was your voice, by the way." Keep it, pick another voice, or go back to the default. If it is not kept, the clip, the clone, the speaker enrollment and every cache derived from them are deleted at once; a test asserts no file is left.

### 2.9 The HUD and the Claude app

**HUD.** `eileen-hud.exe` draws two windows through the `windows` crate (no new dependencies): an overlay (`WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, click-through) and a small strip without `WS_EX_TRANSPARENT` for the clickable parts: the mic button, Allow and Deny, Save a test, the first-run choices and checkbox, a settings panel with the common settings, and a drop target for voice clips. Text is drawn with DirectWrite into Direct2D on a DirectComposition surface, because GDI text drawn into a per-pixel-alpha layered window leaves the alpha at zero and the text invisible; a colour-keyed layered window with non-ClearType GDI text is the fallback if WP8's first measurement prefers it. Both windows call `SetWindowDisplayAffinity` with `WDA_EXCLUDEFROMCAPTURE`, which WP8's first pull request probes with exactly this window type, so screenshots never include them. The HUD treats a click on Allow, Deny, the mic or the tray menu as the user's only if it was physical: the strip reads Raw Input and ignores button presses that arrived with no device handle (synthetic input). The live mini-view of a window a job works in is a DWM thumbnail (`DwmRegisterThumbnail`) inside the strip, which costs no capture and works for covered windows. The HUD hides itself in presentation and full-screen states. The core sends a full `HudState` (section 3.3.6) at most ten times a second as one JSON line; the HUD sends input back as `HudInput` lines with a `physical` flag. Layout is computed by platform-neutral code into a draw list that snapshot tests pin; only the renderer is Windows code (`eileen-hud-win`).

**Tray and hotkeys** run on the shell thread inside the core (`eileen-shell-win`, built by WP2 so slice S1 already has them): tray-icon 0.26.1 without default features, global-hotkey 0.9.0 for mute (Ctrl+Alt+M), save a report (Ctrl+Alt+Shift+B), approve or deny the pending approval, and hands-off (pause every desktop-acting job); the dictation key through Raw Input. The tray menu is data from `eileen-contracts` (mute, pause agents, show me something cool, save a report, quit), so the HUD package adds items without touching shell code. Toasts (tauri-winrt-notification 0.8.1) need an AppUserModelID registered through the Start menu shortcut that `eileen setup` creates; they are for "a job finished while you were away" and pending approvals when the user is away.

**The Claude app** (claude-mods report): one opt-in plugin named `eileen` in `integrations/claude-plugin/`, listed by a `.claude-plugin/marketplace.json` at the repository root and installed by the owner through Claude's own plugin flow (`eileen claude install` prints the two official commands and runs them only after a yes). It holds:

- the handoff skill (`/eileen:handoff`, `disable-model-invocation: true`, passing `${CLAUDE_SESSION_ID}` to Eileen, which continues the session as a `continue_session` job with `--resume <id> --fork-session` once the user confirms by voice or on the HUD, naming the repository and the summary);
- the `eileen` MCP server for the owner's own sessions (`eileen-shim.exe --role session`: `session_status`, `session_jobs`, `session_say` (rate-limited, spoken as "From your Claude session: ..."), `session_mute`, `session_handoff`, `session_ask`); because the model in any session might be prompt-injected, none of these can approve, and `session_handoff` always needs the user's confirmation;
- a display-only mod (a status entry, a band with the last utterance and Mute, and `/eileen` answered with no model turn).

The mod never answers `tool.check`, never approves anything, never submits prompts, and Eileen never depends on it. Learned skills are not put into the owner's own sessions in v0.1 (they would cost context in every session). CI validates the plugin and fails if the validation report gains a gating hook or a new host call without a reviewed change. Nothing patches, wraps or replaces the app, and nothing turns off its updater.

### 2.10 The learning loop

From the learning-loop report, with the changes recorded in D27 and D44: version history lives in `learn.sqlite` instead of a git repository per skill; the decisions (the ladder, the linter, provenance, shadow agreement, the replay state machine) live in a pure crate, `eileen-skills`, that the core calls through the `SkillEngine` port inside `Core::step`, so the simulator runs learning scenarios like everything else; the IO (the store, the recorder, the per-session skill snapshots and the librarian scheduler) lives in `eileen-learn`, a component the runtime drives. The matcher lives in `eileen-intent`, which shares the template grammar with `eileen-skills`.

**1. Capture.** Every tool call from every origin (brain, worker, replay, fast path) becomes a `TrajectoryStep` at the dispatcher, which is authoritative for Eileen's own tools. The harness adds the worker's own tool calls from stream-json, with the input redacted and capped at 2 KB (enough for a shell command, a file path or an edit target), the cost from the `result` event, and a `SkillInvoked` event whenever the worker calls the `Skill` tool on a learned skill (namespaced `eileen-learned:<name>`, or bare when loaded another way), with the arguments it passed. "What was on screen" is structured app state, not screenshots. Redaction runs before anything reaches disk. Each step records the app and app version it touched, the tool's schema hash and the rung it used. Results are truncated by cutting long string leaves while keeping every key and the JSON structure, because recipes bind to that structure. Each job gets a 120 s regret window: only addressed utterances count; a signal is attributed to the job announced last or the focus job, and when two jobs finished inside the window and the words name neither, no signal is recorded (or Eileen asks once). App-level undo counts only as Ctrl+Z seen through Raw Input while a window the job touched has the foreground (nothing else about keys is kept). A retry is a matcher hit on the same skill, or a resolver tool call with the same target, within 120 s. The flight recorder pins a job's input lane for the job's whole duration (capped at 30 minutes), so a long first run can still be saved as a test.

**2. Distillation.** The librarian is a Claude worker with no built-in tools and only the `learn` toolset (`learn_trajectory_read`, `learn_skill_list`, `learn_skill_read`, `learn_skill_history`, `learn_skill_propose`, `learn_fact_propose`). It writes native `SKILL.md` folders ("lessons, not logs") with optional `reference/` files and an optional `recipe.json`. `learn_skill_propose` validates the schema, computes provenance and runs the linter, and returns every error so the librarian can retry a bounded number of times (three); nothing else can write a skill.

- **What it sees.** Only trajectories of jobs that already ran on Claude, and only the fields that were in that job's model context. Fast-path and replay jobs, and dictation, are never sent (dictation is recorded only as length and field category). `learning.distill = "claude" | "off"` controls it, and the first-run privacy note says plainly that distillation sends job records to the user's own Claude plan.
- **What gets distilled.** A deterministic pre-filter in Rust, not a model pass: skip jobs marked no-learn, failed jobs, jobs with an unclear verdict and no objective check, and jobs with fewer than three non-read steps; when a job used a skill successfully with no divergence, only that skill's statistics change. Distil new request kinds, repairs and runs that diverged from a skill, batching up to five trajectories per session.
- **When.** On demand ("learn from that now", `eileen skills distill [--job ID] [--wait]`), or whenever no Eileen job is running and free memory is above 3 GB, even while the user types, at below-normal priority. It is spawned for the batch and stopped afterwards.
- **What it may spend.** `learning.librarian_max_plan_pct` (default 50) of the five-hour window, and nothing when the seven-day window is above 70 percent. Every run's cost is stored (`librarian_runs`), so "what has learning cost me" has an answer. Librarian jobs are announced on the HUD only.

**3. Retrieval.** Every worker session gets its own learned-skill snapshot: at spawn, `eileen-learn` writes `run\<session>\plugin\` (a `.claude-plugin\plugin.json` and the skill folders) from `learn.sqlite`, which is the single source of truth, and records a manifest of skill id, version and BLAKE3 hash per file. The snapshot holds the skills most relevant to the session (for a job, the matcher's hints for its task plus the focused app; for a pooled session, the most used skills of its kind), at most `learning.snapshot_k = 12`, because the CLI lists every description in context. A worker therefore never sees a skill change mid-session, and nothing outside Eileen can slip a folder into what workers load. When the brain dispatches a job for a matched skill (`jobs_dispatch` with `skill`), the job's first user turn says "Follow skill eileen-learned:<id>"; if a pooled session's snapshot lacks that skill, the skill's text (at most 4 KB, Eileen-authored) is carried inline in that turn instead. The trajectory records the snapshot manifest and every `SkillInvoked`, so each run knows which skill version it used. Locally, L1 templates can trigger replay and L2 hints only inform. WP7 probes whether claude 2.1.304 has a total listing budget and records it; the weekly contract test repeats the probe on each new CLI.

**4. Recipes.** A recipe is a typed list of steps that Eileen runs with no model (section 3.3.13):

- **Steps** are `call` (an Eileen tool and op), `delegate` (a narrow worker sub-job with a goal, its bound inputs and postconditions, for the one part a model must do), or `confirm` (show the user something, such as a crop, and wait for a yes; replay never skips it).
- **Arguments** may refer to a slot (`{"$slot": "t"}`) or to an earlier step's output (`{"$step": "faces", "ptr": "/picked/rect"}`); the linter checks that every reference points at an earlier step whose output schema has that pointer.
- **Checks** may call only ops that are read-only and tier 0 (declared per op, so `apps_resolve` `timeline_info` is a check and `apply_drx` is not).
- **Retry** (`on_fail = "retry_once"`) is allowed only on idempotent steps; a second `apply_drx` would add a second node.
- **Eligible for a recipe**: every non-read argument comes from a slot or an earlier output, every step has a postcondition, and no step is a raw coordinate click. Steps that were denied, or are tier 2 or above, are never compiled into a recipe; at that point the procedure says to ask the user.

**5. The promotion ladder.**

| Level | Name | How a skill gets here | What it does |
|---|---|---|---|
| 0 | Trajectory | Every job | Evidence only |
| 1 | Draft | Distilled from a successful job and passed the linter. A draft with any line traced to observed text waits in `pending\` until the user approves each such line on the HUD; every other draft goes straight to level 1 | Goes into matching sessions' snapshots; never replayed |
| 2 | Trusted | Successes with at least two distinct slot values and no recent failure | Hints first; named to the brain; its recipe runs in shadow mode beside the worker |
| 3 | Compiled recipe | Agreeing shadow runs and one explicit approval ("I can do this without asking Claude from now on. OK?") | After a template match, one brain turn confirms intent by dispatching a `Replay` job; Eileen runs the recipe with checks after every step |
| 4 | Spoken fast path | Clean replays | A template match runs the recipe with no model call at all |

Thresholds are data in `LadderPolicy` and depend on risk. A **light** skill (every step tier 0, or tier 1 with a journaled inverse, touching nothing the user authored) needs 1 agreeing shadow run plus approval for level 3 and 3 clean replays for level 4. A **standard** skill (anything else) needs 3 successes with two distinct slot values for level 2, 3 agreeing shadow runs plus approval for level 3, and 10 clean replays with at least 95 percent clean over the last 20 for level 4. A success means: the job completed, no negative signal was attributed to it in the regret window, and at least one objective check passed (a verification hint or a skill postcondition) or the user praised it. An unclear verdict with no objective check counts toward a draft but not toward trusted, and a worker's own "done" never counts alone. Two failures in the last five uses drop a level; a change in the version of an app a recipe uses, or in the schema hash of a tool it calls, puts the recipe on probation (one shadow run before the next replay, or a migration by the linter); 30 idle days archive a skill (never automatic deletion); pinned skills are never archived. Promotion to level 4 is announced.

What each benchmark should look like run by run (B8 and the M6 demo assert this table):

| Request | Run 1 | Run 2 | Runs 3 to 5 | Later |
|---|---|---|---|---|
| B1, the Resolve sharpen (standard) | Worker explores; the brain's `act` tools do the navigation; draft distilled afterwards | Worker follows the draft: fewer turns, no dead ends | Trusted after the third success with two different times or faces; recipe in shadow | Level 3 after three agreeing shadows and approval (about run 7): Eileen replays navigation, stills, face pick, the crop confirmation and the Fusion edit; a delegated step only if the edit needed computer use. Level 4 needs 10 clean replays, and the confirmation step stays |
| E3 with a video the media history knows | Built-in grammar, no model, from the first repeat | Same | Same | Same |
| A media request the grammar does not cover (light) | Worker; draft | Worker with the skill; recipe in shadow agrees | Level 3 after approval; level 4 after three clean replays (about run 6) | No model |
| E5 fan-out orchestration (light, scaffolding only) | Brain plans; draft | Worker-free scaffolding in shadow | Level 3 or 4 for creating worktrees and starting jobs | The coding itself is always a worker |

**6. Replay.** The replay state machine runs inside the core (`SkillEngine`), so it is simulated and replayed like any other logic. Slots resolve deterministically in Rust or the job goes to a worker before any action; an enumerated slot value no successful use has seen (a face position never seen, an app never seen) runs once at level 2 with a worker before it may replay. Each step runs through the same dispatcher and permission gate as a worker with origin `Replay { skill, version }`; confirmations are raised again; the HUD shows "replaying resolve-sharpen-face v3, step 4 of 7"; the user can interrupt between steps and "stop" cancels at once. The first failed check stops the replay and hands off to a Claude worker with a briefing (request, skill and version, steps done with results, the failed check with the observed state, steps left, continue from here). The recovery trajectory becomes a repair candidate.

**7. Shadow agreement** is a pure, table-tested function in `eileen-skills`: resolve the recipe with the job's slots; compare it with the worker's non-read Eileen tool calls by tool, op and bound argument values (with a numeric tolerance per slot kind, for example a face box within 10 percent); ignore order between independent steps; and require every recipe postcondition to hold on the worker's actual results.

**8. Self-healing selectors.** Steps are recorded by identity (process, window class, automation id, control type, name, ancestor chain) plus a postcondition, with the element index only as a hint. When a selector no longer resolves after an app update, the desktop server picks a candidate from evidence gathered before acting: a unique match on control type, name similarity above a threshold, the same ancestor roles. It acts once; if the postcondition fails it hands off. A selector repair, like a content repair, starts one level down.

**9. Safety, and keeping poison out.** A replayed step carries no permission of its own; "always allow" grants are never written into skills. Provenance is computed by `eileen-skills`, never accepted from the librarian: every literal of six tokens or more in `SKILL.md`, the references and recipe arguments must trace to the user's own transcripts in the evidence jobs, to Eileen's tool schemas and descriptions, or to an approved observed line. A literal that overlaps observed content (an n-gram match against the wrapped observed text) is tagged observed and needs the user's approval on the HUD; one that traces nowhere is rejected. The dispatcher taints a job's later steps after its first read of web content, a file outside the job's folder or a notification; tainted steps whose literal arguments do not come from slots (URLs, paths, commands) are dropped from recipes, and their procedure lines need approval. A deny-scan of the skill text rejects URLs the user did not give in the evidence jobs, upload or send verbs with a destination, credential-like paths, "ignore previous" patterns, and a `when_to_use` that claims every task or every session. Learned frontmatter is an allowlist (`name`, `description`, `when_to_use`, `arguments`, `argument-hint`, `disable-model-invocation`, `user-invocable`, `metadata`); no `scripts/` folders, no `` !` `` sequences and no shell preprocessing; secret-like strings are rejected; auto memory and session persistence are off in every session Eileen starts. WP7 tests each of these with an injected web page through the stub harness.

**10. User control** by voice and HUD: what have you learned, what do you know about me, show me how, learn from that now, that's wrong, correct it ("no, my timelines start at zero": the fact is retired and replaced with the user's version; a skill that used it is queued for repair and starts one level down), forget that (confirms once, then deletes the skill, its versions and its evidence everywhere, and writes a tombstone), don't learn from this, stop or start learning, approve, use the old version, how much time have you saved me, what has learning cost me.

**11. The user model and the episodic ledger.** Itemized facts (`user`, `feedback`, `project`, `reference`, `vocabulary`, `phrasing`, `source`) with provenance, the source turn, a last-confirmed time and a confidence, changed by deltas (add, confirm, retire), never whole rewrites. The user's own addressed corrections become facts through a deterministic path in the core, with provenance `user`; the librarian proposes facts with `learn_fact_propose`, never with provenance `observed`; facts are deduplicated and capped per kind; facts unconfirmed for 90 days retire. Workers get a facts block of about 400 tokens scoped to the job's repository and app (for example "this repo tests with cargo test", "make them compete, don't split"). The ledger records Eileen's own events plus sampled focus changes (app, window title, document identity, focus time) for 14 days and answers "go back to what we tried an hour ago", "what was that page I had open yesterday" and the Photoshop recall in E8; InPrivate and incognito windows, password managers and private apps are never recorded, and off-the-record mode records nothing. Ledger lines go into a brain turn only when the utterance refers to the past, or when the brain calls `ledger_search`. At most about 600 tokens per brain turn come from learning.

**12. Privacy of what is stored.** `learn.sqlite` and `eileen.db` run with `PRAGMA secure_delete=ON`; after a forget, the WAL is checkpointed with `TRUNCATE` and blobs are reference-counted, so unreferenced ones are deleted. "Forget" also purges the job's rows in the `events` table and its incidents, recordings and saved scenarios; an acceptance test greps every file under the data directory for a canary request string afterwards. At promotion, evidence shrinks to what replay and measurement need (slot values, tool names, bound arguments, checks, cost), dropping free-text results. The learning folder gets a user-only ACL and is marked not to be indexed by Windows Search. Encryption at rest is v0.2 (decision D40).

**13. Measurement, and skills as tests.** Per skill: uses, successes, failures, undos, handoffs, and median duration, model turns and tokens of the first runs against the last ten; "time saved" is the difference times the uses. When a skill becomes trusted, and at every replay handoff, Eileen writes local sim-lane scenarios from its evidence (never shared automatically). A librarian repair is committed only if the skill's scenarios still replay green. Once per trusted skill, the HUD offers "Save this as a test for the project?" through the normal share preview and redaction. Fleet results carry anonymous learning statistics (skills per level, handoff rate, median turns by run index; no skill names or text). Sharing skills across many machines is v0.3; the fleet will check a skill on several machines before it can ever be offered.

**14. The worked recipes, with real tool names.**

`resolve-sharpen-face` (slots `t: Offset`, `which: Position`, `amount: Choice`):

1. `call apps_resolve hello` (post: `ok`), `call apps_resolve timeline_info` (post: a timeline is current; else delegate "open the right project and timeline" to the brain's question flow)
2. `call apps_resolve open_page {page: "edit"}`, then `call apps_resolve goto {offset_s: $slot t}` (post: `timeline_info.playhead` equals start plus t, within one frame)
3. `call apps_resolve export_still` then `call apps_image faces {path: $step still /path}` (post: at least one face) then `call apps_image pick_face {path, which: $slot which}`
4. `confirm` with the crop from step 3 ("This face, on the right?")
5. `call apps_resolve add_version {name: "eileen-<job>"}` (idempotent; post: the version exists)
6. `call apps_resolve import_fusion_comp {template: "sharpen_ellipse", boxes: $step track /boxes, amount: $slot amount}` (the route the spike picked; or `delegate` "apply a sharpen window at this box on the Color page" when only computer use works)
7. `call apps_resolve export_still` then `call apps_image sharpness {path, rect}` (post: at least 15 percent above the step-3 value)

The `track` step before 6 is `call apps_image track_face {stills: ..., box: ...}` over stills exported every few frames, so the whole recipe is model-free when the Fusion route works.

`youtube-play-at` (slots `video: Media`, `t: Offset`), for phrasings the built-in grammar does not cover: `call apps_recent {query: $slot video, kinds: ["media"]}` (post: one confident match; otherwise `delegate` "find this video on YouTube" to a worker), `call apps_open_url {url: $step match /url, start_s: $slot t, wait_for_media_title: $step match /title}`, `call apps_media status` (post: position within 3 s of t after any ad).

`launch-agents-fanout` (slots `n: Count`, `task: Text`, `repo: File`): `call jobs_dispatch {kind: "coding", fanout: $slot n, mode: "compete", repo: $slot repo, task: $slot task}` (post: n jobs accepted). The coding itself is always a worker; only the orchestration is compiled.

### 2.11 Settings by voice, and configuration

**Files.** TOML with comments (`toml` 1.1.8), in `%LOCALAPPDATA%\eileen\config\`: `eileen.toml` (the user's settings, every key documented in a comment), `profiles\<name>.toml` (overlays), and a generated `eileen.schema.json` for editor completion. The commented default file and the schema are generated from the config types' doc comments by `cargo xtask codegen` and checked fresh by a test. `eileen config explain` prints every effective value and where it came from (default, file, profile, voice, HUD).

**The settings model** (`eileen-config`, section 3.3.11). Every setting has a `SettingSpec`: key, type, default, doc, whether it is reachable by voice and on the HUD (all are, unless stated), its risk class, how it applies (on the next turn, live, or restart of one component) and its safety bound. A test fails unless every leaf key of the config schema has exactly one `SettingSpec` with a matching doc, and the reverse. Changes go through one flow, whatever the source:

1. **Propose.** The brain calls `settings_propose` with the changes and the user's words, the L1 grammar does it for fixed phrasings, or the HUD's settings panel does it for a click. The proposal is validated against the schema and the safety bounds. The utterance the brain passes is informational only and is never evidence of consent.
2. **Check before applying** when a choice could misfire. A new name: not on a common-word list, not close to a household member's name in the user model, and recognised in three of three sentences Eileen synthesizes and transcribes with its own recognizer. A new hotkey: a trial `RegisterHotKey` that fails if another app owns it. A new voice: the real-time factor of one clause.
3. **Confirm** in one sentence. Low-risk changes apply at once with "say undo that to go back"; changes with a warning ask first.
4. **Apply and version.** Every applied change writes a row in `settings_versions` (full snapshot, diff, source, the utterance); "undo that" or "go back to how it was" applies an earlier snapshot as a new version, so history only grows. Hand edits of the file are picked up by a watcher, validated (an invalid file keeps the previous version and the HUD names the line), and versioned with source `file`. Comments survive every apply.
5. **Profiles** ("work mode", "editing mode") are overlays switched by voice and exportable as one TOML file.

**Safety bounds.** Tier 3 can never be relaxed. A relaxation for a tier-1 action is allowed by voice. Skipping confirmation for a tier-2 action needs an explicit, logged decision: Eileen names the action, the user says "yes, always allow <action>" (or clicks it on the HUD), and the decision is stored with the approval that made it; it can never cover credentials, payments, sending messages as the user or deleting outside a repository. **Safety-relevant keys** (risk `Safety`) change only with an approval built from the user's own voice or click, never because the brain proposed it: `hud.visible_intent_ms`, `desktop.lease_idle_s`, every `privacy.*` key, `logging.include_personal`, `permissions.*` (private apps, desktop apps, grants), the approvals rows of `announcements`, `approvals.*`, `fleet.upload`, `learning.distill`, `mcp.servers`, `hotkeys.mute` and `conversation.window_s`. The product and repository stay Eileen; the user's assistant name, voice and personality are the user's own.

**Rename and personality** ("I want to call you Nova now", "be funnier") change `assistant.*`, which the addressee gate, the recognizer hint list, the phrase cache and the per-turn `[assistant]` block read. Because personality travels in the turn, not the system prompt, the change applies on the next turn with no brain restart; the cache re-renders phrases that contain the name.

### 2.12 Permission tiers and security

| Tier | Name | Examples | Behaviour |
|---|---|---|---|
| 0 | Observe | DesktopState, a screenshot, read a file in the job's folder, read a page, read a job | Silent |
| 1 | Reversible | Move or snap a window, change the volume, open an app, a file or a site, play or pause, edit files in the job's repository, create a branch, merge locally, add a Resolve version and an edit on top of it, insert dictated text | At once, journaled with its inverse |
| 2 | Consequential | Send a message, delete outside a repository, push, install software, spend credits, change settings outside Eileen, speak as the user to others, publish a scenario or results, any shell command not on the allowlist, read outside the job's folder, network egress once a job is tainted | Confirm first |
| 3 | Blocked | Enter credentials, make payments, disable security features, bypass a CAPTCHA, read vendor credential files or Eileen's own data folder, type text that looks like a secret or a card number | Never; explain and hand the step to the user |

Every tool declares its tier in its `ToolSpec`, per op where a tool has ops; a few depend on arguments (a control named Send is tier 2; a blocked combo is tier 3). An undeclared tool, including any tool of an approved third-party MCP server the user has not relaxed, is tier 2.

**Worker built-in tools** are classified by a deterministic policy in the core (`eileen_core::tiers::claude_tool_tier`), from facts the runtime adds to each `PreToolUse` request before the core sees it (the core does no IO, so the runtime canonicalises every path argument and classifies it):

| Path class | Read, Grep, Glob | Edit, Write |
|---|---|---|
| The job's repository or worktree, its scratch folder, the job's temp folder | 0 | 1 |
| Toolchain folders (cargo registry and git, rustup toolchains, npm global modules, uv cache) | 0 | 2 |
| Eileen's data folder (`%LOCALAPPDATA%\eileen`), credential stores (`.ssh`, `.aws`, `.config\gh`, `.git-credentials`, `.netrc`, vendor CLI credential files, Windows credential folders), browser profile folders | 3 | 3 |
| Anywhere else | 2 | 2 |

**Shell commands** (`Bash`, `PowerShell`) are parsed with a small tokenizer (never evaluated) and default to tier 2. An allowlist makes the usual work tier 0 or 1: read-only commands (`git status`, `git diff`, `git log`, `ls`, `dir`, `cat`, `type`, `rg`, `grep`), local git (`add`, `commit`, `branch`, `switch`, `checkout`, `stash`, `merge`, `worktree list`), and build and test commands inside the job's folder (`cargo build|check|test|clippy|fmt|xtask`, `npm test`, `npm run test`, `pnpm test`, `pytest`, `python -m pytest`, `uv run pytest`, `go test`, `dotnet build|test`). Known egress binaries and subcommands (`curl`, `wget`, `Invoke-WebRequest`, `Invoke-RestMethod`, `ssh`, `scp`, `gh`, `git push|fetch|pull|clone|remote add`, `npm install|publish`, `pip install`, `cargo install|publish`, `winget`) are tier 2; security settings (`Set-ExecutionPolicy`, `netsh`, `bcdedit`, `reg add` under policy keys, `icacls` and `takeown` outside the job's folder) are tier 3; anything naming a credential path is tier 3. A command the tokenizer cannot fully parse (command substitution, `-EncodedCommand`, `Invoke-Expression`, `iex`, unknown parts in a pipeline) is tier 2 and can be approved only by a HUD click, never by voice, because its spoken summary would not be trustworthy.

**Taint.** Each job carries two flags in the core: `untrusted_seen` (after it read web content, a desktop tree, a screenshot, the selection, or a file outside its folder) and `private_read` (after it read anything outside its folder and the learned-skill snapshot). Once both are set, every call that can send data out is tier 2: `WebFetch`, `WebSearch`, `apps_open_url` or `apps_open_uri` to a host the user did not name and has not visited, and any shell command not on the allowlist. A simulator invariant checks it. Where the task names a site, `WebFetch` is restricted to that domain for the job (Claude's domain rules in `--allowedTools`).

**Containment in v0.1** (decision D40, which amends D14): the job's repository or worktree, the path guard, the shell policy, taint, the `PreToolUse` guard failing closed, per-app desktop grants and Job Objects. The CLI's `--restricted` mode (section 2.5) is the second fence if WP3's test shows workers still work under it; other sandbox and Windows container options are measured by WP3 and recorded; a real sandbox and encryption at rest are v0.2.

**Credential boundary.** Eileen spawns the unmodified, installed `claude` binary and never reads, copies, logs or refreshes vendor credentials, never runs a login flow, and never writes the user's Claude settings for workers (all worker configuration is per-session flags). The binary is resolved to the real `.exe`: on this PC `claude` on `PATH` is the npm shim (`%APPDATA%\npm\claude.cmd` and `claude.ps1`), which points at `node_modules\@anthropic-ai\claude-code\bin\claude.exe`; the resolver parses the npm shim and the native installer's versioned layout and never picks the copy bundled inside the Claude desktop app. `eileen doctor` checks sign-in with `claude auth status` (exit code), never by reading files, and compares the installed CLI version with the one the brain is running. Because the always-on brain and spare keep the executable open, which can block an npm update on Windows, Eileen offers an update window: it drains workers, stops the brain and spare, lets the update run, and restarts them (WP3 verifies the behaviour once).

**Environments.** Two policies, built by `eileen-harness`:

- **Brain, librarian and scribe** (no shell): a constructed allowlist. It starts empty and adds only `SystemRoot`, `SystemDrive`, `windir`, `USERPROFILE`, `HOMEDRIVE`, `HOMEPATH`, `APPDATA`, `LOCALAPPDATA`, `TEMP`, `TMP`, `PATH` (system directories, git, the claude install directory), `PATHEXT`, `COMSPEC`, `NUMBER_OF_PROCESSORS`, `PROCESSOR_ARCHITECTURE`, `OS`, plus the session's own variables.
- **Workers** (they run the user's builds and tests): the user's own environment, with system plus user `PATH` read from the registry so a freshly installed toolchain is seen, minus a denylist: every `CLAUDE*`, `ANTHROPIC*`, `MCP_*`, `OPENAI*`, `CODEX_*`, `XAI_*` and `DEEPSEEK*` variable, anything ending in `_API_KEY`, `_TOKEN` or `_SECRET`, `GH_TOKEN`, `GITHUB_TOKEN` and `AWS_*`. A short pass-through list survives the denylist: `CLAUDE_CODE_GIT_BASH_PATH` (the CLI uses it to find Git Bash on Windows), `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` and `NODE_EXTRA_CA_CERTS`. For coding jobs, toolchain folders Eileen can detect (`%USERPROFILE%\.cargo\bin`, Node, uv) are prepended when missing. Then the session's own variables are added.

A tidy check fails if any source names a vendor credential path or the persistent MCP registration command.

**Untrusted input.** Window titles, UIA text, page text, file contents, transcripts of other voices, tool results and model output are data. In the contracts they are wrapped as `Untrusted<T>` or `Personal<T>` (section 3.3.1), and a test fails if a free-text field reachable from `Input`, `ToolResult` or `DesktopState` is left unwrapped. They can never approve, raise a tier, create an override, start a job from an ambient turn, or become a skill without the user's approval. The core's approval type can only be built from `UserInput`.

**Who is calling.** Tokens sit in files any process of the same user can read (`run\<session>\mcp.json`, `run\bus.json`), so a token alone proves nothing:

- The dispatcher calls `GetNamedPipeClientProcessId` on every pipe connection and accepts a role only from an `eileen-shim.exe` from Eileen's own install folder running inside the Job Object of the session that token belongs to (`IsProcessInJob` with the supervisor's job handle). A worker that copies the brain's token is still inside its own job and is refused.
- The loopback server maps each client socket to its process (`GetExtendedTcpTable`). Hook endpoints accept only a process inside the matching session's job; every other endpoint (state, control, `say`, `handoff`) refuses any process inside any Eileen-started Job Object.
- `eileen say` enters as `TurnSource::Cli`, which can never answer an approval, confirm a safety setting or create a grant, and gets read-only brain tools (Turn authority, section 2.3).
- The path guard denies workers Eileen's whole data folder (tier 3).

WP2's acceptance includes a process inside a worker Job Object, holding every token file, being refused by every endpoint and every other role's pipe.

**Loopback endpoint.** Binds 127.0.0.1 only on a random port, with a per-boot bearer token in `%LOCALAPPDATA%\eileen\run\bus.json` (user-only ACL). That token reaches status and control (mute, say) only; no endpoint reachable with it can approve anything.

**Secrets.** Only one is allowed now (the DeepSeek key, used by no v0.1 route), plus connector tokens later. They live in Windows Credential Manager through keyring-core 1.0.1 and windows-native-keyring-store 1.1.0 (a Windows-only dependency), are typed as `secrecy::SecretString`, and are never in config files, logs or model-visible text.

**Privacy.** Everything stays on the PC except what a Claude session sends to the owner's own plan.

- Speech that was not addressed to Eileen stays in memory only: the HUD shows it for a few seconds, and every persisted sink (the `events` table, logs, the flight recorder's input lane when an incident or recording is written, scenarios) stores "[not addressed, n words]" instead, unless the user opts in. Nothing heard while another app uses the microphone (a call) is stored at all.
- The `events` and `approvals` tables keep 30 days; trajectories, the ledger, recordings and incidents are local, size-capped and deletable by voice (section 2.15). Screenshots are not stored by default.
- The public repository never receives the owner's voice, screen or personal paths (tidy enforces it for fixtures and scenarios).
- The HUD's privacy note says plainly that Eileen can delete what it stores but not what a vendor retains.

### 2.13 Observability

From the observability report. Typed, prefixed ULIDs: `sess_`, `turn_` (the request ID), `job_`, `ws_` (a worker session), `call_`, `speak_`, `appr_`, `inc_`, `rec_`. `TraceCtx { turn, job, call }` rides on every bus envelope, every speech frame (the header carries the turn; JSON commands carry the full `TraceCtx` and the parent span id), every MCP request (`_meta.traceparent` built from the turn ULID, plus the IDs under an `eileen/` key; rmcp 3.6.0 has the accessors) and every worker environment (`TRACEPARENT`, which `claude -p` reads at start, and `EILEEN_SESSION_ID`; hooks name the session and the core maps it to the bound job). One trace per turn; a job is a child span of the turn that started it and may outlive it. Span and event names are constants in `eileen_trace::names`; user content travels only in fields prefixed `pii.`; a span-parent table is checked in every simulation run. Every task is started through `eileen_rt::task::spawn`, which names it, keeps the current span and registers it with `await-tree`; bare `tokio::spawn` is banned by clippy everywhere else.

Sinks in every process: JSON-lines files at INFO (`%LOCALAPPDATA%\eileen\logs\<process>.<date>.jsonl`, daily rotation, a janitor caps the total at 500 MB, `pii.` fields reduced to length and keyed hash unless `logging.include_personal`), an in-memory flight recorder at DEBUG (10 minutes or 32 MB in the core, 8 MB per sidecar, plus the core's input lane with a checkpoint every minute, so every incident is replayable; a running job's input lane is pinned for its whole duration, up to 30 minutes), a console at WARN when started from a terminal, and the slow-span layer. The WASAPI callback never calls `tracing`; it bumps a heartbeat counter and pushes fixed-size ticks into a real-time-safe ring. OpenTelemetry, ETW and `tokio-console` are opt-in features that the PR lane never compiles (v0.2). When the input lane is written to disk (an incident, a recording, a saved scenario), speech that was not addressed to Eileen is replaced by "[not addressed, n words]", which no grammar or name rule matches, so a replay still reaches the same decisions.

Incidents are written on a panic, crash, watchdog trip, supervisor kill, "Eileen, something went wrong", the hotkey or the tray, into `%LOCALAPPDATA%\eileen\incidents\<time>_inc_<id>\` with `manifest.toml` (section 3.3.16), merged events, the replayable input lane, `timeline.json` for Perfetto, the async task tree, the last DesktopState, the job snapshot, redacted config, log tails and minidumps. `eileen-guard.exe` writes minidumps from outside the crashing or hung process (Embark's crash-handler 0.8.1 and minidumper 0.11.0), hang-dumps a sidecar before the supervisor kills it, and restarts a hung core at most once per 10 minutes.

Replay (section 3.3.16 and `eileen-replay`): L1 replays the core logic from a recording and reports the first divergent event (strict mode, for incidents and `--doublecheck`); scenario mode replays a saved or shared scenario against a fresh core and checks its expectations (section 4.8); L2 adds the real speech process on recorded audio; L3 is a live re-run by hand. `eileen replay inc_... --level core` tells logic bugs from model, desktop or timing problems. A redaction canary test injects fake secrets, a spoken card number and a password-manager clipboard, and fails if any sink writes them where policy forbids. The end-to-end smoke run asserts that every JSON log line written by any process between a turn's open and close spans carries its turn or job ID, or comes from a short allowlist of background targets.

Code navigation for people and Eileen's own Claude workers: the generated `docs/CODEMAP.md` (crate and process graph, module trees, and the event map of which code constructs and handles each `BusEvent` and `Input` variant), and a read-only toolset `dev` on the `eileen` MCP server (`dev_codemap`, `dev_crate_doc`, `dev_event_map`, `dev_trace_get`, `dev_logs_query`, `dev_incident_list`, `dev_incident_read`, `dev_replay_core`, `dev_scenario_list`) that applies the share-pack redaction policy to everything it returns. rust-analyzer reaches at most one Eileen coding worker at a time through a repo-local `eileen-dev` plugin with `checkOnSave` off, inside that job's memory admission; the agents that build Eileen itself during the build phase run without it (section 6.1).

### 2.14 Connectors and platforms

**Connectors.** A connector is a capability provider: it publishes a `ConnectorManifest` (section 3.3.12) with its tools (name, schema, and per op the tier, read-only flag, idempotence and whether it needs the foreground), its event streams, its read-only check ops for recipes, where it runs (a task in the core for pure network code, or a supervised sidecar for anything with FFI or drivers), and whether it senses the room. The router, the permission gate, the job registry and the learning recorder see only manifests and tool calls. v0.1 ships two: `pc.desktop` and `pc.apps`. Third-party MCP servers the user approves per app (section 2.5) are not connectors in this sense: they talk to workers directly, and the `PreToolUse` guard tiers their calls. The phone (Telegram and ntfy, then voice from anywhere), Home Assistant (and Matter through it), other devices on the user's network and earbuds as microphones are new connectors with the same tiers, learning loop and job model. Audio-producing connectors deliver 16 kHz frames into the same VAD, ASR and gate, tagged with their own `source` in the speech frame header (section 3.3.7), each with its own echo handling. Room-sensing connectors are opt-in per device, processed locally, and contribute only derived events to trajectories; the registry refuses to start one without a recorded opt-in.

**Windows first, not Windows only.** Every OS-touching piece sits behind traits in `eileen-platform` (section 3.4) and is implemented in a per-area Windows crate (`eileen-audio-win`, `eileen-desktop-win`, `eileen-apps-win`, `eileen-shell-win`, `eileen-hud-win`). The sidecar binaries hold platform-neutral server logic and pick the implementation by target. Native libraries (cpal, sherpa-onnx, ort, the keyring store, tray-icon, global-hotkey, the toast crate, crash-handler) are declared only under `[target.'cfg(windows)'.dependencies]`, per the native dependency table in section 4.1, so the neutral logic around them (the speech pipe protocol, turn fusion, the log-mel, the clause splitter, `WavSource`, `RoomMixer`, timecode math, face picking) builds and tests on Linux. A Linux port means new `-linux` crates (AT-SPI, PipeWire, Wayland or X11 input and capture, MPRIS over D-Bus, Secret Service), not a redesign. Every platform-neutral crate builds, lints and tests on `ubuntu-latest` in CI from the first commit; Windows-only crates compile to an empty library on other targets.

### 2.15 Data directory

```
%LOCALAPPDATA%\eileen\
  config\           eileen.toml, profiles\<name>.toml, eileen.schema.json
  eileen.db         jobs, events (with seq and TraceCtx; 30 days), approvals (30 days), settings_versions, journal,
                    plan_usage, memory samples (1 day)
  learning\         learn.sqlite (trajectories, skills, skill_versions, uses, facts, ledger, answers cache, librarian_runs,
                    tombstones); pending\ (drafts with observed lines, waiting for approval); user-only ACL, not indexed
  models\           vad\, stt\, turn\, tts\, voice\, embed\, face\ (downloaded by eileen setup, SHA256-checked)
  voices\           <name>\ voice.toml (engine, consent record), reference.wav, consent.wav, state cache
  phrases\          <voice>\<hash>.pcm (stock and app-launch phrase cache)
  cache\            apps.json (the last app index), recent.json (the recent-work and media index)
  logs\             <process>.<date>.jsonl
  incidents\        <time>_inc_<id>\ ...; pending\
  recordings\       rec_<id>\ manifest.toml, inputs.jsonl, audio\ (opt-in)
  blobs\            <2 hex>\<blake3>.zst (shared by recordings, incidents and trajectories; reference-counted)
  scenarios\        saved\<id>\ (local scenarios, including those generated from trusted skills), outbox\<id>\
                    (redacted, approved for sharing)
  fleet\            results\<date>.jsonl, machine.toml (random install id, never hardware ids)
  worktrees\        <repo-hash>\<job-slug>\
  run\              bus.json, brain\, <session>\ (claude-settings.json, mcp.json, plugin\ snapshot, cwd\), guard socket
  build-slot-0.lock, build-slot-1.lock, desktop.lock   (the build guard's locks, section 6.1)
```

Retention defaults: trajectories 30 days unless cited by a skill (200 MB cap; evidence shrinks at promotion), the ledger 14 days, facts retire after 90 days unconfirmed, `events` and `approvals` 30 days, recordings 7 days and 2 GB, incidents 30 days and 1 GB, logs 14 days and 500 MB. A janitor in the core enforces them; "delete my recordings" and "forget that" remove data at once, including the rows and blobs that referenced it. `EILEEN_DATA_DIR` (and `eileen run --data-dir`) points everything at another root, which every test uses.

## 3. Workspace and contracts

### 3.1 The Cargo workspace

One workspace (resolver 3, edition 2024, `rust-version = "1.99"`, toolchain pinned to 1.99.0), flat `crates/*`, `publish = false` everywhere. The root manifest uses `members = ["crates/*", "xtask", "tools/repo-tools"]` and `exclude = ["fuzz"]`, so a package adds or fills its crate without touching the root manifest. Every third-party dependency is declared once in `[workspace.dependencies]` (section 4.1); members write `foo.workspace = true`. Members may add features to other dependencies, but not to `windows`, `windows-sys` or `tokio`: their feature sets are declared once as the union every package needs, so `cargo xtask check -p <crate>` and the workspace build compile them once instead of alternating (a check enforces it). A package that needs one more feature of those three opens a one-line `amendment` pull request against the root manifest; that is the only way a later package touches it. Internal crates are declared in `[workspace.dependencies]` too.

WP0 creates every crate below as a documented stub that compiles on Windows and Linux: the crate doc with the guidance sections (section 4b.4), the public types and signatures this section specifies, the real native dependencies under their target tables (so native build failures surface in W0, not in WP4's first pull request), and bodies that return a typed `Unavailable` error or do nothing. When WP0's phase A has merged, each stub belongs to the package named in the table, which replaces the stub body and keeps the signatures (or changes them through an amendment pull request to this file).

| Crate | Kind | Platform | Process | Unsafe | Owner | May depend on (internal) |
|---|---|---|---|---|---|---|
| `eileen-contracts` | lib | neutral | all | forbid | WP0 | none |
| `eileen-stdx` | lib | neutral | all | forbid | WP0 | contracts |
| `eileen-trace` | lib | neutral | all | forbid | WP0 | contracts, stdx |
| `eileen-rt` | lib | neutral | all async processes | forbid | WP0 | contracts, stdx, trace |
| `eileen-platform` | lib | neutral | all | forbid | WP0 | contracts |
| `eileen-supervisor` | lib | neutral, Windows module | all | `ffi` only | WP0 | contracts, stdx, trace, rt |
| `eileen-buildguard` | lib | neutral, Windows module | tools, core | `ffi` only | WP0 | none (std and `windows-sys` only) |
| `eileen-core` | lib | neutral | core | forbid | WP1 | contracts, stdx, trace (the resolver, dictation cleaner, settings validator and skill engine arrive through ports in `eileen-contracts`) |
| `eileen-record` | lib | neutral | core, tools | forbid | WP1 | contracts, stdx, trace |
| `eileen-replay` | lib | neutral | core, tools | forbid | WP1 | core, record, contracts, stdx, trace |
| `eileen-sim` | lib, dev only | neutral | tests | forbid | WP1 (skeleton WP0) | core, record, replay, intent, dictation, skills, contracts, stdx, trace |
| `eileen-runtime` | lib | neutral | core | forbid | WP2 | core, record, config, mcp, supervisor, rt, contracts, stdx, trace |
| `eileen-mcp` | lib | neutral | core, sidecars, shim | forbid | WP2 | rt, contracts, stdx, trace |
| `eileen-crash` | lib | neutral, Windows module | all | `ffi` only | WP2 | contracts, trace |
| `eileen-shell-win` | lib | Windows | core | `ffi` only | WP2 | platform, contracts, stdx |
| `eileen-scenarios` | lib | neutral | core, tools | forbid | WP2 | record, replay, supervisor, rt, contracts, stdx, trace |
| `eileen-cli` | bin `eileen` | neutral | core | forbid | WP2 | runtime, core, config, harness, audio, learn, skills, intent, dictation, mcp, record, replay, scenarios, fleet, crash, rt, supervisor, stdx, trace; shell-win (Windows target) |
| `eileen-shim` | bin `eileen-shim` | neutral | shim | forbid | WP2 | mcp, rt, contracts, stdx, trace |
| `eileen-guard` | bin `eileen-guard` | neutral, Windows module | guard | none of its own | WP2 | crash, contracts, stdx, trace |
| `eileen-harness` | lib | neutral | core | forbid | WP3 | supervisor, buildguard, rt, contracts, stdx, trace |
| `eileen-stub-harness` | bin, dev only | neutral | tests | forbid | WP3 | harness, contracts, stdx |
| `eileen-audio` | lib | neutral logic, Windows devices | core | forbid | WP4 | platform, rt, contracts, stdx, trace; audio-win (Windows target) |
| `eileen-audio-win` | lib | Windows | core, speech | `ffi` only | WP4 | platform, contracts, stdx |
| `eileen-speech` | lib and bin | neutral logic, Windows models | speech | forbid | WP4 | supervisor, rt, contracts, stdx, trace; audio-win (Windows target) |
| `eileen-dictation` | lib | neutral | core | forbid | WP4 | contracts, stdx |
| `eileen-desktop` | lib and bin | neutral logic | desktop | forbid | WP5 | mcp, platform, supervisor, rt, contracts, stdx, trace; desktop-win (Windows target) |
| `eileen-desktop-win` | lib | Windows | desktop | `ffi` only | WP5 | platform, contracts, stdx |
| `eileen-apps` | lib and bin | neutral logic | apps | forbid | WP6 | mcp, platform, supervisor, rt, contracts, stdx, trace; apps-win (Windows target) |
| `eileen-apps-win` | lib | Windows | apps | `ffi` only | WP6 | platform, contracts, stdx |
| `eileen-intent` | lib | neutral | core | forbid | WP7 | skills, contracts, stdx |
| `eileen-skills` | lib | neutral | core | forbid | WP7 | contracts, stdx |
| `eileen-learn` | lib | neutral | core | forbid | WP7 | skills, record, rt, contracts, stdx, trace |
| `eileen-config` | lib | neutral | core | forbid | WP7 | contracts, stdx, trace |
| `eileen-hud` | lib and bin | neutral layout | hud | forbid | WP8 | rt, contracts, stdx, trace; hud-win (Windows target) |
| `eileen-hud-win` | lib | Windows | hud | `ffi` only | WP8 | platform, contracts, stdx |
| `eileen-fleet` | lib | neutral | core, tools | forbid | WP9 | scenarios, replay, record, supervisor, rt, contracts, stdx, trace |
| `eileen-e2e` | lib and bin, dev only | neutral, Windows fixtures | tests | forbid | WP9 | scenarios, fleet, replay, record, supervisor, mcp, config, rt, contracts, stdx, trace |
| `xtask` | bin, not shipped | neutral | tools | `ffi` through buildguard only | WP0 | buildguard; third-party: `serde_json`, `toml` |
| `tools/repo-tools` | bin, not shipped | neutral | tools | forbid | WP0 | contracts; third-party: `syn`, `serde_json`, `toml` |

What changed from the first draft and why: `eileen-rt` holds the task-spawn wrapper, named threads and bounded channels, so every async crate can obey the spawn rule without depending on the runtime; `eileen-buildguard` holds the build slots, shared by `cargo xtask` and the harness's build-heavy coding jobs; `eileen-replay` (shipped) holds both replay modes and the expectation matcher, so `eileen replay` and the fleet work in the installed product while `eileen-sim` stays development-only; `eileen-scenarios` (shipped) holds save, share, submit and the real-desktop runner, so contributors can share from the first slice; `eileen-fleet` (shipped) holds the fleet runner; `eileen-skills` holds the pure half of the learning loop so the core can call it; `eileen-shell-win` moves the tray, hotkeys and Raw Input into WP2 for slice S1, and `eileen-hud-win` holds the HUD renderer.

Rules the table encodes, each checked by a repository check (section 4b.3):

- `eileen-core`, `eileen-intent`, `eileen-skills`, `eileen-dictation`, `eileen-replay` and `eileen-config` link no platform or vendor native library and nothing that can hang. `eileen-core` performs no IO at all. Vendored C with no IO of its own (bundled SQLite in `rusqlite`, `zstd`) is allowed only in the store and blob modules (`eileen-record::blob`, `eileen-learn::store`, `eileen-runtime::store`), whose calls run on the runtime's blocking pool.
- Only `eileen-mcp` (and the binaries through it) knows rmcp. Only `eileen-harness` knows vendor CLI flags and protocols. Only `eileen-contracts` types are serialized across a process, pipe, file or socket.
- Every crate may depend on `eileen-stdx` (the clock, IDs, checked casts) and every async crate on `eileen-rt`, so the clock, ID and spawn bans hold everywhere.
- The shipped `eileen` binary links no ONNX Runtime and no app automation: `eileen voice ...` runs `eileen-speech.exe voice ...` through the supervisor (or asks the running speech process over its pipe), and `eileen doctor resolve` runs `eileen-apps.exe doctor resolve`.
- `-win` crates, and the Windows modules of `eileen-supervisor`, `eileen-buildguard`, `eileen-crash` and `eileen-guard`, are gated with `#![cfg(windows)]` (or a `cfg(windows)` module), so `cargo clippy --workspace` and the neutral tests run on Linux. Native dependencies appear only under the target tables section 4.1 allows.
- Unsafe code lives only in a private module named `ffi` (`ffi.rs`, or `ffi/` with one file per API area such as `ffi/uia.rs` and `ffi/input.rs`) declared with `#[expect(unsafe_code, reason = "...")]`; one unsafe operation per block, each with a `// SAFETY:` comment.

Non-crate directories and their owners:

| Path | Contents | Owner |
|---|---|---|
| root files, `.cargo/`, `.config/`, `.github/` (workflows, templates, `CODEOWNERS`, `fleet-contributors.txt`), `.githooks/`, `.claude/` (settings, rules, skills) | Workspace, gates, CI, templates, agent settings | WP0 |
| `AGENTS.md`, `CLAUDE.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`, `SECURITY.md`, `NOTICE` | Guidance and policy | WP0 |
| `TESTING.md` | The testers' guide: install, try, save, share, submit, the fleet | WP2 (written from section 4.8 with slice S1) |
| `docs/FLEET.md` | Maintainers' guide to fleet ingest, clustering and triage | WP9 |
| `docs/CODEMAP.md`, `docs/reference/`, `.claude/rules/crate-*.md` | Generated by `cargo xtask codegen`; any package regenerates them, nobody edits them by hand | generator |
| `docs/wp/WP<n>-<topic>.md` | Living work package file | that package |
| `docs/DEBUGGING.md` | Debugging guide (observability report section 7) | WP2 |
| `docs/PLAYBOOK.md`, `templates/` | The project-agnostic engineering playbook and the reusable templates (section 4b.6) | WP0 (later changes through amendments) |
| `fuzz/Cargo.toml`, `fuzz/README.md`, `fuzz/rust-toolchain.toml` | Fuzz workspace with every target from section 4.7 pre-declared | WP0 |
| `fuzz/fuzz_targets/<target>.rs`, `fuzz/corpus/<target>/`, `fuzz/regressions/<target>/` | One target each | the package owning the parser (section 4.7) |
| `scenarios/README.md`, `scenarios/_schema/`, `scenarios/core/` | Scenario format, examples | WP0 |
| `scenarios/<area>/` (with `_fixtures/` templates inside each area) | Scenarios per area | `feel/`, `jobs/`, `first-run/` WP1; `runtime/` WP2; `claude/` WP3; `voice/`, `dictation/` WP4; `desktop/` WP5; `apps/` WP6; `learning/`, `settings/` WP7; `hud/`, `claude-plugin/` WP8; `bench/` WP9 |
| `tests/harness-scripts/` | Recorded stream-json scripts for the stub harness | WP3 (learning scripts live in `crates/eileen-learn/tests/scripts/`, WP7) |
| `adapters/resolve/` | Lua bridge, Fusion and DRX templates, the spike's scripts | WP6 |
| `integrations/claude-plugin/`, `.claude-plugin/marketplace.json` | The `eileen` Claude plugin | WP8 |
| `tools/claude-plugins/eileen-dev/` | Dev plugin for Eileen's own coding workers | WP0 |

`Cargo.lock` is shared. Packages only add entries for dependencies already declared in the workspace. After a merge conflict, take `main`'s lockfile and run `cargo xtask lock` (which runs `cargo update --workspace` and fails if any third-party version changed). Generated files are resolved the same way: take `main`'s copy and run `cargo xtask codegen`.

### 3.2 What stays Python, and the Python scaffold

Nothing in the v0.1 build is Python. The rust-architecture report measured why: the same one-tool adapter answered in 1.7 to 2.9 s as a stdlib-only JSON-RPC shim and in 37 to 69 s with the Python MCP SDK on the loaded PC, and a bare interpreter took 4.5 to 6.9 s to start. Resolve, the one place Python looked unavoidable, runs through a Lua bridge under Resolve's own interpreter, and Chatterbox runs from its ONNX graphs through `ort`. If a later adapter needs Python (Resolve's fusionscript fallback), it is a stdlib-only JSON-RPC shim at startup, run with `python -I` from a uv venv pinned by its own lockfile, long-lived and supervised, never a global install.

The Python scaffold under `src/eileen`, `tests/*.py`, `pyproject.toml` and the Python CI job is retired by WP0 in the pull request that replaces CI (`wp0/ci-bootstrap`, section 6.2), in the order the owner's repository settings require: the new `all-green` check first reports while the Python jobs still run, the owner switches the required checks, and only then does the same pull request delete the scaffold. Its pydantic schemas are superseded by `eileen-contracts`, and `policy/default.yaml` by the TOML defaults. Before deleting, WP0 tags the current `main` commit as `python-scaffold-final`, so the history stays one command away.

### 3.3 Contracts

Everything here lives in `eileen-contracts` unless a path says otherwise. Every serialized type derives `serde` and `schemars`; every wire shape and tool input schema is pinned by an insta snapshot, so any change is a reviewed diff. The crate is split into one module per producing area (`ids`, `jobs`, `core_io`, `bus`, `desktop`, `hud`, `speech`, `harness`, `tools/<toolset>`, `intent`, `config`, `connectors`, `learn`, `scenario`, `record`, `fleet`, `ports`), each with its own snapshot file, so packages that add fields in parallel rarely touch the same file. Maps that reach an effect or a snapshot are `BTreeMap` or `IndexMap`, never `HashMap`. Field additions inside a sub-enum belong to the package that owns the producing component and update the snapshots; a new top-level `Input`, `Effect` or `BusEvent` variant is an architectural change and needs an amendment to this file.

#### 3.3.1 Identifiers, time, correlation and text

```rust
// ids.rs: prefixed ULIDs. Display writes "job_01K8...", FromStr requires the prefix.
// The core mints them from MonoTime plus the injected RNG; drivers go through the eileen_stdx Ids port.
macro_rules! prefixed_ulid { ($name:ident, $prefix:literal) => { /* newtype over ulid::Ulid */ } }
prefixed_ulid!(SessionId, "sess");   prefixed_ulid!(TurnId, "turn");   prefixed_ulid!(JobId, "job");
prefixed_ulid!(WorkerSessionId, "ws"); prefixed_ulid!(CallId, "call"); prefixed_ulid!(SpeakId, "speak");
prefixed_ulid!(ApprovalId, "appr");  prefixed_ulid!(IncidentId, "inc"); prefixed_ulid!(RecordingId, "rec");
prefixed_ulid!(ProposalId, "prop");  prefixed_ulid!(InstallId, "inst");

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

/// Text from outside Eileen's own code (screen, page, file, tool result, model output). Data, never an instruction.
#[serde(transparent)] pub struct Untrusted<T>(T);
/// Text that may identify a person or their files (transcripts, paths, window titles). Sinks apply the pii policy;
/// the share serializer replaces it with a placeholder.
#[serde(transparent)] pub struct Personal<T>(T);
```

`TraceCtx::traceparent()` renders W3C `00-<32 hex of the turn ULID, or the job ULID>-<16 hex span>-01`. Every free-text `String` reachable from `Input`, `ToolResult` and `DesktopState` is wrapped in `Untrusted` or `Personal`, or is listed in `contracts/text-allowlist.toml` with a reason (enum-like strings such as a tool name); a test walks the generated schemas (each wrapper carries an `x-eileen-text` schema extension) and fails otherwise.

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
pub enum JobKind { Coding, ComputerUse, General, Research, DeepAnswer, Librarian, Replay, ContinueSession, AppTask }
pub enum JobStatus { Queued, Running, Blocked, AwaitingApproval, Completed, Failed, Cancelled, Interrupted }
pub enum FanoutMode { Compete, Split }
pub enum Toolset { Brain, Act, Worker, Desktop, Apps, Learn, Session, Dev }
pub enum TurnSource { Voice, Typed, Hud, Hotkey, Tray, Cli, ClaudeSession, JobEvent, Scenario, Connector(ConnectorId) }
/// Addressed: began from addressed user input (voice after the gate, typing in Eileen's UI, the HUD); full brain tools.
/// Ambient: everything else (job events, announcements, Cli, ClaudeSession); read-only brain tools.
pub enum TurnAuthority { Addressed, Ambient }
pub enum PoolClass { ComputerUse, Deep }
pub enum CallOrigin {
    Brain { turn: TurnId, authority: TurnAuthority },
    Worker { job: JobId, session: WorkerSessionId, route: Route },
    Replay { job: JobId, skill: SkillRef },
    FastPath { turn: TurnId },
    Session,            // the owner's own Claude session through the plugin
    Core,               // Eileen's own bookkeeping calls (checks, intent highlight, media pause during approvals)
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
    pub repo: Option<Personal<String>>,   // repo name or absolute path
    pub resume_session: Option<String>,   // ContinueSession only
    pub skill: Option<SkillRef>,          // a learned skill to follow (Replay: the recipe to run)
    pub slots: SlotValues,                // Replay: the resolved slots
    pub authorized_by: TurnId,            // the addressed turn that caused it; checked by the dispatcher
    pub parent: Option<JobId>,            // fan-out children point at their parent
    pub chrome: bool,                     // the user opted this job in to Claude in Chrome
}

pub struct Job {
    pub id: JobId, pub name: String, pub slug: String, pub ordinal: Option<u8>, pub spec: JobSpec,
    pub status: JobStatus, pub route: Route, pub model: String, pub session: Option<WorkerSessionId>,
    pub created: WallTime, pub updated: WallTime,
    pub last_progress: Option<Untrusted<String>>, pub progress_pct: Option<u8>,
    pub salience_pct: u8,                 // drives announcement order
    pub worktree: Option<Personal<String>>, pub branch: Option<String>, pub snapshot_ref: Option<String>,
    pub cost: Cost, pub grants: Vec<Grant>, pub taint: Taint, pub focus: bool,
    pub override_words: Option<Personal<String>>,   // the spoken override that routed it, verbatim
    pub approval_deadline: Option<WallTime>,
}

pub struct Cost { pub duration_ms: u64, pub api_ms: u64, pub model_turns: u32,
                  pub tokens_in: u64, pub tokens_out: u64, pub tokens_cache_read: u64 }
pub struct Grant { pub matcher: GrantMatcher, pub tier: Tier, pub scope: GrantScope, pub granted_by: ApprovalId, pub summary: String }
pub enum GrantMatcher {
    ExactCall { tool: String, input_digest: String },   // the normalised call that was shown to the user
    ToolPattern { pattern: String },                    // tier 1 only
    DesktopApp { process: String },                     // "May Claude control Resolve?"
}
pub enum GrantScope { ThisJob, Always /* tier 1, or tier 2 with a logged decision id */ }
pub struct Taint { pub untrusted_seen: bool, pub private_read: bool }
pub struct PlanUsage { pub five_hour_pct: Option<u8>, pub seven_day_pct: Option<u8>, pub resets_at: Option<WallTime> }
pub struct MemoryPressure { pub free_mb: u32, pub commit_pct: u8, pub largest_process_mb: u32 }
```

#### 3.3.3 Inputs and effects of the core

The core is a state machine with one entry point. Drivers turn the outside world into `Input` and carry out `Effect`; they hold no decisions.

```rust
// eileen-core
impl Core {
    pub fn new(config: CoreConfig, ports: CorePorts, snapshot: Option<CoreSnapshotV1>) -> Self;
    pub fn step(&mut self, now: MonoTime, rng: &mut CoreRng, input: Envelope<Input>, effects: &mut Vec<Envelope<Effect>>);
    pub fn snapshot(&self) -> CoreSnapshotV1;   // a contracts type (3.3.16), for checkpoints and replay
}
pub struct CorePorts {                           // section 3.3.15
    pub resolver: Box<dyn IntentResolver>, pub cleaner: Box<dyn DictationCleaner>,
    pub settings: Box<dyn SettingsValidator>, pub skills: Box<dyn SkillEngine>,
}
```

```rust
pub enum Input {
    Voice(VoiceEvent),
    Playback(PlaybackEvent),
    Brain(BrainEvent),
    Worker(WorkerSessionId, WorkerEvent),
    Hook(WorkerSessionId, HookRequest),
    ToolCall(ToolCallRequest),            // from a shim (caller verified by PID and Job Object), a replay or the core
    ToolResult(CallId, ToolResult),       // from a sidecar or the core-side executor
    Sidecar(SidecarId, SidecarEvent),
    Learn(LearnEvent),
    User(UserInput),                      // hotkeys, HUD, tray, typed text, Cli; never screen or model text
    Activity(ActivityEvent),
    Memory(MemoryPressure),               // every 5 s
    Settings(SettingsEvent),
    Store(StoreEvent),
    Timer(TimerId),
}

pub enum VoiceEvent {
    SpeechStarted { turn: TurnId, source: u16 },
    Partial { turn: TurnId, text: Personal<String>, stable: bool },
    SpeechEnded { turn: TurnId },
    EndOfTurn { turn: TurnId, text: Personal<String>, smart_turn_pct: u8, silence_ms: u32,
                asr_confidence_pct: u8, reason: TurnEndReason, source: u16 },
    BargeInWindow { speak: SpeakId, window: u8, prob_pct: u8 },     // only while Eileen speaks
    ClauseReady { speak: SpeakId, clause: u16, chars: u32, samples: u32 },
    SpeakDone { speak: SpeakId },
    Embedding { request: u64, vector: Vec<f32> },
    CheckResult { request: u64, result: SpeechCheck },             // name recognisability, speaker match, clone timing
    SpeakerMatch { turn: TurnId, score_pct: u8 },                  // only if the speaker check is adopted and enrolled
    Health { rtf_by_engine: BTreeMap<String, u16> },               // per-mille real-time factor
}
pub enum PlaybackEvent {
    Played { speak: SpeakId, clause: u16, samples_played: u32 },   // at most every 50 ms
    Paused { speak: SpeakId, clause: u16, samples_played: u32 },
    Stopped { speak: SpeakId, clause: u16, samples_played: u32 },
    Drained { speak: SpeakId },
    DeviceChanged, Stalled,
}
pub enum BrainEvent {
    Ready { which: BrainSlot, model: String },
    TextDelta { turn: TurnId, text: Untrusted<String> },
    TurnDone { turn: TurnId, cost: Cost, context_tokens: u64, is_error: bool },
    RateLimit(PlanUsage),
    Exited { which: BrainSlot, code: Option<i32> },
}
pub enum WorkerEvent {
    Init { session_id: String, pid: u32, model: String, tools: Vec<String>, mcp_servers: Vec<String>,
           plugins: Vec<String>, capabilities: Vec<String> },
    Narration { line: Untrusted<String> },
    Text { delta: Untrusted<String> },
    ToolUse { tool_use_id: String, name: String, input: Untrusted<serde_json::Value> },   // redacted, at most 2 KB
    ToolResult { tool_use_id: String, ok: bool },
    SkillInvoked { name: String, args: Untrusted<String> },
    TurnDone { cost: Cost, is_error: bool, summary: Untrusted<String> },
    RateLimit(PlanUsage),
    AuthFailed { message: String },       // parsed from the result event, not the exit code
    Malformed { line_len: u32 },
    Exited { code: Option<i32> },
}
pub enum HookRequest {
    PreToolUse { request: u64, tool_name: String, tool_input: Untrusted<serde_json::Value>, tool_use_id: String,
                 paths: Vec<PathFact> },  // added by the runtime: each path argument canonicalised and classified
    PermissionRequest { request: u64, tool_name: String, tool_input: Untrusted<serde_json::Value>, tool_use_id: String },
    Notification { kind: String, message: Untrusted<String> },
    PermissionDenied { tool_name: String }, PostToolUseFailure { tool_name: String, error: Untrusted<String> },
    SubagentStart, SubagentStop, Stop { last_message: Option<Untrusted<String>> }, StopFailure { error: String },
}
pub struct PathFact { pub arg: String, pub class: PathClass, pub canonical: Personal<String> }
pub enum PathClass { JobFolder, JobTemp, SkillSnapshot, Toolchain, EileenData, Credential, BrowserProfile, Elsewhere }
pub struct ToolCallRequest {
    pub caller: Caller,                    // which shim session or internal origin, to route the reply
    pub origin: CallOrigin,
    pub tool: ToolName,
    pub args: Untrusted<serde_json::Value>,
    pub traceparent: Option<String>,
}
pub struct ToolResult { pub content: Vec<Content>, pub is_error: bool, pub verification: Option<Verification>,
                        pub duration_ms: u32, pub rung: Option<Rung> }
pub enum Content { Text(Untrusted<String>), Image { mime: String, base64: String } }   // never structured content beside an image
pub struct Verification { pub kind: String, pub observed: Untrusted<serde_json::Value> }
pub enum Rung { AppApi, AppMcp, DeepLink, AppActions, Uia, Dom, Vision }
pub enum SidecarEvent { Ready { manifest: ConnectorManifest }, Heartbeat, Event(ConnectorEvent), Exited { code: Option<i32> } }
pub enum ConnectorEvent { Desktop(DesktopDelta), Media(MediaState), App { app: String, event: AppEvent } }
pub enum AppEvent { RenderProgress { pct: u8 }, RenderDone { ok: bool }, VersionChanged { version: String }, Other(Untrusted<serde_json::Value>) }
pub enum ActivityEvent {
    PhysicalInput,                        // from Raw Input; at most one per 250 ms; carries no key or position
    OtherAudio { playing: bool },         // loopback energy Eileen's own stream does not explain, or SMTC Playing
    MicInUseElsewhere(bool),
    SessionLocked(bool),
    Presentation(PresentationState),
}
pub enum LearnEvent {
    SkillsChanged { skills: Vec<SkillMeta>, recipes: Vec<Recipe>, phrasings: Vec<PhrasingFact>, facts: Vec<UserFact> },
    SnapshotWritten { session: WorkerSessionId, manifest: SnapshotManifest },
    Proposed { skill: SkillId, outcome: ProposeOutcome },   // committed, pending approval, rejected with lint errors
    LibrarianFinished { cost: Cost, distilled: u16 },
    Forgot { report: ForgetReport },
}
pub enum UserInput {
    SetMute(bool), ToggleMute, Approve(ApprovalId), Deny(ApprovalId), CancelJob(JobId), PauseDesktopJobs,
    Typed { text: Personal<String> }, Cli { text: Personal<String> },
    Hud { input: HudInput, physical: bool }, DictationKey(KeyEdge), SaveIncident, SaveScenario,
}
pub enum SettingsEvent { FileChanged { valid: bool, error_line: Option<u32> }, ChecksDone { proposal: ProposalId, checks: Vec<CheckResult> } }
pub enum StoreEvent { Loaded { jobs: Vec<Job>, settings_version: u64 } }
```

```rust
pub enum Effect {
    Speech(SpeechCmd),          // Speak { speak, voice, clauses, whole_line }, Cancel { speak }, Resume { speak }, SetMode(ListenMode),
                                // SetHotwords, Embed { request, text }, Check { request, check }, LoadModel, UnloadModel,
                                // Enroll { kind }, CloneFromRecording { voice }, DeleteVoice { name }
    Audio(AudioCmd),            // SetCapture(bool), Duck { db }, Unduck, Pause { speak }, Resume { speak }, Stop { speak },
                                // PlayPhrase { key, voice }, Earcon(Earcon), Record { seconds, purpose }
    Brain(BrainCmd),            // Turn { turn, message, authority }, Interrupt, Recycle { summary }
    Worker(WorkerSessionId, WorkerCmd),   // Start(SessionSpec), Bind(JobBinding), Prompt { text }, Interrupt, Cancel,
                                          // Recycle, RemoveWorktree, RestoreSnapshot { snapshot_ref }
    Hook(WorkerSessionId, HookReply),     // PreToolUse { request, decision: HookDecision },
                                          // PermissionRequest { request, decision: PermissionDecision }
    ToolForward { call: CallId, connector: ConnectorId, tool: ToolName, args: serde_json::Value, deadline_ms: u32 },
    ToolReply { caller: Caller, call: CallId, result: ToolResult },
    Sidecar(SidecarId, SidecarCmd),   // Start, Restart, HangDumpThenKill
    Hud(HudState),              // full snapshot; the driver rate-limits to 10 per second
    Learn(LearnCmd),            // Begin, Step, Signal, Finish, WriteSnapshot { session, select }, RunLibrarian { batch },
                                // Commit { doc, reason }, Forget(ForgetPlan), Rollback, FactDelta(FactDelta)
    Settings(SettingsCmd),      // RunChecks { proposal }, WriteVersion(SettingsVersion), ApplyLive(Vec<SettingKey>)
    Persist(Record),            // JobUpsert, EventAppend, ApprovalRow, JournalRow, SettingsVersion, PlanUsage
    Emit(BusEvent),
    SetTimer { id: TimerId, at: MonoTime }, CancelTimer(TimerId),
    Incident(IncidentTrigger),
    Notify(Toast),
}
pub enum HookDecision { None, Ask { reason: String }, Deny { reason: String } }   // never Allow
pub enum PermissionDecision { Allow { approval: ApprovalId }, Deny { reason: String } }   // Allow only from a UserInput approval
```

#### 3.3.4 Bus events

Published to the HUD, the loopback endpoint, the Claude plugin and the `events` table, each in an `Envelope`. Speech that was not addressed to Eileen is not a bus event: it reaches only the HUD state while it is current.

```rust
pub enum BusEvent {
    TurnStarted { source: TurnSource, authority: TurnAuthority },
    Heard { text: Personal<String>, is_final: bool },          // addressed or awaiting a decision only
    TurnRouted { route: TurnRoute, reason: String },           // FastPath, Act, Replay, Brain, Clarify, Ignored, Dictation
    Speaking { speak: SpeakId, cause: SpeakCause, text: String },
    SpeakingStopped { speak: SpeakId, heard_chars: u32, interrupted: bool, resumed: bool },
    MuteChanged { muted: bool },
    ConversationWindow { open: bool, until_ms: u32 },
    BrainReady { model: String },
    JobAccepted { job: JobId, name: String, ordinal: Option<u8>, kind: JobKind, route: Route, model: String, reason: String },
    JobQueued { job: JobId, reason: QueueReason },             // Memory, BuildSlot, Plan
    JobStarted { job: JobId, warm: bool },
    JobNarration { job: JobId, line: Untrusted<String> },
    JobProgressed { job: JobId, line: Untrusted<String>, pct: Option<u8> },
    JobBlocked { job: JobId, reason: String },
    ApprovalRequested { approval: ApprovalId, job: Option<JobId>, tier: Tier, summary: String, voice: VoiceApproval,
                        deadline: WallTime },
    ApprovalResolved { approval: ApprovalId, allowed: bool, by: ApprovalSource },
    ApprovalExpired { approval: ApprovalId },
    JobCompleted { job: JobId, summary: Untrusted<String>, artifacts: Vec<Personal<String>> },
    JobFailed { job: JobId, error: String },
    JobCancelled { job: JobId, by: CancelSource },
    JobInterrupted { job: JobId },
    DesktopIntent { rect: Rect, label: String, until_ms: u32 },
    InputLock { holder: Option<JobId> },
    ForegroundLease { job: JobId, seconds_left: u16 },
    UserTookOver { job: JobId },
    DesktopJobsPaused { paused: bool },
    PlanUsage(PlanUsage),
    Learn(LearnBusEvent),       // drafted, pending approval, promoted, demoted, forgot, replay step, replay handoff
    SettingChanged { key: String, from: String, to: String, version: u64 },
    FirstRun(FirstRunPhase),
    RecordingChanged { on: bool },
    IncidentCaptured { id: IncidentId },
    SidecarRestarted { which: SidecarId, reason: String },
    Lagged { missed: u64 },
}
pub enum VoiceApproval { ActionWord { word: String }, HudOnly { reason: HudOnlyReason } }   // OtherAudio, MicInUse, Unparsed, Fleet
```

`JobNarration`, `JobCompleted` and friends replace the old `Task*` names. Event names in the past tense, commands in the imperative. The snake_case dotted names used in scenarios and timing budgets (`turn.ended`, `speaking.started`, `job.completed`) are generated from these variants and the span constants into `docs/reference/events.md`, and `cargo xtask scenarios check` rejects any name not in it.

#### 3.3.5 DesktopState

The October version of the old ARCHITECTURE section 2 object (physical pixels, virtual-screen coordinates):

```rust
pub struct DesktopState {
    pub captured_at: WallTime,
    pub monitors: Vec<Monitor>,          // ordinal, device_path, friendly_name, rect, work_rect, dpi, is_primary
    pub windows: Vec<WindowInfo>,        // hwnd (i64), pid, process, exe_version, title (Untrusted), class, monitor_ordinal,
                                         // frame_rect, z_order, is_minimized, is_cloaked, is_foreground, is_eileen, is_private
    pub foreground: Option<WindowRef>,
    pub focused_element: Option<ElementInfo>, // control_type, name (Untrusted), automation_id, rect, patterns, is_password, window
    pub selection: Option<Selection>,    // text (at most 4,000 chars, Untrusted), source: TextPattern | ClipboardFallback
    pub caret: Option<Rect>,
    pub recent_references: Vec<Reference>,   // at most 8, newest first
    pub presentation: PresentationState, // Normal, Busy, FullScreen, Presentation, QuietHours
    pub mic_in_use_elsewhere: bool,
    pub now_playing: Option<MediaState>, // merged in by the core from pc.apps
    pub degraded: Vec<String>,           // fields that timed out or are stale: unknown, not empty
}
pub struct DesktopDelta { pub seq: u64, pub changes: Vec<DesktopChange> }
```

`eileen_contracts::desktop::sanitize(&str, max_chars) -> Untrusted<String>` is the only way text from UIA enters a contract type. `selection` with `source = ClipboardFallback` is filled only when the request refers to something copied, never from clipboard data marked for exclusion from monitoring or history (section 2.6), and always after the Redactor.

#### 3.3.6 HUD state

```rust
pub struct HudState {
    pub v: u16,
    pub layout: HudLayout,               // corner, scale_pct, opacity_pct, show_transcript, high_contrast
    pub mic: MicState,                   // LoadingSpeech, Listening, Hearing { level_pct }, Muted, Dictating { target }, SpeechRestarting
    pub conversation: Option<u16>,       // seconds left in the conversation window
    pub heard: Option<HeardLine>,        // text, is_final, addressed: Option<bool>, turn (for "that was for you")
    pub speaking: Option<String>,
    pub jobs: Vec<HudJob>,               // at most 8: id, name, ordinal, route display name, status, line, pct, focus, paused
    pub approvals: Vec<HudApproval>,     // id, summary, tier, seconds_left_voice, deadline, voice (ActionWord | HudOnly)
    pub intent: Option<HudIntent>,       // rect, label
    pub input_lock: Option<String>,      // job name holding the mouse and keyboard
    pub lease: Option<HudLease>,         // job, seconds_left; or took_over
    pub watch: Option<HudWatch>,         // hwnd (i64), label: the live thumbnail of a window a job works in
    pub panel: Option<HudPanel>,         // Markdown (the brain's ---hud--- part) | Learned | Facts | ScenarioPreview
                                         // | Confirm { image: Option<BlobRef>, question, options, id } | ContactSheet { items, id }
                                         // | Diff { job, stat, tests } | Settings(SettingsPanel)
    pub suggestions: Vec<String>,        // at most 3
    pub first_run: Option<FirstRunView>, // choices, consent checkbox state, progress, reveal
    pub indicators: Indicators,          // learning, recording, off_the_record, plan_usage_pct, brain_warming,
                                         // rendering_voice, low_memory, desktop_jobs_paused
}
pub enum HudInput {
    ToggleMute, Approve(ApprovalId), Deny(ApprovalId), CancelJob(JobId), PauseDesktopJobs, ContinueDesktopJobs,
    SaveScenario, DismissPanel, ApproveSkillLine { skill: SkillId, line: u32 },
    Answer { id: u64, option: u8 },      // Confirm panels, contact sheets, clarifying questions
    RunHeardLine { turn: TurnId },       // "that was for you" by tap
    SetSetting { key: String, value: serde_json::Value },   // goes through the normal settings flow
    FirstRun { choice: FirstRunChoice, consent_own_voice: bool },   // Tour | Skip | Surprise
    DropFile { path: Personal<String> }, // an audio clip dropped on the strip: starts voice enrollment
}
```

Wire: core to HUD `{"v":1,"state":{...}}` per line; HUD to core `{"v":1,"input":{...},"physical":true}` per line; at most 64 KiB per line. `physical` is false when the click arrived with no Raw Input device handle; the core ignores non-physical approvals, mute changes and first-run consent.

#### 3.3.7 The speech pipe

Named pipe `\\.\pipe\eileen-speech-<session>` (Unix socket under `$XDG_RUNTIME_DIR` on Linux tests), user-only ACL, created by the core, opened by `eileen-speech` with a token from its environment; the core checks the client process is the `eileen-speech.exe` it spawned.

```rust
#[repr(C)] pub struct FrameHeader { pub kind: u8, pub flags: u8, pub source: u16, pub len: u32, pub seq: u64, pub turn: [u8; 16] }
// source 0 is the PC microphone; later audio connectors (earbuds, a phone) get their own number
// kind 1 Audio   core -> speech  512 samples of i16, 16 kHz mono (one 32 ms window), len = 1024
// kind 2 Command core -> speech  JSON { ctx: TraceCtx, span: u64, cmd: SpeechCmd }
// kind 3 Event   speech -> core  JSON { ctx: TraceCtx, span: u64, event: VoiceEvent }
// kind 4 Pcm     speech -> core  JSON PcmHeader { speak, clause, sample_rate } then i16 samples, at most 1 MiB
// kind 5 Ping, kind 6 Pong       heartbeat every 1 s
```

`len` is validated against the kind's maximum before any allocation (fuzz target `speech_frame`). VAD, ASR and barge-in state are keyed by `source`. PCM goes straight to the audio driver; the core only sees `ClauseReady`, `Played` and `Paused`.

#### 3.3.8 Worker and brain drivers

```rust
// eileen-harness
pub trait WorkerDriver: Send {
    fn route(&self) -> Route;
    /// Spawn a session (pooled or for one job) and wait for its init event.
    fn start(&mut self, spec: SessionSpec) -> impl Future<Output = Result<WorkerSession, DriverError>> + Send;
    /// Give a started session its job: the first user turn.
    fn bind(&mut self, binding: JobBinding) -> impl Future<Output = Result<(), DriverError>> + Send;
    fn send(&mut self, cmd: WorkerCmd) -> impl Future<Output = Result<(), DriverError>> + Send;
    /// None when the session has ended and every event has been delivered.
    fn next_event(&mut self) -> impl Future<Output = Option<WorkerEvent>> + Send;
}
pub enum AnyWorker { Claude(ClaudeWorker), Codex(CodexWorker), Acp(AcpWorker) }   // enum dispatch, no dyn
pub fn worker_for(route: Route, cfg: &HarnessConfig) -> Result<AnyWorker, DriverError>; // RouteUnavailable unless built

pub struct SessionSpec {
    pub session: WorkerSessionId, pub pool: Option<PoolClass>, pub kind: JobKind,
    pub route: Route, pub model: String, pub effort: Option<Effort>,
    pub cwd: String, pub worktree: Option<WorktreeSpec>, pub snapshot_ref: Option<String>,
    pub append_system_prompt: String,
    pub toolsets: Vec<Toolset>, pub allowed_tools: Vec<String>, pub extra_mcp_servers: Vec<ApprovedMcpServer>,
    pub plugin_dirs: Vec<String>,         // the session's learned-skill snapshot first
    pub resume_session: Option<String>, pub chrome: bool,
    pub env: EnvPolicy,                   // WorkerDenylist or ConstructedAllowlist (section 2.12)
    pub build_slot: Option<BuildSlotTicket>,   // held for build-heavy coding jobs
    pub trace: TraceCtx, pub hook_endpoint: HookEndpoint,
    pub shim: ShimEndpoint,               // pipe name and per-session token for the generated MCP config
}
pub struct JobBinding {
    pub job: JobId, pub kind: JobKind, pub task: String, pub skill: Option<SkillRef>,
    pub inline_skill: Option<String>,     // at most 4 KB when the snapshot lacks the skill
    pub facts: Vec<String>,               // at most about 400 tokens, scoped to the repo and app
    pub trace: TraceCtx,
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
| Start | Spawn `claude -p` with the flags of section 2.5 through the supervisor; wait for `system/init`; read `session_id`, model, tools, MCP status, plugins, `capabilities` | `initialize`, `initialized`, `thread/start` (`baseInstructions`, `developerInstructions`, `model`, `approvalPolicy`, `ephemeral`) | `initialize`, `session/new` with stdio `mcpServers` (every ACP agent must accept stdio MCP) |
| Bind a job | The first stream-json `{"type":"user",...}` line, with the task, "Follow skill eileen-learned:<id>" and the facts block | `turn/start` | `session/prompt` |
| User turn | One stream-json user line | `turn/start` | `session/prompt` |
| Steer | Interrupt, then a user turn | `turn/steer` | `session/cancel`, then `session/prompt` |
| Interrupt | `control_request` subtype `interrupt` (feature-detected from `system/init`, verified against the real CLI by WP3) | `turn/interrupt` | `session/cancel` |
| Cancel | Kill the Job Object | Kill the Job Object | Kill the Job Object |
| Approvals | `PreToolUse` HTTP guard answers `ask`; the user is reached through the route WP3's contract test picks (the `PermissionRequest` hook, `--permission-prompt-tool`, or deny then steer, section 2.5) | `item/commandExecution/requestApproval`, `item/fileChange/requestApproval` answered by the core | `session/request_permission` answered by the core |
| Tool events | `assistant` tool_use blocks, `user` tool_result blocks, hook events; a `Skill` tool_use becomes `SkillInvoked` | `item/*` notifications | `session/update` |
| Narration | Text lines starting with `> ` | Same convention in agent messages | Same convention |
| Cost and usage | `result` event (`duration_ms`, `duration_api_ms`, `num_turns`, `usage`) | turn completion | n/a |
| Rate limits | Rate-limit stream events | `account/rateLimits/read` | n/a |
| Eileen's tools | Generated `--mcp-config` with the shim (and approved third-party servers), `--strict-mcp-config` | MCP server config at `thread/start` | stdio `mcpServers` in `session/new` |
| Learned skills | `--plugin-dir run\<session>\plugin` | Unverified (Codex not installed); check when added | Per agent |
| Sign-in check | `claude auth status` exit code; auth failures also parsed from the `result` event | `account/read` | agent's own status command |

Every vendor process is spawned by `eileen_supervisor` in a Job Object; the ACP crate's own `process` feature is never used (it does not job-wrap on Windows), and the Codex types are generated from `codex app-server generate-json-schema` when that driver is built.

#### 3.3.9 MCP tools

One MCP server name, `eileen`, served by `eileen-shim` and forwarded to the core's dispatcher, so Claude sees `mcp__eileen__<tool>`. Each session's hello names its role, the dispatcher verifies the caller (section 2.12), and the core answers with the toolsets for that role; `tools/list` shows only those.

| Toolset | Who gets it | Tools |
|---|---|---|
| `brain` | the brain | `jobs_dispatch`, `jobs_status`, `jobs_control`, `desktop_state`, `desktop_look`, `capabilities`, `settings_get`, `settings_propose`, `settings_apply`, `settings_undo`, `learned_query`, `learned_forget`, `learned_correct`, `ledger_search`, `scenario_save`, `ask_user`, `hud_show` |
| `act` | the brain (addressed turns only) | `desktop_launch_app`, `desktop_window`, `desktop_volume`, `apps_open_url`, `apps_open_uri`, `apps_open_path`, `apps_media`, `apps_recent`, `apps_resolve` (navigation ops only) |
| `worker` | every worker | `ask_user`, `report`, `routes_propose`, and `approve` only if WP3's contract test picks the permission-prompt-tool route |
| `desktop` | computer-use and general workers | the 18 `desktop_*` tools of section 2.6 |
| `apps` | computer-use and general workers | `apps_media`, `apps_open_url`, `apps_open_uri`, `apps_open_path`, `apps_resolve`, `apps_office`, `apps_explorer`, `apps_recent`, `apps_image`, `apps_photoshop` |
| `learn` | the librarian only | `learn_trajectory_read`, `learn_skill_list`, `learn_skill_read`, `learn_skill_history`, `learn_skill_propose`, `learn_fact_propose` |
| `session` | the owner's own Claude sessions, through the plugin | `session_status`, `session_jobs`, `session_say`, `session_mute`, `session_handoff`, `session_ask` |
| `dev` | coding workers in an Eileen worktree | `dev_codemap`, `dev_crate_doc`, `dev_event_map`, `dev_trace_get`, `dev_logs_query`, `dev_incident_list`, `dev_incident_read`, `dev_replay_core`, `dev_scenario_list` |

In an ambient brain turn (section 2.3) the dispatcher accepts only `jobs_status`, `desktop_state`, `hud_show` and `learned_query`.

Built-in tools per job kind (`--allowedTools`, before the tier guard narrows them): coding and continue_session get `Read`, `Grep`, `Glob`, `Edit`, `Write`, `Bash`, `PowerShell`, `WebSearch`, `WebFetch` and `mcp__eileen__*`; general gets the same plus the desktop and apps toolsets; computer_use gets `Read` and `mcp__eileen__*`; research and deep_answer get `Read`, `WebSearch`, `WebFetch` and `mcp__eileen__*`; the librarian gets `--tools ""` and `mcp__eileen__learn_*`. `WebFetch` is narrowed to a domain rule when the task names a site. No kind gets `Task` or other subagent tools in v0.1.

Every tool has a `ToolSpec`:

```rust
pub struct ToolSpec {
    pub name: ToolName, pub connector: ConnectorId, pub toolset: Toolset,
    pub description: String,             // at most 600 characters, written for the model
    pub ops: Vec<OpSpec>,                // one entry named "" for tools without ops
    pub timeout_ms: u32,
    pub input_schema: serde_json::Value, // generated by schemars from the argument struct below
    pub schema_hash: String,             // BLAKE3 of input_schema; recipes record it
    pub rung: Rung,                      // the route this tool represents
}
pub struct OpSpec {
    pub op: String,
    pub tier: TierRule,                  // Fixed(Tier) | ByArgs (the server computes it per call)
    pub read_only: bool,                 // usable as a recipe check (tier 0 only)
    pub idempotent: bool,                // a recipe may retry it once
    pub needs_foreground: bool,          // takes the input lock and a lease
    pub preview_before: bool,            // show the target or crop and ask before running (hard-to-undo edits)
    pub inverse: Option<String>,         // how the journal reverses it ("load_version", "restore window rect", ...)
}
```

Argument structs (the JSON Schemas are generated from these and snapshot-tested; `hwnd` is an `i64` because `HWND` is not `Send`; coordinates are physical virtual-screen pixels):

```rust
// brain toolset
pub struct JobsDispatch { pub kind: JobKind, pub task: String, pub title_hint: Option<String>, pub depth: Option<Depth>,
    pub fanout: Option<u8>, pub mode: Option<FanoutMode>, pub subtasks: Vec<String>, pub subquestions: Vec<String>,
    pub repo: Option<String>, pub route: Option<Route>, pub model: Option<String>,
    pub skill: Option<SkillId>, pub slots: Option<serde_json::Value> }   // kind Replay with a level-3 skill runs its recipe
    // returns { jobs: [{ job, name, ordinal, queued }], ack: String }    tier 1 (asks first when plan usage is high and fanout > 2)
pub struct JobsStatus { pub job: Option<JobRef> }                       // JobRef: id, slug, name or ordinal; None = all live jobs. Tier 0
pub struct JobsControl { pub job: JobRef, pub action: JobAction }
    // Cancel | Interrupt | Steer { text } | Reroute { route, model, depth } | Focus | ShowChanges | Keep | Undo. Tier 1, Keep with cleanup tier 2
pub struct DesktopStateArgs { pub detail: Detail, pub since_seq: Option<u64> }   // Summary | Full. Tier 0
pub struct DesktopLook { pub target: LookTarget, pub max_edge_px: Option<u32> }  // Screen | Foreground | Window { hwnd }; default 1280. Tier 0
pub struct Capabilities { pub scope: CapabilityScope }                   // FocusedApp | Everything. Tier 0
pub struct SettingsGet { pub keys: Vec<String> }                         // empty: a short summary. Tier 0
pub struct SettingsPropose { pub changes: Vec<SettingChange>, pub utterance: String }   // returns a Proposal (3.3.11). Tier 0
pub struct SettingsApply { pub proposal: ProposalId }                    // tier per the proposal's risk; Safety keys need a UserInput approval
pub struct SettingsUndo { pub steps: Option<u8>, pub to_version: Option<u64> }           // tier 1
pub struct LearnedQuery { pub about: Option<String>, pub kind: Option<LearnedKind>, pub since: Option<String>, pub limit: Option<u8> }  // Skills | Facts | Both. Tier 0
pub struct LearnedForget { pub target: ForgetTarget }                    // Skill { id } | Fact { id } | About { text } | LastUsed | Job { job }. Tier 2
pub struct LearnedCorrect { pub target: CorrectTarget, pub instruction: String }   // Fact { id } | Skill { id }. Tier 1
pub struct LedgerSearch { pub query: String, pub since_hours: Option<u32>, pub limit: Option<u8> }      // tier 0
pub struct ScenarioSave { pub which: SaveTarget, pub failed: bool, pub expected: Option<String> }      // Last | Turn { turn } | Job { job }. Tier 1
pub struct AskUser { pub question: String, pub options: Vec<String>, pub timeout_s: Option<u16>, pub image: Option<BlobRef> }
    // at most 140 chars, 4 options. Tier 0. Refused when it asks for a password, PIN, one-time code or card number
pub struct HudShow { pub markdown: String, pub title: Option<String> }  // at most 8 KiB. Tier 0

// worker toolset
pub struct Report { pub summary: String, pub artifacts: Vec<String>, pub outcome: ReportOutcome }      // Done | Blocked | Failed
pub struct RoutesPropose { pub app: String, pub kind: RouteKind, pub name: String, pub source_url: String, pub why: String }
    // AppMcp | AppApi | DeepLink. Records a proposal the user can accept by voice or HUD; installs nothing. Tier 0
pub struct Approve { pub tool_name: String, pub input: serde_json::Value, pub tool_use_id: String }
    // only if WP3's test picks the permission-prompt-tool route; accepted only for a pending ask with this tool_use_id;
    // reply shape recorded from the real CLI by WP3

// desktop toolset
pub struct DesktopWindows { pub include_minimized: bool }                                             // tier 0
pub struct DesktopTree { pub hwnd: i64, pub query: Option<String>, pub max_depth: Option<u8>, pub max_elements: Option<u16> }   // 12 and 250 by default. Tier 0
pub struct DesktopScreenshot { pub target: ShotTarget, pub marks: bool, pub max_edge_px: Option<u32> }  // Screen | Monitor { ordinal } | Window { hwnd } | Region { rect }. Tier 0
pub struct DesktopClickElement { pub hwnd: i64, pub element: u32, pub selector: Option<Selector> }    // tier 1, or 2 for send-like names
pub struct DesktopSetValue { pub hwnd: i64, pub element: u32, pub value: String }                    // tier 1; refuses password fields
pub struct DesktopTypeText { pub hwnd: i64, pub element: Option<u32>, pub text: String }             // tier 1 (2 in terminals and unknown targets); foreground
pub struct DesktopPressKeys { pub hwnd: i64, pub keys: String }                                       // "ctrl+s"; tier 1, ByArgs (Enter in mail or chat 2, blocked combos 3)
pub struct DesktopClick { pub hwnd: i64, pub x: i32, pub y: i32, pub button: MouseButton, pub count: u8 }   // tier 1, ByArgs; foreground
pub struct DesktopScroll { pub hwnd: i64, pub x: i32, pub y: i32, pub dx: i32, pub dy: i32 }          // tier 1
pub struct DesktopDrag { pub hwnd: i64, pub from: Point, pub to: Point, pub button: MouseButton }    // tier 1; foreground
pub struct DesktopWindow { pub hwnd: i64, pub action: WindowAction }
    // Focus | Minimize | Maximize | Restore | Move { rect } | Snap { monitor, zone, fraction_pct } | Close.
    // Tier 1; Close is tier 1 when the app reports nothing unsaved, else 2 (WM_CLOSE through SendMessageTimeout)
pub struct DesktopLaunchApp { pub name: String, pub dry_run: bool, pub wait_main_window_ms: Option<u32> }   // tier 1; ambiguous names return candidates
pub struct DesktopWaitFor { pub condition: WaitCondition, pub timeout_ms: u32 }   // at most 30,000. Tier 0
pub struct DesktopSelection { pub hwnd: Option<i64>, pub allow_clipboard: bool }  // tier 0
pub struct DesktopInsertText { pub text: String, pub target: InsertTarget, pub category: Option<FieldCategory> }   // tier 1
pub struct DesktopVolume { pub action: VolumeAction }    // Set { pct } | Step { delta_pct } | Mute(bool) | Get. Tier 1 (Get 0); inverse journaled
pub struct Selector { pub process: String, pub window_class: Option<String>, pub automation_id: Option<String>,
    pub control_type: Option<String>, pub name: Option<String>, pub ancestors: Vec<SelectorStep> }

// apps toolset
pub struct AppsMedia { pub action: MediaAction, pub session: Option<String> }
    // Status | Play | Pause | Toggle | Next | Previous | Seek { seconds } | SeekBy { seconds }. Status tier 0, else 1
pub struct AppsOpenUrl { pub url: String, pub start_s: Option<u32>, pub wait_for_media_title: Option<String> }   // https only. Tier 1
pub struct AppsOpenUri { pub uri: String }                // allowlisted schemes only (ms-settings:, mailto: compose, approved app protocols). Tier 1
pub struct AppsOpenPath { pub path: String, pub with_app: Option<String> }   // tier 1
pub struct AppsResolve { pub op: ResolveOp }
    // tier 0: Hello | TimelineInfo | ProjectsRecent | Timelines | CurrentItem | Versions | Markers | RenderStatus | IsRendering
    // tier 1: OpenPage { page } | Goto { offset_s: f64 } | GotoTimecode { tc } | SetTimeline { name } | ExportStill
    //         | AddVersion { name } | LoadVersion { name } | MarkerAdd { offset_s, name, color }
    // ByArgs: LoadProject { name } (2 if the open project has unsaved changes) | RenderStart { preset, file_name } (2 if it would overwrite)
    //         | ImportFusionComp { template, params } and ApplyDrx { template, window: Option<Rect>, all_layers: bool }
    //           (1 after AddVersion in the same job, else 2; preview_before)
    // the act toolset exposes only the tier-0 ops and OpenPage, Goto, GotoTimecode, SetTimeline, LoadProject (its ByArgs tier still applies), MarkerAdd
pub struct AppsImage { pub op: ImageOp }
    // Faces { path } | PickFace { path, which: Position } | Crop { path, rect } | Sharpness { path, rect }
    // | TrackFace { stills: Vec<String>, start: Rect }. Tier 0
pub struct AppsOffice { pub op: OfficeOp }    // WordSelection | ExcelActiveCell. Tier 0
pub struct AppsExplorer { pub op: ExplorerOp }   // Selection | OpenFolder { path }. Tier 0 or 1
pub struct AppsRecent { pub query: String, pub kinds: Vec<FileKind>, pub limit: Option<u8>, pub thumbnails: bool }   // FileKind includes Media. Tier 0
pub struct AppsPhotoshop { pub op: PhotoshopOp }
    // Recent | Open { path } | DuplicateLayer | SelectEllipse { rect, feather_px } | SmartSharpen { amount_pct, radius_px }. Tier 1;
    // SmartSharpen has preview_before; Generative Fill and Neural Filters, if ever added, are tier 2

// learn toolset
pub struct LearnTrajectoryRead { pub job: JobId }
pub struct LearnSkillRead { pub id: SkillId }
pub struct LearnSkillHistory { pub id: SkillId }
pub struct LearnSkillPropose { pub doc: SkillDoc, pub reason: CommitReason, pub evidence: Vec<JobId> }
    // returns Committed { version } | PendingApproval { lines } | Rejected { errors: Vec<LintError> }
pub struct LearnFactPropose { pub kind: FactKind, pub text: String, pub evidence: Vec<JobId> }   // never with observed provenance

// session toolset
pub struct SessionHandoff { pub session_id: String, pub cwd: String, pub summary: String, pub next: String }
    // creates a pending handoff the user confirms by voice or HUD (naming the repo and the summary); never starts a job alone
pub struct SessionSay { pub text: String }     // at most 200 characters, at most one per 30 s, spoken as "From your Claude session: ..."
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

Shim to core protocol: named pipe `\\.\pipe\eileen-dispatch-<session>`, user-only ACL, JSON lines (at most 16 MiB per line, for images). First line `{"hello":{"v":1,"token":"...","role":"brain"|{"worker":"ws_..."}|"librarian"|"session"|"dev"}}`; the dispatcher checks the token, then the caller's process (an `eileen-shim.exe` from Eileen's install folder inside the matching session's Job Object, section 2.12), and answers `{"welcome":{"tools":[ToolSpec...]}}`; then `{"call":{"id":n,"tool":"...","args":{...},"meta":{...}}}` answered by `{"result":{"id":n,"content":[...],"is_error":false}}`. A wrong token or the wrong process closes the pipe and logs a security event. The shim never decides anything. WP0's stub shim already answers MCP `initialize` and an empty `tools/list`, so the claude CLI never waits `MCP_TIMEOUT` (30 s) on it during W1.

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
    pub awaiting: Option<Awaiting>, pub assistant_name: String, pub monitors: u8, pub media_history: Vec<MediaRef> }
pub enum Resolution {
    Command { command: CoreCommand, slots: SlotValues },        // L1 safety and control, and built-in commands
    Tool { tool: ToolName, args: serde_json::Value, source: ToolSource },   // Grammar | ConfirmedPhrasing (slot-free only)
    Skill { skill: SkillRef, slots: SlotValues, level: Level }, // L1 template: level 4 replays now; level 3 goes to the brain to confirm
    Ambiguous { question: String, options: Vec<String> },       // including an ambiguous slot ("15 seconds or 15 minutes?")
    Hints(Vec<(SkillRef, u16)>),                                // L2: for the brain only
    Brain { depth: Option<Depth>, addressee_unsure: bool },     // L3, with any depth override
}
pub enum SlotValue { Resolved(serde_json::Value), Ambiguous { candidates: Vec<serde_json::Value> } }
pub fn introspect(tools: &[ToolSpec], app_caps: &[AppCapability], skills: &[SkillMeta], focused: Option<&str>) -> Capabilities;
```

Rules the resolver enforces, each with a golden-file test in WP7: the precedence of section 2.4 (safety and control, then built-in grammar, then learned templates); a full match accounts for every content word, and an unknown descriptor demotes a template hit to a hint; a slot resolver returns `Ambiguous` for bare numbers and times without units, and replay never runs on an ambiguous slot; a confirmed L2 phrasing runs directly only when its target has no slots (for a slotted target, the L1 slot resolvers must extract every slot from the new utterance, stored values are never reused, and the skill's own level rules still apply); and the linter rejects a learned template that matches any phrase in the grammar golden file, has fewer than two literal tokens, or has a `Text` slot that could swallow a conjunction or a grammar verb. The template grammar itself (parse, validate, normalise) lives in `eileen-skills::template`, shared by the matcher and the linter.

Latency budgets, tested in CI with loose bounds and measured on the owner's PC: `resolve` under 1 ms p99 with 5,000 skills and the full grammar; `resolve_semantic` under 5 ms p99 for the comparison; the embedding round trip under 20 ms p95 for a 12-word utterance (WP4 measures; if it misses, L2 runs lexical only and says so in `eileen doctor`). The candidate embedding model is a small sentence-embedding model of the all-MiniLM-L6-v2 class (Apache-2.0, about 23 MB as int8 ONNX); WP4 verifies the license, size, digest and latency before adopting it and records the result.

#### 3.3.11 Settings and configuration

```rust
// eileen-contracts::config: the whole configuration as types; doc comments become the commented default file
pub struct Config {
    pub assistant: AssistantConfig,   // name ("Eileen"), personality (free text, at most 400 chars), verbosity, humor
    pub voice: VoiceConfig,           // live, narration, announcements, fallback, rate_pct, watermark_exports
    pub hud: HudConfig,               // corner, scale_pct, opacity_pct, show_transcript, suggestions, visible_intent_ms
    pub conversation: ConversationConfig,     // window_s (45), follow_up_s (8), alone_scoring (on)
    pub announcements: AnnouncementsConfig,   // per event kind: Interrupt | WhenIdle | Silent | HudOnly; per job kind overrides
    pub approvals: ApprovalsConfig,   // voice_window_s (20), deadline_min (30), hook_hold_s (120), action_word_tier2 (true)
    pub routing: RoutingConfig,       // brain model ids, per kind: route, model, depth; hot_spare; recycle thresholds; speculative
    pub memory: MemoryConfig,         // profile (auto | full | low), reserve_mb (2048), pool_computer_use (1), pool_deep (1),
                                      // max_build_heavy_jobs (1), worker_job_limit_mb (0 = off)
    pub permissions: PermissionsConfig,       // grants (tier 1, or tier 2 with a logged decision), private apps, desktop apps
    pub desktop: DesktopConfig,       // lease_idle_s (5), visible_intent_ms (400)
    pub hotkeys: HotkeysConfig,       // mute, report, approve, deny, hands_off, dictation
    pub dictation: DictationConfig,   // polish, categories, key, languages
    pub learning: LearningConfig,     // enabled, distill ("claude" | "off"), librarian_max_plan_pct (50), snapshot_k (12),
                                      // regret_window_s, ladder thresholds
    pub mcp: McpConfig,               // servers: per app, approved third-party MCP servers and the job kinds that get them
    pub privacy: PrivacyConfig,       // location, off_the_record, ledger_days, keep_unaddressed (false)
    pub focus: FocusConfig,           // quiet_during_calls, quiet_full_screen
    pub first_run: FirstRunConfig,    // done, max_clone_ttfa_ms (3000)
    pub speech: SpeechConfig,         // pin_working_set (false), low-memory models
    pub logging: LoggingConfig, pub flight: FlightConfig, pub incidents: IncidentsConfig, pub recording: RecordingConfig,
    pub fleet: FleetConfig,           // opt_in, schedule, upload, mode (recorded | live), turn_budget
    pub dev: DevConfig,               // max_lsp_workers
}

// eileen-config
pub struct SettingSpec {
    pub key: &'static str,            // "assistant.name"
    pub doc: &'static str,
    pub voice: bool, pub hud: bool,
    pub risk: Risk,                   // Low | Misfire(CheckKind) | Safety(SafetyBound)
    pub apply: ApplyMode,             // NextTurn | Live | Restart(Component)
}
pub enum CheckKind { NameRecognisable, HotkeyFree, VoiceRealtime, None }
pub enum SafetyBound { NeedsUserApproval, Tier1Only, Tier2WithLoggedDecision, Never }
pub struct SettingChange { pub key: String, pub value: serde_json::Value }
pub struct Proposal { pub id: ProposalId, pub changes: Vec<SettingChange>, pub diff: Vec<DiffLine>,
    pub checks: Vec<CheckResult>, pub confirm_sentence: String, pub needs_yes: bool, pub utterance: String,
    pub source: SettingsSource }
pub struct SettingsVersion { pub version: u64, pub at: WallTime, pub source: SettingsSource, pub utterance: Option<String>,
    pub approval: Option<ApprovalId>, pub diff: Vec<DiffLine>, pub snapshot_toml: String }
pub enum SettingsSource { Voice, Hud, File, Profile(String), Undo, Default, BrainProposal }

pub fn registry() -> &'static [SettingSpec];
pub fn load(dir: &Path) -> Result<(Config, Vec<Diagnostic>), ConfigError>;          // every error names a line
pub fn validate(current: &Config, changes: &[SettingChange]) -> Result<Proposal, ConfigError>;
pub fn apply(current_toml: &str, changes: &[SettingChange]) -> Result<String, ConfigError>;   // keeps comments (toml_edit)
pub fn explain(config: &Config, layers: &Layers) -> Vec<Explained>;
```

The flow itself (propose, check, confirm, apply, version, undo) is a state machine in `eileen-core::settings`, so it is simulated like everything else; `eileen-config` provides the pure parts and the file IO used by the runtime's driver. Which keys are `Safety` is listed in section 2.11.

#### 3.3.12 Connectors

```rust
pub struct ConnectorManifest {
    pub id: ConnectorId, pub version: String,
    pub runs: RunsIn,                    // CoreTask | Sidecar { binary: String }
    pub platforms: Vec<Platform>,        // Windows, Linux, MacOs
    pub tools: Vec<ToolSpec>,
    pub events: Vec<String>,             // event kinds it publishes
    pub checks: Vec<(ToolName, String)>, // (tool, op) pairs recipes may use as checks; each op is read_only and tier 0
    pub senses_room: bool,               // camera, room microphone: opt-in per device, local only
    pub requires_opt_in: bool,
}
pub struct AppCapability { pub app: String, pub verbs: Vec<String>, pub version: Option<String>, pub rungs: Vec<Rung> }
```

Each sidecar answers a `connector_manifest` call (core-only, never shown to models) right after the MCP handshake. Each adapter fills `AppCapability.version` from the app itself (Resolve's `GetVersionString`, the executable's file version otherwise), which is what puts a recipe on probation after an app update.

#### 3.3.13 Learning types

The types of the learning-loop report section 6.2, with the changes the reviews required: no Gemini route; steps keyed by `CallId`; recipes with step references and three step kinds; checks per op; skill use attributed; provenance computed; versions in SQLite.

```rust
// eileen-contracts::learn
pub enum Level { Draft, Trusted, Recipe, FastPath }
pub enum Provenance { User, Worker { route: Route }, Tool { tool: ToolName }, Observed { source: ObservedSource }, Librarian, Approved { approval: ApprovalId } }
pub enum ObservedSource { Screen, Web, File, Notification }
pub enum JobOrigin { Worker, Replay, FastPath, Act }

pub struct Trajectory {
    pub schema: u16, pub job_id: JobId,
    pub request: Personal<String>, pub task: String,
    pub context: DesktopContext,          // focused app, app versions touched, document identity (redacted), connectors
    pub route: Route, pub origin: JobOrigin,
    pub snapshot: Option<SnapshotManifest>,   // which skill versions the session had
    pub skills_used: Vec<SkillUse>,
    pub steps: Vec<TrajectoryStep>,
    pub cost: Cost, pub outcome: Option<Outcome>, pub redactions: u32, pub no_learn: bool,
}
pub struct SkillUse { pub skill: SkillRef, pub args: Untrusted<String>, pub slot_values: SlotValues }
pub struct TrajectoryStep {
    pub call: CallId, pub at_ms: u64,
    pub tool: ToolName, pub op: Option<String>, pub schema_hash: String,
    pub args: Untrusted<serde_json::Value>,      // redacted
    pub ok: bool,
    pub result: Untrusted<serde_json::Value>,    // redacted; long string leaves cut, every key and the structure kept
    pub tier: Tier, pub approval: Option<bool>,
    pub provenance: Provenance,                  // computed by eileen-skills, never taken from the librarian
    pub tainted: bool, pub rung: Option<Rung>, pub app: Option<(String, String)>,   // app and version touched
}
pub enum Signal { Accepted, Praised, Corrected { text: Personal<String> }, Undone { after_ms: u64 },
    Retried, Cancelled, PostcheckPassed, PostcheckFailed { step: String } }
pub enum Verdict { Success, Failure, Unclear }
pub struct Outcome { pub status: JobStatus, pub signals: Vec<Signal>, pub verdict: Verdict, pub objective_check: bool }

pub struct SkillRef { pub id: SkillId, pub version: u32 }
pub enum SlotKind { Offset, Count, Text, App, File, Url, Media, Position, Monitor, Choice { options: Vec<String> } }
pub struct SlotSpec { pub name: String, pub kind: SlotKind, pub required: bool, pub default: Option<serde_json::Value>,
    pub seen_values: Vec<serde_json::Value> }   // enumerated kinds: values a successful use has seen
pub struct SkillMeta {
    pub id: SkillId, pub version: u32, pub level: Level, pub risk: RiskClass,   // Light | Standard
    pub description: String, pub triggers: Vec<String>, pub slots: Vec<SlotSpec>,
    pub apps: Vec<(String, String)>,      // app, version range the recipe was proven on
    pub tool_hashes: Vec<(ToolName, String)>,   // schema hashes the recipe was proven with
    pub has_recipe: bool, pub evidence: Vec<JobId>, pub phrasings: Vec<String>,
    pub pinned: bool, pub untrusted_origin: bool,
}
pub struct Recipe { pub skill: SkillRef, pub pre: Vec<Check>, pub steps: Vec<RecipeStep> }
pub struct RecipeStep { pub id: String, pub kind: StepKind, pub pre: Vec<Check>, pub post: Vec<Check>,
    pub timeout_ms: u32, pub on_fail: OnFail }
pub enum StepKind {
    Call { tool: ToolName, op: Option<String>, args: serde_json::Value },   // {"$slot": "t"}, {"$step": "id", "ptr": "/json/pointer"}
    Delegate { goal: String, inputs: serde_json::Value, post: Vec<Check> }, // a narrow worker sub-job
    Confirm { question: String, show: Option<serde_json::Value> },          // never skipped by replay
}
pub enum OnFail { Handoff, RetryOnce /* idempotent ops only */, Abort }
/// Checks may only call ops that are read_only and tier 0; the linter enforces it against the connector manifests.
pub struct Check { pub tool: ToolName, pub op: Option<String>, pub args: serde_json::Value, pub path: String, pub expect: Expect }
pub enum Expect { Equals(serde_json::Value), Contains(String), AtLeast(f64), AtLeastRel { of_step: String, path: String, factor: f64 },
    Within(f64), Exists, Truthy }

pub struct UserFact { pub id: String, pub kind: FactKind, pub text: String, pub provenance: Provenance,
    pub source_turn: Option<TurnId>, pub source_job: Option<JobId>, pub confidence_pct: u8,
    pub created: WallTime, pub confirmed: WallTime }
pub enum FactKind { User, Feedback, Project, Reference, Vocabulary, Phrasing, Source }
pub struct PhrasingFact { pub utterance: Personal<String>, pub target: PhrasingTarget, pub embedding: Vec<f32>, pub confirmed: u16 }
pub enum PhrasingTarget { SlotFreeCall { tool: ToolName, args: serde_json::Value }, GrammarEntry { id: String }, SkillTemplate { skill: SkillId } }

pub struct SkillVersion { pub skill: SkillId, pub version: u32, pub at: WallTime, pub reason: CommitReason,
    pub evidence: Vec<JobId>, pub doc: SkillDoc }           // full content of SKILL.md, references and recipe
pub enum CommitReason { Distill, Repair, UserEdit, UserCorrection, Rollback, Promote, Demote }
pub struct SnapshotManifest { pub session: WorkerSessionId, pub files: Vec<(SkillRef, String /* path */, String /* blake3 */)> }
pub struct LadderPolicy { pub light: Thresholds, pub standard: Thresholds, pub demote_failures_in_last5: u8,
    pub archive_idle_days: u16 }   // defaults in section 2.10
pub struct UseRecord { pub job: JobId, pub at: WallTime, pub verdict: Verdict, pub objective_check: bool,
    pub slot_values: SlotValues, pub replay: bool, pub handoff: bool, pub app_versions: Vec<(String, String)> }
```

`eileen-skills` exposes, as pure functions: `template::{parse, validate, normalize}`, `ladder::next_level(meta, uses, policy) -> Transition`, `lint::lint(doc, evidence, manifests) -> Vec<LintError>` (every rule of the learning-loop report section 3.11 plus the rules of section 2.10: step references, per-op checks, idempotent retries, eligibility, the deny-scan, template collisions with the grammar), `provenance::compute(doc, evidence) -> ProvenanceReport`, `shadow::agree(recipe, slots, steps) -> ShadowVerdict`, `verdict::success(outcome) -> bool`, the replay state machine (`ReplayMachine`), and `forget::plan(target, store_index) -> ForgetPlan`. `eileen-learn` implements the IO: `Recorder`, the `learn.sqlite` store, the snapshot writer, the librarian scheduler and the `learn` toolset handlers, the user-model and ledger stores, and the forget executor.

#### 3.3.14 Scenarios

The scenario format is a contract because packages, contributors and CI all write and read it. `scenarios/<area>/<id>/scenario.toml`, validated by deserializing into these types and against a JSON Schema generated from them into `scenarios/_schema/scenario.schema.json`:

```rust
pub struct Scenario {
    pub schema: u16,                     // 1
    pub id: ScenarioId, pub title: String, pub area: String, pub tags: Vec<String>,
    pub origin: ScenarioOrigin,          // Synthetic | SyntheticFromUser | ContributedConsentedAudio | FromSkill
    pub contributor: Option<String>,     // GitHub handle, never an email
    pub recorded_at_commit: Option<String>,
    pub outcome: ExpectedOutcome,        // Pass | ExpectedFail (a saved failure, until fixed)
    pub intent: ScenarioIntent,          // utterances (text, placeholders allowed), optional consented audio, stock voice,
                                         // expected: Option<String> (the user's "what should have happened")
    pub environment: ScenarioEnv,        // min Windows build, requires (capabilities: "resolve-studio>=21.0", "monitors>=2",
                                         // "real-claude", "photoshop"), lanes: Sim | Desktop | Bench
    pub world: Option<SimWorld>,         // for hand-written sim scenarios: brain script, worker scripts by fan-out index,
                                         // sidecar latency and faults, sim profile, memory pressure
    pub fixtures: Fixtures,              // files from templates in the area's _fixtures/, apps to launch, window layout
    pub steps: Vec<Step>,
    pub expect: Vec<Expectation>,
    pub timing: BTreeMap<String, TimingBudget>,   // "turn.ended -> job.completed" = { max_ms, ci_factor }
    pub sim: Option<SimReplay>,          // inputs = "replay/inputs.jsonl" (plain JSONL, size-capped), expected = "replay/expected.trace"
    pub quarantine: Option<Quarantine>,  // issue link and expiry date; required for a known-flaky scenario
    pub status: ScenarioStatus,          // Active | Stale { since_commit, issue }
}
pub enum Step {
    Say { utterance: u16 },              // index into intent.utterances
    Type { text: String }, Hotkey { keys: String }, Hud { input: HudInput },
    WaitFor { event: String, timeout_ms: u32 },   // a name from docs/reference/events.md
    Approve, Deny, Pause { ms: u32 }, PhysicalInput,   // PhysicalInput simulates the user touching the mouse
}
pub enum Expectation {
    Event { pattern: String },           // matched against the event trace with [ANY], [ANYLINE], [UNORDERED] (3.3.16)
    Call { tool: ToolName, args_contain: serde_json::Value },
    Spoken { contains: String },  NotSpoken,
    JobStatus { status: JobStatus },
    WindowOnMonitor { process: String, ordinal: u8 },
    WindowSnapped { process: String, zone: String },
    FieldText { selector: Selector, contains: String },
    File { path: String, sha256: Option<String>, contains: Option<String> },
    ResolvePlayhead { timecode: String, tolerance_frames: u32 },
    MediaPosition { seconds: f64, tolerance_s: f64 },
    SharpnessIncrease { min_pct: u16 },
    SettingEquals { key: String, value: serde_json::Value },
    NoFileLeft { glob: String },         // for deletion checks (first run declined, forget)
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
outcome = "pass"

[intent]
utterances = ["Open Resolve and go to 15:30", "uh go to fifteen thirty in resolve"]
voice = "kokoro:af_heart"            # used when a runner needs audio; the text is the source of truth

[environment]
requires = ["resolve-studio>=21.0"]
lanes = ["sim", "desktop"]

[fixtures]
apps = [{ launch = "resolve", project_template = "_fixtures/eileen-test" }]

[[steps]]
say = { utterance = 0 }

[[steps]]
wait_for = { event = "turn.ended", timeout_ms = 20000 }

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
inputs = "replay/inputs.jsonl"
expected = "replay/expected.trace"
```

Sim-lane expectations are the ones the simulator can check (events, calls, spoken text, job status, settings, files in a simulated store); desktop-lane expectations need a real desktop and run on the owner's PC and on fleet PCs. Contributed scenarios may use only fixture templates already in the repository; they carry no binary files of their own.

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
/// Implemented by eileen_skills::Engine. Pure; the core drives replay and the ladder through it.
pub trait SkillEngine: Send {
    fn rebuild(&mut self, skills: &[SkillMeta], recipes: &[Recipe], manifests: &[ConnectorManifest]);
    fn next_level(&self, skill: &SkillId, uses: &[UseRecord]) -> Transition;
    fn success(&self, outcome: &Outcome) -> bool;
    fn start_replay(&self, skill: &SkillRef, slots: &SlotValues) -> Result<ReplayState, ReplayRefusal>;
    /// Feed the last step's result (or None to begin); get the next thing to do:
    /// Call(ToolCallPlan) | Check(CheckPlan) | Delegate(JobSpec) | Confirm(AskUser) | Done | Handoff(HandoffBrief).
    fn next(&self, state: &mut ReplayState, last: Option<&StepResult>) -> ReplayStep;
    fn shadow_agree(&self, skill: &SkillRef, slots: &SlotValues, steps: &[TrajectoryStep]) -> ShadowVerdict;
}

/// A component the runtime starts: audio, speech client, harness, sidecar clients, learning, HUD client, shell.
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

Each component crate exposes one constructor, for example `eileen_harness::component(cfg: &HarnessConfig) -> Box<dyn Component>`, `eileen_audio::component(cfg)`, `eileen_learn::component(cfg)`, `eileen_shell_win::component(cfg)`. `eileen-replay` also provides a component, `eileen_replay::responder(scenario)`, that answers brain and worker commands from a recording; `eileen run --replay-models <dir>` swaps it in for the harness, which is how fleet PCs run scenarios with no model (section 4.8). WP0's stubs return a component that accepts its effects and does nothing, so `eileen-cli` wires everything from the first day.

#### 3.3.16 Recording, replay and fleet formats

The simulator, the replayer, the flight recorder, the stub harness, scenarios and the fleet share these formats, so they are contracts (`eileen-contracts::{record, scenario, fleet}`), snapshot-tested, and each parser of them has a fuzz target.

```rust
pub struct InputRecord { pub seq: u64, pub mono: MonoTime, pub at: WallTime, pub ctx: TraceCtx, pub input: Input }
    // large values replaced by {"$blob": "b3:..."}; unaddressed speech replaced by a placeholder when written
pub struct Checkpoint { pub seq: u64, pub mono: MonoTime, pub rng_state: [u64; 4], pub core: CoreSnapshotV1 }
pub struct CoreSnapshotV1 { pub v: u16, /* jobs, mute, conversation window, approvals, input lock, settings version,
    skill versions, plan usage, first-run phase, id counters: every field the core needs to resume */ }
    // eileen-core converts to and from it; a new field is added with a default so old checkpoints still load
pub struct RecordingManifest { pub id: RecordingId, pub origin: Origin, pub started: WallTime, pub ended: WallTime,
    pub eileen_version: String, pub recorded_at_commit: String, pub config_hash: String,
    pub audio: Vec<AudioLane>, pub turns: Vec<TurnId>, pub jobs: Vec<JobId> }
pub struct IncidentManifest { pub id: IncidentId, pub trigger: String, pub note: Option<Personal<String>>, pub at: WallTime,
    pub session: SessionId, pub turns: Vec<TurnId>, pub jobs: Vec<JobId>, pub eileen_version: String,
    pub recorded_at_commit: String, pub build_profile: String, pub os: String, pub cpu_load_pct: u8, pub free_ram_mb: u32,
    pub processes: Vec<String>, pub contains_personal: bool, pub contains_audio: bool, pub redactions: u32,
    pub dumps: Vec<String> }   // the observability report's section 4.3 fields
pub enum StubScriptLine {                     // tests/harness-scripts/*.ndjson, played by eileen-stub-harness
    Event { json: serde_json::Value },        // one stream-json line, emitted as is
    Delay { ms: u32 }, Crash { code: i32 }, Malformed { bytes: String },
    PermissionRequest { tool_name: String, tool_input: serde_json::Value },
    Narration { line: String },
    AwaitUser,                                // wait for the next user line before continuing
}
pub struct SimRegression {                    // crates/eileen-sim/regressions/<name>.toml
    pub profile: String, pub seed: Option<u64>, pub found_at_commit: String,
    pub inputs: Vec<InputRecord>,             // decoded inputs, never a buffer of random bytes
    pub faults: Vec<FaultRecord>,             // each injected fault as an explicit record, never a buggify site ordinal
    pub invariant: String,
}
pub struct FleetResult {                      // one JSON line per scenario run
    pub install: InstallId, pub run: String, pub scenario: ScenarioId, pub scenario_commit: String,
    pub mode: FleetMode,                      // Recorded (no model) | Live (real Claude, within a turn budget)
    pub passed: bool, pub expectations: Vec<(String, bool)>, pub phases_ms: BTreeMap<String, u32>,
    pub windows_build: String, pub monitors: u8, pub dpi: Vec<u16>, pub app_versions: BTreeMap<String, String>,
    pub locale: String, pub cpu_class: String, pub ram_gb: u8, pub free_ram_mb: u32, pub user_became_active: bool,
    pub learning: Option<LearningStats>,      // skills per level, handoff rate, median turns by run index; no names
}
pub struct FleetRun { pub install: InstallId, pub eileen_version: String, pub started: WallTime, pub results: Vec<FleetResult> }
```

**The expected-trace grammar** (`replay/expected.trace`): one event per line, `<dotted.event.name> <key>=<value> ...`, values compared after the same normalisation the recorder applies; `[ANY]` matches any one line, `[ANYLINE]` any number of lines, and a block between `[UNORDERED]` and `[END]` matches its lines in any order. IDs are written as `@1`, `@2` (first appearance order), never as real ULIDs, so traces survive ID changes.

**Two replay modes** (`eileen-replay`):

- **Strict**, for incidents, recordings and `--doublecheck`: start from the checkpoint, feed every recorded input in order with its recorded time and RNG state, compare every bus event with the recording, and stop at the first difference with its sequence number and span path. It is valid only on the commit that recorded it.
- **Scenario**, for saved and shared scenarios: start a fresh core from the scenario's config and fixtures (never an old snapshot; `eileen record trim` emits the setup inputs needed instead), schedule the user's own inputs (speech, typing, hotkeys, HUD) by time, and serve brain, worker, hook and tool responses reactively when the live core emits the matching effect, matched by connector, tool, a digest of the arguments and the ordinal among equal calls. Recorded IDs are remapped to live IDs by first appearance. It checks the scenario's expectations, not full-trace equality, and reports any effect it had no recorded response for. A scenario that no longer replays after a change is either re-recorded as a reviewed diff (`cargo xtask scenarios bless <id>`) or marked `stale` with an issue, never silently skipped.

### 3.4 Platform traits

`eileen-platform` holds the traits every OS implements. Windows implementations live in the `-win` crates; fakes for tests live next to each trait under `cfg(any(test, feature = "fake"))`.

```rust
pub trait AudioEndpoints: Send + Sync {          // eileen-audio-win; cpal does the streaming itself
    fn default_capture(&self, category: StreamCategory) -> Result<EndpointInfo, PlatformError>;
    fn default_render_loopback(&self) -> Result<EndpointInfo, PlatformError>;   // the far-end reference for AEC
    fn set_communications_ducking(&self, opt_out: bool) -> Result<(), PlatformError>;
    fn register_audio_thread(&self) -> Result<ThreadTicket, PlatformError>;   // MMCSS
    fn system_volume(&self) -> Result<VolumeControl, PlatformError>;          // IAudioEndpointVolume for desktop_volume
}
pub trait Grounding: Send + Sync {               // eileen-desktop-win
    fn monitors(&self) -> Result<Vec<Monitor>, PlatformError>;
    fn windows(&self, include_minimized: bool) -> Result<Vec<WindowInfo>, PlatformError>;
    fn foreground(&self) -> Result<Option<WindowRef>, PlatformError>;
    fn window_from_point(&self, p: Point) -> Result<Option<WindowRef>, PlatformError>;
    fn focused_element(&self, deadline: Deadline) -> Result<Option<ElementInfo>, PlatformError>;
    fn element_from_point(&self, p: Point, deadline: Deadline) -> Result<Option<ElementInfo>, PlatformError>;
    fn selection(&self, allow_clipboard: bool, deadline: Deadline) -> Result<Option<Selection>, PlatformError>;
    fn presentation_state(&self) -> Result<PresentationState, PlatformError>;
    fn mic_in_use_elsewhere(&self) -> Result<bool, PlatformError>;
    fn subscribe(&self, sink: Box<dyn Fn(DesktopChange) + Send + Sync>) -> Result<Subscription, PlatformError>;
}
pub trait Automation: Send + Sync {              // eileen-desktop-win (UIA on its own MTA thread, abandoned and replaced on a hang)
    fn tree(&self, hwnd: i64, limits: TreeLimits, deadline: Deadline) -> Result<ElementTree, PlatformError>;
    fn invoke(&self, element: &ElementHandle, deadline: Deadline) -> Result<Verification, PlatformError>;
    fn set_value(&self, element: &ElementHandle, value: &str, deadline: Deadline) -> Result<Verification, PlatformError>;
    fn find(&self, hwnd: i64, selector: &Selector, deadline: Deadline) -> Result<Vec<ElementCandidate>, PlatformError>;
}
pub trait Input: Send + Sync {                   // eileen-desktop-win
    /// Physical idle time from Raw Input; synthetic input does not reset it.
    fn user_idle_ms(&self) -> Result<u32, PlatformError>;
    fn take_foreground(&self, hwnd: i64) -> Result<ForegroundOutcome, PlatformError>;   // Taken | Refused
    /// Sends events one at a time; stops within one event when physical input or a focus change is seen.
    fn send(&self, hwnd: i64, events: &[InputEvent], guard: ForegroundGuard) -> Result<SendOutcome, PlatformError>;
    fn restore(&self, token: RestoreToken, cursor_if_unmoved: bool) -> Result<(), PlatformError>;
}
pub trait Capture: Send + Sync {
    fn capture(&self, target: &ShotTarget) -> Result<RgbaImage, PlatformError>;
}
pub trait Apps: Send + Sync {                    // index and launch
    fn index(&self) -> Result<Vec<AppEntry>, PlatformError>;
    fn launch(&self, app: &AppEntry) -> Result<u32, PlatformError>;     // pid
    fn main_window(&self, pid: u32, deadline: Deadline) -> Result<Option<WindowRef>, PlatformError>;
    fn open_url(&self, url: &str) -> Result<(), PlatformError>;
    fn open_uri(&self, uri: &str) -> Result<(), PlatformError>;
    fn open_path(&self, path: &str, with_app: Option<&AppEntry>) -> Result<(), PlatformError>;
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
    fn thumbnail(&self, item: &RecentItem, edge_px: u32) -> Result<Option<RgbaImage>, PlatformError>;
}
pub trait ShellHost: Send + Sync {               // eileen-shell-win, on the core's shell thread
    fn register_hotkey(&self, chord: &str, id: u32) -> Result<(), PlatformError>;   // fails if another app owns it
    fn set_tray(&self, state: TrayState) -> Result<(), PlatformError>;               // menu items are data
    fn toast(&self, toast: &Toast) -> Result<(), PlatformError>;
    /// Raw Input sink: edges of the configured dictation key and a timestamp of the last physical input, nothing else.
    fn watch_input(&self, dictation_key: VirtualKey, sink: Box<dyn Fn(ShellInputEvent) + Send + Sync>) -> Result<Subscription, PlatformError>;
    fn set_run_at_sign_in(&self, on: bool) -> Result<(), PlatformError>;            // HKCU Run entry
}
pub trait HudSurface {                           // eileen-hud-win, driven by eileen-hud
    fn draw(&mut self, list: &DrawList) -> Result<(), PlatformError>;
    fn set_watch(&mut self, hwnd: Option<i64>, rect: Rect) -> Result<(), PlatformError>;   // DWM live thumbnail
    fn next_input(&mut self) -> Option<(HudInput, bool /* physical */)>;
}
```

COM automation of Office, Explorer, Photoshop and the Resolve process gate are Windows-specific adapters inside `eileen-apps-win` behind the `apps_*` tool contracts; a Linux port replaces them per app, not through a generic trait.

### 3.5 The command line

`eileen.exe` subcommands, wired in `eileen-cli` by WP0's stub and implemented in the owning crate's `cli` module:

| Command | What it does | Implemented in |
|---|---|---|
| `eileen run [--data-dir D] [--audio-source wav:F] [--audio-sink capture:F] [--stub-harness SCRIPT] [--replay-models DIR] [--muted] [--no-hud] [--console] [--test-harness]` | Start everything; `--console` prints what is heard, what Eileen says and the jobs, one line each; `--test-harness` enables the runner's tool endpoint (never in normal runs) | runtime and cli (WP2) |
| `eileen say "<text>"` | Typed request into the running instance (an ambient `Cli` turn) | runtime |
| `eileen status`, `eileen jobs [list \| tail ID \| cancel ID]` | Status from the loopback endpoint | runtime |
| `eileen setup`, `eileen doctor [claude \| audio \| resolve \| desktop]` | Models, checks, Start menu and sign-in entries, self-test; `doctor resolve` and `doctor desktop` run `eileen-apps.exe` and `eileen-desktop.exe` subcommands through the supervisor | runtime (framework), speech (models), harness (claude), apps (resolve) |
| `eileen config [explain \| edit \| undo \| profiles \| export \| import]` | Settings | config (WP7) |
| `eileen skills [list \| show \| history \| rollback \| forget \| distill [--job ID] [--wait]]`, `eileen facts [list \| forget]` | Learned skills and facts | learn (WP7) |
| `eileen voice [list \| learn \| use \| delete]` | Voices; runs `eileen-speech.exe voice ...` through the supervisor, or asks the running speech process | speech (WP4) |
| `eileen scenario [save \| list \| show \| share \| submit \| run]` | Scenario corpus | scenarios (WP2) |
| `eileen test --fleet [--once \| --nightly \| --arm-tonight] [--filter] [--mode recorded\|live]` | Fleet runner | fleet (WP9) |
| `eileen trace ID \| last`, `eileen logs`, `eileen incident [list \| show \| capture \| pack]` | Debugging | runtime |
| `eileen record [start \| stop \| list \| trim]`, `eileen replay ID [--level core] [--mode strict\|scenario]` | Recording and replay | record and replay (WP1) |
| `eileen debug [stats \| tasks \| dump]` | Live internals | runtime |
| `eileen claude [install \| uninstall \| update-window]` | Prints the official plugin commands and runs them after a yes; drains and restarts Claude sessions around a CLI update | harness (WP3) |
| `eileen apps add-mcp <app> <server>` | Approve a third-party MCP server for an app (tier 2) | config (WP7) |
| `eileen mcp-probe`, `eileen bench` | Developer probes | runtime |

## 4. Pins, models, setup, budgets and tests

### 4.1 Dependency pins

WP0 writes this into the root `Cargo.toml`. The versions are the ones the rust-architecture, voice-stack, computer-use and observability reports verified on 2026-10-20 (the skeleton compiled and passed clippy, tests, cargo-deny and cargo-machete on the async, data, protocol and Windows rows). Several were released within two weeks of that date; they are the verified starting set, and the 14-day cooling-off rule applies to every update after WP0. Protocol and native-inference crates are pinned exactly because they change weekly. The feature lists of `tokio`, `windows` and `windows-sys` are the union every package needs, declared once here (section 3.1); members never add features to those three.

```toml
[workspace.dependencies]
# Internal crates: one line each, path = "crates/<name>", for every crate in section 3.1.

# Async and plumbing
tokio = { version = "1.54.0", default-features = false, features = ["rt-multi-thread", "macros", "sync", "time", "io-util", "net", "process", "fs", "signal"] }
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
windows = { version = "0.62.2", default-features = false, features = [   # take core types from windows::core; never mix windows-core 0.100
  "Win32_Foundation", "Win32_Security", "Win32_System_Com", "Win32_System_Ole", "Win32_System_Variant",
  "Win32_System_Threading", "Win32_System_JobObjects", "Win32_System_ProcessStatus", "Win32_System_Diagnostics_ToolHelp",
  "Win32_System_Memory", "Win32_System_SystemInformation", "Win32_System_Power", "Win32_System_Registry",
  "Win32_System_DataExchange", "Win32_System_Pipes", "Win32_System_RemoteDesktop", "Win32_NetworkManagement_IpHelper",
  "Win32_Storage_FileSystem", "Win32_UI_Accessibility", "Win32_UI_WindowsAndMessaging", "Win32_UI_Input",
  "Win32_UI_Input_KeyboardAndMouse", "Win32_UI_HiDpi", "Win32_UI_Shell", "Win32_UI_Shell_PropertiesSystem",
  "Win32_UI_Controls", "Win32_Graphics_Gdi", "Win32_Graphics_Dwm", "Win32_Graphics_Direct2D",
  "Win32_Graphics_Direct2D_Common", "Win32_Graphics_DirectWrite", "Win32_Graphics_Dxgi", "Win32_Graphics_Dxgi_Common",
  "Win32_Graphics_Direct3D", "Win32_Graphics_Direct3D11", "Win32_Graphics_DirectComposition", "Win32_Media_Audio",
  "Win32_Media_Audio_Endpoints", "Media_Control", "Foundation", "Foundation_Collections",
] }   # WP0 starts from this list and adds what the stubs show is missing; later additions go through an amendment pull request
windows-sys = { version = "0.61.2", features = ["Win32_Foundation", "Win32_System_Threading", "Win32_System_JobObjects",
  "Win32_System_SystemInformation", "Win32_Storage_FileSystem", "Win32_Security"] }   # buildguard and supervisor
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
ort = { version = "=2.0.0-rc.13", default-features = false, features = ["std", "ndarray", "copy-dylibs"] }   # rc.14 and rc.15 are too new; copy-dylibs puts the DLLs beside test binaries
realfft = "3.5.0"
# rtrb: WP0 pins the newest non-yanked release crates.io lists (the observability report saw 0.4.0 and 0.3.5 listed inconsistently)

# Images
image = { version = "0.25.10", default-features = false, features = ["png", "jpeg"] }
fast_image_resize = "6.1.0"
jpeg-encoder = { version = "0.7.1", features = ["simd"] }   # license (MIT OR Apache-2.0) AND IJG: allow IJG in deny.toml, attribution in NOTICE
base64 = "0.23.1"

# Command line and repository tools
clap = { version = "4.6.7", features = ["derive"] }
# syn: WP0 pins the newest 2.x release at least 14 days old (tools/repo-tools only)

# Tests
insta = { version = "1.49.0", features = ["json", "redactions", "glob"] }
proptest = { version = "1.12.0", default-features = false, features = ["std"] }
proptest-state-machine = "0.9.0"
arbitrary = { version = "1.5.0", features = ["derive"] }
loom = "0.7.2"
# shuttle and shuttle-tokio: WP0 pins the newest releases at least 14 days old (0.9.7 and 1.0.0 on 2026-10-20; 0.9.8 was
# still inside the cooling-off period); WP2 uses them behind a `shuttle` feature no shipped binary enables
```

Deliberately not used: `chacha20poly1305` and DPAPI wrapping (encryption at rest is v0.2, decision D40), `uiautomation` (it initialises COM on every construction; Eileen owns one MTA thread), `nucleo-matcher` (MPL-2.0; Eileen writes its own scorer), `xcap` (no gain over the own capture code), `gix` (skill history is in SQLite), `serde_yaml` and `serde_yml` (deprecated or unsound; config is TOML), `hyper` (the loopback endpoint is a small hand-written HTTP/1.1 server with strict limits and a fuzz target), the OpenTelemetry crates (v0.2, behind a feature). Any new dependency is an architectural change: say so in the pull request, and it must pass cargo-deny.

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

**Native dependencies, and where they may appear.** Every crate below that wraps a platform or vendor native library is declared only under `[target.'cfg(windows)'.dependencies]` of the crates in the right-hand column, so `ubuntu-latest` never builds ALSA, never downloads an unverified Linux prebuilt, and never meets `ort`'s link error without `ORT_LIB_LOCATION`. Check 15 of section 4b.3 enforces the table.

| Native dependency | Allowed in (Windows target table only) | Neutral logic tested on Linux beside it |
|---|---|---|
| `cpal` | `eileen-audio`, `eileen-audio-win` | ring buffer, resampler, AEC wiring with a fake device, `WavSource`, `CaptureSink`, `RoomMixer`, playback accounting |
| `sherpa-onnx`, `ort` | `eileen-speech` (and `ort` in `eileen-apps` for YuNet) | the pipe protocol, turn fusion, the log-mel, the clause splitter, the phrase cache, enrollment checks over stored embeddings, timecode math, face picking and the Laplacian on decoded images |
| `keyring-core`, `windows-native-keyring-store` | `eileen-config` | everything else in config |
| `tray-icon`, `global-hotkey`, `tauri-winrt-notification` | `eileen-shell-win` | n/a (Windows crate) |
| `crash-handler`, `minidumper` | `eileen-crash`, `eileen-guard` | the panic-hook message cleaning |
| `windows`, `windows-sys` | `-win` crates and the Windows modules of `eileen-supervisor`, `eileen-buildguard`, `eileen-crash`, `eileen-guard` | n/a |

**The speech build.** `.cargo/config.toml` sets `rustflags = ["-C", "target-feature=+crt-static"]` for `x86_64-pc-windows-msvc`. `cargo xtask fetch-native` downloads `sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib.tar.bz2` (8,032,957 bytes, SHA256 `b8eedf41bd6d3779218887b48367bb7a3ece5aaa7667f01f69ee823a12b0a9e7`) into a cache directory, verifies the hash, deletes DLLs left from an older pin, and from then on every cargo invocation `cargo xtask` makes on Windows (and every Windows CI job, through the same subcommand) gets `SHERPA_ONNX_ARCHIVE_DIR` pointing there, `ORT_LIB_LOCATION` at the extracted lib folder and `ORT_PREFER_DYNAMIC_LINK=1`, so `ort` links the same `onnxruntime.dll` and nothing downloads unverified binaries. `ort`'s `copy-dylibs` feature copies the DLLs into `target/<profile>`, `deps` and `examples`, so test binaries load them from beside themselves: `System32` holds an older `onnxruntime.dll` (10.7 MB on this PC) that the loader searches before `PATH`. `eileen-speech` and `eileen-apps` check at start that the loaded `onnxruntime.dll` sits beside the executable and reports version 1.28.2, and exit with a clear error otherwise. Shipped beside the binaries: `onnxruntime.dll` 1.28.2 (17,799,168 bytes), `onnxruntime_providers_shared.dll` (104,960 bytes), `sherpa-onnx-c-api.dll` (4,605,952 bytes). `sherpa-onnx-sys` finds its output folder by searching for a folder named after `$PROFILE`, which breaks under a custom profile, so releases build with `--release` and the release workflow sets `CARGO_PROFILE_RELEASE_LTO=fat` and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` instead of a separate `dist` profile. `sherpa-onnx-sys` build-depends on `ureq` with its default TLS, which brings `webpki-roots` (CDLA-Permissive-2.0); `deny.toml` carries a license exception scoped to that one crate (section 4b.3). Before the first binary release, the espeak-ng license question (the prebuilt may contain GPL-3.0 espeak-ng) must be answered (section 7).

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
| Kokoro v1.0 int8 | `kokoro-int8-multi-lang-v1_0` (already downloaded on the owner's PC) | recorded by WP4 | pinned by WP4 | Apache-2.0 | low-memory profile only, if WP4's equal-load test passes |
| Kroko streaming zipformer EN | `sherpa-onnx-streaming-zipformer-en-kroko-2025-08-06.tar.bz2` | 57,267,600 B | `c8676e5ff9ac2a85296e53ee0fd4d5fb1db6770e7a7647166eeafe349ade6834` | per Banafo's Kroko-ASR page; WP4 verifies before shipping a downloader | low-memory profile only |
| Chatterbox-Turbo (official ONNX, q4 or q8) | `speech_encoder`, `embed_tokens`, `language_model`, `conditional_decoder` graphs | pinned by WP4 | pinned by WP4 | MIT | downloaded only when the user picks Chatterbox for a voice; loaded while that voice speaks |
| Chatterbox-Nano (community ONNX export) | per export | pinned by WP4 | pinned by WP4 | MIT for the model; the export's own license checked by WP4 (section 7) | as above |
| Bundled demo clip | a few seconds of CC0 audio or video for the first-run demo | under 1 MB | committed | CC0 | in the install folder |

`eileen setup`, run once by the user (and safe to rerun):

1. Check Windows build, CPU, RAM and free disk; warn below 4 GB free disk or 8 GB RAM, and choose the memory profile (section 4.4).
2. Find the installed `claude` binary (resolve the npm shim or the native installer's layout to the real `.exe`; never the copy bundled in the Claude desktop app), check its version against the tested range, and run `claude auth status`. If it is not signed in, say "Please run `claude auth login` yourself, then run setup again" and stop that part. Never read credential files, never run a login.
3. Show every model's license and total size and ask once; download into `%LOCALAPPDATA%\eileen\models\`, verify each SHA256, and delete anything that fails. Chatterbox is not downloaded here; it is offered when the user picks it for a voice.
4. Create the Start menu shortcut with Eileen's AppUserModelID (needed for toasts) and, if the user wants it, the "start at sign-in" `HKCU` Run entry.
5. Write the commented default `eileen.toml` if none exists.
6. Offer the Claude plugin (prints the two official commands, runs them after a yes) and the Playwright browser extension (link only).
7. Self-test: EcoQoS opt-out applied, capture, render and loopback devices open, the loaded `onnxruntime.dll` is the shipped one, models load, one Kokoro clause synthesizes, one Pocket TTS clause from a bundled stock reference is timed (the first run's surprise path uses this number), the brain session reaches `system/init`; then Eileen says hello and the first run (E18) begins on the next `eileen run`.

`eileen doctor` repeats steps 1, 2 and 7 without changing anything, plus `eileen doctor resolve` (bridge `hello` when Resolve runs, and the exact preference to change if External scripting is not set to Local), `eileen doctor desktop` (DPI awareness, UIA thread health, Raw Input) and `eileen doctor audio` (devices, AEC mode and reference, measured idle CPU).

### 4.3 Latency budget per stage

From the end of the user's speech. "Quiet" is the best measured runs; "typical load" is the owner working (Chrome, VS Code and the Claude app open), which is what he will feel. The voice-stack, brain-latency, computer-use and subscription-harnesses reports are the sources; every row is measured again by WP9's benchmarks, with the resolved model id recorded beside every model timing.

| Stage | Quiet | Typical load | Measured basis |
|---|---|---|---|
| Silence before the end-of-turn check | 200 ms (setting) | same | design |
| Smart Turn features plus model | 70 to 100 ms, overlapping the next row | up to about 250 ms | four threads, 2026-10-11 |
| Nemotron tail flush (final transcript) | about 400 ms (740 ms if Parakeet is also loaded) | 0.5 to 1 s | 2026-10-11, 2026-10-20 |
| Addressee gate plus L1 resolver | under 1 ms | same | matcher measurement |
| L2 semantic (embedding round trip plus comparison) | under 25 ms | to measure | WP4 and WP7 |
| Earcon when routed to the brain or a job | under 150 ms | same | design |
| Fast-path action | window move 8 to 10 ms; SMTC control 1 to 50 ms; app launch by AUMID to measure | SMTC up to 2.2 s with a fresh manager, so one is held | computer-use, app-hooks |
| Cached phrase to first sound | one 10 ms buffer | same | 2026-10-20 |
| **Fast path, end of speech to first sound** | **about 0.5 to 0.6 s** | **about 0.8 to 1.2 s** | sum |
| Brain warm turn, first text | 1.0 to 1.5 s | 2.2 to 4.2 s | brain-latency, connectors off |
| TTS first clause (three to six words) | about 0.5 s with Kokoro direct; WP4 measures the shipped sherpa-onnx path on a quiet PC | 1.1 to 2.9 s (2.88 s p50 per sentence at 73 percent load; 4.5 to 13.3 s while paging) | voice-stack |
| **Brain turn, end of speech to first word** | **target 1.5 s p50; about 2 to 3 s today** | **3 to 5 s** | sum |
| Paraphrase through the brain's `act` tools, end of speech to first effect | about 3 s | 4 to 6 s | sum; B9 phase |
| Worker start, warm pool | 1 to 2 s to the first turn | 2 to 5 s | one user turn on a warm session |
| Worker start, cold | 11 to 30 s | 30 to 111 s | learning-loop row 7 (34.5 s), brain-latency (39 to 48 s cold), subscription-harnesses (1.2 to 1.6 s to init on a light day) |
| Barge-in, speech onset to silence | at most 150 ms (designed 70 to 110 ms) | same; the stop is checked by invariant 2 | measured parts |
| Mute, key to capture stopped | one 10 ms buffer | same | stop flag seen 1.5 ms after it was set |
| Dictation, key release to text in the field | 0.4 to 0.8 s (local path) | to measure | dictation report |
| Computer-use step (screenshot, model turn, action) | seconds per step, dominated by the model turn | longer | claude read a screenshot in 3 turns and 6.2 s |
| Deep answer | tens of seconds; never blocks the conversation | same | design |

The owner asked for "within about a second" from the fast brain. The fast path and cached acknowledgements reach that; the earcon gives the feel of being heard at once; a synthesized brain answer takes about two seconds on a quiet machine because the first clause costs 0.5 s or more. The levers, in order: a short first clause, cached phrases (including per-app launch phrases rendered after indexing), WP4's measurement of the shipped Kokoro path and of Kokoro int8 under equal load, the Pocket TTS v4 small models if they exist and measure well, and speculative brain turns on stable partials (an opt-in experiment, section 2.3). Benchmarks publish p50 end of speech to first word, quiet and typical.

### 4.4 Memory budget

| Process | Budget | Basis |
|---|---|---|
| `eileen.exe` | under 60 MB working set while idle | the debug skeleton idled at 26.6 MB |
| `eileen-speech.exe` | about 1.25 GB with VAD, Nemotron, Smart Turn and Kokoro; plus about 0.6 GB while Pocket TTS is loaded; plus about 0.75 GB while Parakeet is loaded; Chatterbox by WP4's measurement | voice-stack |
| `eileen-desktop.exe`, `eileen-apps.exe`, `eileen-hud.exe` | under 60 MB each while idle | proof server 4.9 MB idle; to measure |
| Brain plus hot spare | two `claude -p` processes, 128 to 750 MB each as measured | harness and claude-mods reports |
| Warm pool | one or two `claude -p` processes | same |
| Each bound worker | one `claude -p` process, plus its build if it runs one (a cold Rust workspace build took 4 to 5 GB at 6 jobs) | same, rust-architecture |

The owner's real working set leaves little: a live read on 2026-10-20 showed 1.27 GB free with one `rustc` running, Chrome at 2.95 GB working set, the Claude processes at 3.6 GB private, a WSL virtual machine at 4.1 GB private and VS Code at 1.3 GB private. So memory is managed, not assumed:

- **Admission** (section 2.5): no job starts if it would leave less than `memory.reserve_mb` free; the pool and the spare are dropped first; build-heavy coding jobs take a build slot.
- **Profiles.** `memory.profile = "auto"` picks `low` when free memory stays under 3 GB for a minute at start: Kroko instead of Nemotron if its license checks out (else Nemotron stays), Kokoro int8 if WP4's equal-load test shows it is not slower, no hot spare, no warm pool, Parakeet never loaded automatically, Chatterbox not offered for live use. The HUD shows a "low memory" indicator and `eileen doctor` explains.
- **Residency.** One ASR model resident; the cloned voice engine and Parakeet load on demand; `speech.pin_working_set` (off by default) keeps the speech process's working set from being trimmed during silences, after WP4 measures the cost of the first command after 30 idle minutes.
- B1 runs with the hot spare off, because Resolve Studio itself needs several gigabytes.

### 4.5 Idle budget

Idle and listening, unmuted, with nothing playing, measured with real WASAPI capture (not only with stub sidecars):

| Process | Idle CPU (one logical core) | Basis |
|---|---|---|
| `eileen.exe` | under 2 percent with AEC bypassed (loopback silent); about 6 percent extra while AEC runs (592 us per 10 ms frame), plus the loopback capture's own cost, which WP4 measures | voice-stack, design |
| `eileen-speech.exe` | under 8 percent (VAD alone measured 5.8 percent) | voice-stack |
| `eileen-desktop.exe`, `eileen-apps.exe` | under 1 percent each (event-fed, no polling) | to measure |
| `eileen-hud.exe` | under 1 percent idle; measured again while updating ten times a second | WP8 measures |
| `claude -p` sessions | near zero between turns | harness report |

No growth in working set, handles, GDI objects or threads after warm-up. The soak tests in section 4.7 enforce slopes, not absolute numbers.

### 4.6 What runs where

A `plan` job classifies each pull request by the paths it touches: `docs-only`, `scenarios-only`, `neutral-rust`, `windows-rust` (a `-win` crate, a `cfg(windows)` module or a native dependency), `plugin`, `workflow`. Jobs whose inputs are untouched skip cleanly, and `all-green` counts a skip as a pass.

| Lane | Contents | Budget |
|---|---|---|
| Every push to a pull request | **Linux:** fmt, typos, cargo-deny, cargo-machete, zizmor, `cargo xtask lint-repo`, `cargo xtask codegen --check`, Markdown lint, internal links, clippy and nextest of neutral crates, simulator PR seeds and stored regressions, exhaustive turn-taking to depth 6, fuzz corpus replay, `cargo fuzz build` when a parser changed, loom when `eileen-audio` changed, shuttle tests when `eileen-runtime` changed, scenario validation and scenario-mode replay of every sim-lane scenario (sharded by hash beyond a 2-minute budget), the `evidence` job for fix pull requests, `cargo mutants --in-diff` (advisory). **Windows** (skipped for `docs-only` and `scenarios-only`): clippy `-D warnings` on the workspace, nextest for `-win` crates and Windows-only tests (modules named `win`), doc tests, end-to-end smoke scenarios with the stub harness and transcript injection. **`pr-meta`** (its own workflow, re-run when the description or labels change) and `claude-plugin` (when `integrations/claude-plugin/` exists) | under about 15 minutes with a warm cache |
| Merge to `main` | Full Windows nextest of the workspace, random simulator seeds for 10 minutes, a thin-LTO release smoke build, cache saves. There is no merge queue on a user-owned repository, and branches are up to date before merging, so the pull request lane is not repeated | 20 minutes |
| Nightly | Simulator random seeds per profile for 30 minutes (Linux, plus one Windows run), fuzzing 10 minutes per target, exhaustive depth 8, the full end-to-end set not marked `requires`, a 30-minute soak, flake hunt (`retries = 2`, `flaky-result = "fail"`), shuttle and loom suites, coverage, external link check, `cargo deny check --all-features`, build timings, models downloaded from their pinned URLs and verified (never kept in the Actions cache), the real speech process on cached WAVs, the credential-free CLI drift job (below), fleet-results ingest and clustering, the scenario catalogue and benchmark results rendered to the repository's GitHub Pages site (section 4.8). Failures open or update one issue per failing job, labelled `nightly`; ordinary test failures on `main` go to the `flake` issue | about 1 hour per job, in parallel |
| Long runs on demand (`long.yml`) | `workflow_dispatch` with inputs (ref, fuzz targets, minutes, simulator profiles, soak hours), and pull request labels `fuzz-10m` and `sim-30m` that run the changed targets on the pull request's head and post a summary; packages link these runs as evidence for their "clean for 10 minutes" acceptance lines | as requested |
| Weekly | Full mutation run (four shards), 4-hour soak, one hour per hot fuzz target, a dependency update pull request respecting the 14-day cooling-off period (protocol crates by hand with the probe subcommands as smoke tests), a job that reads the pull request lane's durations through the API and opens an issue when the p50 passes 15 minutes | hours |
| Release (tag `v*`) | Everything above on the tagged commit; `cargo build --release` with `CARGO_PROFILE_RELEASE_LTO=fat` and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` on `windows-latest`; `.pdb` files and a Breakpad symbols zip; checksums; a license listing that includes the shipped native DLLs; end-to-end smoke from the built artifacts; a draft release the owner publishes | owner signs off |
| Nightly prebuilt (if the owner allows it, section 7) | An unsigned zip of the thin release build with checksums, published as a `nightly` pre-release so testers need no Rust toolchain | part of nightly |
| Owner's PC, nightly when idle (opt-in) | `cargo xtask nightly-local` from Task Scheduler with keep-awake if allowed: benchmarks B1 to B9 with real Claude within a nightly turn budget, the stream-json and approval contract tests, the skill-listing probe, weekly an overnight soak. Desktop scenarios run only attended (a "hands off for 10 minutes" countdown) or after the owner armed the night ("run the nightly tonight"); they skip a locked session, stop at physical input, auto-deny every tier-2 approval with voice approval off, use only owned fixtures and no signed-in accounts, and lock the workstation when an armed run ends. Skips when free memory is under 2 GB or the owner used the PC in the last 30 minutes. Results to the devhub store | the owner's plan limits |
| Contributors' PCs (opt-in) | `eileen test --fleet` (section 4.8), with the same attended or armed rules | the contributor's choice |

**CLI drift, credential-free (nightly).** Install the pinned and the latest claude CLI from npm into separate prefixes, parse `claude --help` for every flag listed in `eileen-harness`'s flag constant, check that a hook settings file and the generated MCP config are accepted, and run `claude plugin validate --strict` on `tools/claude-plugins/eileen-dev` and the plugin where that works signed out. A mismatch opens an issue. The owner's-PC contract test (real sign-in) runs nightly in his lane.

**Cache budget.** GitHub gives a repository 10 GB of Actions cache. Windows keeps two keys (`win-clippy`, `win-test`), Linux one (`linux`), fuzzing one, each saved only from `main` and restored elsewhere; nightly jobs restore and never save; models are never cached. `rust-toolchain.toml` lists no extra targets, so Linux jobs do not download the Windows standard library.

The owner's PC is never a self-hosted runner for this public repository: a fork's pull request could run code on it. First-time contributors' workflow runs need the owner's approval (GitHub's default, kept on).

### 4.7 Test strategy

**Layers.**

| Layer | Proves | Tool | Runs on |
|---|---|---|---|
| Unit and property | One module, including invalid input | `#[test]`, proptest | every OS |
| Snapshot | Wire formats, tool schemas, HUD draw lists, user-facing text | insta (`--unreferenced reject`) | every OS |
| Integration per crate | A crate through its public API, one test binary at `tests/it/main.rs` | nextest | every OS for neutral crates, Windows for `-win` crates |
| Exhaustive | Turn-taking and barge-in for every input sequence up to a length | own enumerator | Linux |
| Deterministic simulation | The whole core, including the skill engine and memory admission, under random inputs and injected faults, checked by invariants, replayable from a seed | `eileen-sim` | Linux on pull requests, both OSes nightly |
| Concurrency | The tokio glue (bus fan-out with a slow subscriber, a supervisor restart racing a cancel, two workers finishing at once, a shim pipe dropping mid-call), deadlines, heartbeats and backoff with paused time, and the audio ring buffer | paused tokio time, shuttle (WP2), loom (WP4) | Linux |
| Fuzzing | Untrusted parsers never panic, stay bounded, round-trip | cargo-fuzz, corpus replay tests | fuzzing on Linux, replay everywhere |
| End to end | Real Eileen processes, real desktop, recorded or synthesized audio in, event trace out | `eileen-scenarios` runner | `windows-latest` with stubs; the owner's PC and fleet PCs with real Claude and apps |
| Timed scenarios | The owner's benchmark requests stay fast | `eileen-e2e --bench`, A/B against the base binary | the owner's PC |
| Soak | Memory, handles, GDI objects, threads, idle CPU, orphans, log and store sizes | `cargo xtask soak` | `windows-latest` nightly and weekly; the owner's PC weekly |
| Mutation and coverage | Tests check what they cover | cargo-mutants, cargo-llvm-cov | Linux, coverage also Windows |

**Tests that need the owner's PC or real Claude** live in modules named `owner_pc::` or `real_claude::`. Nextest's `default-filter` excludes them in the `default` and `ci` profiles; the `owner` profile, which `cargo xtask nightly-local` and `cargo xtask test --owner` use, includes them; xtask prints how many it excluded. `#[ignore]` is banned, and a check fails if such a test lives outside those modules, so a skip never looks like a pass.

**Simulation.** The core is built for it from the first commit (section 3.3.3): one `Core::step`, closed `Input` and `Effect` enums, time and randomness injected, ordered maps, every queue bounded, `buggify!` fault points activated per run (a runtime switch, never a Cargo feature), `never!` and `always!` soft assertions that log, count and dump the flight recorder in production and panic under tests. `eileen-sim` owns a `World` of simulated parts (microphone and speech with several sources, other audio playing, playback in 10 ms callbacks, physical input, the brain with delay distributions and failures, workers replaying stub scripts with faults, sidecars with latency, hangs past the deadline and crashes, Job Object semantics, memory pressure, an in-memory model of the store and of the learning store). Each run picks a profile, seeds a PRNG, generates inputs from an `arbitrary::Unstructured` buffer (so the same generator is a seeded test, a fuzz target and a minimiser), steps the core, applies effects to the world and checks every invariant after every step. A failure prints the replay command with the commit; `--doublecheck` runs a seed twice and compares trace hashes; minimised failures are stored as `SimRegression` files (decoded inputs with every fault as an explicit record) in `crates/eileen-sim/regressions/` and replayed on every pull request; pull requests use a fixed seed set, `main` and nightly use fresh seeds. The simulator thread gets an explicit large stack (Windows' default main-thread stack is small). Nextest gives the simulator, `turn::exhaustive` and end-to-end tests longer slow-timeouts (section 4b.3), and the PR seed set is split into one test per seed.

Invariants checked after every step (each a named function):

1. The voice path is never starved: every voice input is consumed within two audio frames of virtual time.
2. Barge-in: after the first speech window while Eileen speaks, playback is ducked by the next output callback and paused by the third window; onset to silence never exceeds 150 ms of virtual time. A pause resolves within 600 ms to a resume or a cancel.
3. What the brain is told the user heard equals the text mapped from samples actually handed to the device.
4. Every job ends in exactly one terminal state within its deadline plus the supervisor's kill bound; no job stays running without a live worker past one heartbeat timeout.
5. Job state changes follow the legal graph (also checked against a proptest-state-machine reference model).
6. Every bus event carries a turn ID or a job ID where it belongs; nothing is emitted for a job after its terminal event; per-job order is preserved; a slow subscriber gets an explicit `Lagged`, never silent loss; no unaddressed speech becomes a bus event.
7. A tool call above tier 1 is always preceded by an approval built from `UserInput` (for tier 2 by voice, with the action word or another allowed form, and never while voice approval is off); a replayed skill never raises a tier; a learned skill never skips a confirmation; once a job is tainted and has read private data, every egress-capable call is tier 2; an ambient brain turn never runs a non-read tool.
8. A crashed or hung child restarts with bounded backoff, at most N times per window; killed children leave no descendants.
9. Mute stops audio reaching recognition within one frame; nothing is spoken while muted; "cancel" reaches the worker as an interrupt within its bound; physical input during a lease stops synthetic input within one event.
10. Two utterances never overlap; the announcement queue stays bounded.
11. Two speeds: the deep answer either replaces the fast one or goes to the HUD; the user never hears both in full; an interruption sends the deep answer to the HUD.
12. After a simulated core restart, running jobs are marked interrupted and no ID repeats.
13. The span-parent table holds for every span the run produced, and every log line inside a turn carries its turn ID.
14. Memory admission: no job starts when the simulated free memory after its estimate is under the reserve; at most `max_build_heavy_jobs` build-heavy jobs and two builds run at once.

Exhaustive tests enumerate every sequence of the turn-taking inputs (speech start, speech end, end of turn, clause ready, playback tick, mute, unmute, brain text, a backchannel, other audio starting, physical input) up to length 6 on pull requests and 8 nightly, checking invariants 2, 3, 9 and 10.

**End to end without a sound card or credentials.** `eileen-audio` defines `AudioSource` and `AudioSink` from day one: production uses WASAPI; tests use `WavSource { path, pace }`, a `CaptureSink` that writes what Eileen says to a WAV, and a `RoomMixer` that feeds a delayed, attenuated copy of playback, plus an optional music or speech-video bed, back into the input so echo cancellation and barge-in see real bleed. `eileen run --audio-source wav:<file> --audio-sink capture:<file>` selects them. For pull requests, a transcript-injection speech stub replaces the models (the hosted runner has no audio device and the models are 1.3 GB); nightly runs the real speech process with models downloaded and verified. `eileen-stub-harness` speaks the same stream-json as `claude -p` and plays `StubScriptLine` scripts from `tests/harness-scripts/*.ndjson` (section 3.3.16); the scripts start as redacted recordings of real sessions on the owner's PC. The contract test on the owner's PC runs the same prompts through the real CLI nightly and compares event shapes (types and fields, not text) with the scripts, plus the approval cases of section 2.5, so CLI changes show up as a failing check. CI never holds vendor credentials. Desktop tests run in one nextest group with `max-threads = 1`, use only windows the test created (owned fixtures such as a corrected testpad, isolated Edge profiles, Notepad launched by AUMID and closed unsaved), and the first nightly proves the hosted runner gives UIA an interactive desktop before more depend on it. Small synthetic WAV fixtures (two distinct synthetic speakers) are committed for the enrollment and speaker tests, so CI never runs a TTS model to make them.

**Benchmarks** (timed scenarios, phases measured from event-log spans, judged by A/B against the base binary with alternating runs and a Mann-Whitney test, reporting only differences that are significant and larger than about 10 percent; runs with under 2 GB free memory are recorded and excluded):

| ID | Request | Phases | Where |
|---|---|---|---|
| B1 | "Open the video editor, go to 15:30 and make the face of the girl on the right less blurry" | end of speech to Resolve ready (including a cold launch if needed); to playhead at 15:30 (read back through the bridge); to the crop confirmation; to the correction applied; sharpness gain (pass at 15 percent or more plus the owner's yes); total; worker turns; hot spare off | owner's PC |
| B2 | The Photoshop beach request | same shape | skipped until Photoshop is present (`requires = ["photoshop"]`) |
| B3 | "Launch three agents to fix the flaky login tests in the demo repo" (the `flaky-login` fixture) | warm or cold start per agent; first narration per job; all finished; HUD shows all three; peak memory | CI with stubs; owner's PC with real Claude |
| B4 | Writing code by speaking | end of speech to text in the editor; word error rate against the script; "that variable" resolved correctly | both |
| B5 | Interrupting Eileen mid-sentence, and a backchannel | onset to silence; correctness of the heard prefix; backchannel resumed | both, strict on the owner's PC |
| B6 | "What's the best place to eat near me" | fast answer start; no unsourced current facts in it; deep answer arrival; spoken length; HUD content | owner's PC (network, location set by voice) |
| B7 | Dictation into a field | key release to text; filler and self-correction accuracy | both |
| B8 | The same hard request kind, learning off against learning on | A/B on the same machine: a fresh data directory (learning off) against the learned skill, alternating, at least three pairs, each pair with new slot values (15:30 right, 42:10 left, 1:02:10 largest); time, tokens, worker turns, success | owner's PC (three real-Claude B1 pairs at most, to spare the plan) |
| B8-curve | A cheap recipe-eligible request repeated 12 times (YouTube play-at on a phrasing outside the grammar, or a testpad fixture skill) | per run index: level, model turns, tokens, duration, success, handoff; must match the table in section 2.10 | owner's nightly lane |
| B9 | "Open Premiere", "pause", "snap this to the left half", and a paraphrase ("get Premiere up for me") | end of speech to effect: fast path, and through the brain's `act` tools (about 3 s) | both |

Every run appends one JSON line (scenario, phase, milliseconds, commit, binary hash, claude CLI version, resolved model id, free RAM, CPU load, whether the owner was active) to the devhub results store. The nightly Pages build renders a benchmark results page from it.

**Fuzz targets**, each calling a `#[doc(hidden)] pub mod fuzz_entry { pub fn <target>(data: &[u8]) }` in a neutral crate, each with a minimised seed corpus (size-capped, checked) that the owning crate's `tests/it/fuzz_corpus.rs` replays as an ordinary test on every OS, together with `fuzz/regressions/<target>/`. The corpus nightly fuzzing grows is kept as an artifact, not committed.

| Target | Input | Owner |
|---|---|---|
| `stream_json_line`, `control_message` | lines from `claude -p` | WP3 |
| `mcp_client_message`, `mcp_tool_arguments`, `dispatch_line` | JSON-RPC from sidecars, tool arguments from workers, shim pipe lines | WP2 |
| `worker_hook_payload`, `http_request` | hook bodies and the loopback HTTP parser (fail closed) | WP2 |
| `scenario_file` | contributed scenarios and their `inputs.jsonl` lines | WP2 |
| `uia_text`, `desktop_state` | UIA strings through `sanitize`, DesktopState snapshots | WP0 (contracts) |
| `config_toml` | the user's config files | WP7 |
| `skill_record` | learned skill files, front matter and `recipe.json` | WP7 |
| `command_grammar` | transcripts into the fast path | WP7 |
| `speech_frame` | speech pipe frames | WP4 |
| `dictation_cleanup` | transcripts into the cleaner | WP4 |
| `hud_line` | HUD protocol lines | WP8 |
| `resolve_reply` | Resolve bridge replies | WP6 |
| `fleet_result` | fleet result lines and fleet issue comments | WP9 |
| `recording`, `expected_trace` | recording input lines, expected-trace files | WP1 |
| `sim` | simulator scenarios from bytes, checked by every invariant | WP1 |

`cargo fuzz build` runs on every pull request that touches a parser crate; a crash is minimised into `fuzz/regressions/<target>/` with a focused regression test before its fix counts. A bug found any other way in a fuzzed parser is first treated as a gap in the fuzz target.

**Soak.** `cargo xtask soak` runs the real processes with WAV input and stub workers, plays a scripted request every few minutes, samples once a second through `cargo xtask measure-idle`'s sampler, and judges slopes after a 10-minute warm-up: working set and private bytes (under 1 MB per hour for the core), handles, GDI and USER objects, threads, idle CPU (within the table of section 4.5), live children (exactly the expected set), log, SQLite and recorder sizes (within caps), and the scripted request's latency (no upward trend).

### 4.8 Real use becomes tests: the community scenario corpus and the test fleet

The owner's friends install Eileen on their own PCs and test it. Everything they and the owner try feeds the test suite, so the suite grows from real use instead of only from what developers think of. Capture and sharing ship with slice S1 (WP2), so friends can contribute from the first build they can run; the fleet arrives in M6 (WP9).

**Capture.** "Eileen, save that as a test", the HUD's Save button or `eileen scenario save [--last | --turn ID | --job ID] [--failed]` turns a request into a scenario folder under `%LOCALAPPDATA%\eileen\scenarios\saved\<id>\`:

1. The flight recorder's input lane is trimmed to that turn and its jobs (`eileen record trim`), together with the setup inputs a fresh core needs to reach the same starting state, which gives the sim-lane replay (`replay/inputs.jsonl`, plain JSON lines, and `replay/expected.trace`). Speech that was not addressed to Eileen is already a placeholder in the lane.
2. The utterance is stored as text. The person's real voice is stored only if they explicitly allow it and record the consent sentence; otherwise runners regenerate audio with a stock voice.
3. The starting state becomes fixtures where Eileen can describe it generically (which app was focused, how many monitors, which app version), drawn only from fixture templates already in the repository; steps come from the trajectory.
4. Expectations are inferred from what changed: a window moved becomes `window_on_monitor` or `window_snapped`; inserted text becomes `field_text`; a changed file becomes `file`; a Resolve playhead becomes `resolve_playhead`; a media seek becomes `media_position`; a setting becomes `setting_equals`; the dispatcher's calls become `call` expectations. The person can say "and it should also ..." to add one.
5. A failure is saved the same way with `outcome = "expected_fail"`; Eileen asks once "what should have happened?" and stores the answer as `intent.expected`, and the incident bundle is attached locally.

**Generic and private.** Nothing leaves the PC until `eileen scenario share <id>`, which generalises and redacts: every value wrapped as `Personal` or `Untrusted` in the contracts (section 3.3.1) is replaced by a placeholder (`{HOME}`, `{DOCS}`, `{WORK}`, `{NAME}`, `{TITLE}`), the Redactor runs over every line, unaddressed utterances are replaced with stock sentences of the same length, window titles keep only the app name, screen text is removed and screenshots are off. Then `share` replays the redacted copy in scenario mode locally: if it no longer passes, the scenario is offered for the desktop lane only, never with a sim replay that tests something else. A preview shows exactly the files that would be shared, with every redaction highlighted, in the terminal and on the HUD. Only after the person approves is the result written to `scenarios\outbox\<id>\` with `origin = "synthetic-from-user"` (or `contributed-consented-audio` when audio is included with its consent record).

**Sharing.** `eileen scenario submit <id>` uses the contributor's own GitHub account through the `gh` CLI they signed in to themselves (Eileen never handles GitHub credentials, the same rule as for vendor CLIs): it forks if needed, creates a branch `scenario/<id>`, adds `scenarios/<area>/<id>/`, and opens a pull request whose title and filled scenario template (type `scenario`, evidence: the local replay result) satisfy `pr-meta`, after one confirmation (publishing is tier 2). Without `gh`, it copies the folder as a zip and opens the scenario issue form (`.github/ISSUE_TEMPLATE/scenario.yml`), whose fields map one to one to the schema (utterances, app, requires, steps, expectations), so a contributor can also fill it by hand; `cargo xtask scenarios from-issue <n>` turns a form into a draft pull request for a maintainer.

**CI on scenario pull requests.** The `plan` job classifies a pull request that touches only `scenarios/<area>/<id>/` as `scenarios-only`: the Windows jobs skip, and Linux runs `cargo xtask scenarios check` (the schema, the privacy rules: no `origin = "user"`, only placeholders in personal fields, the Redactor and a PII scan over every input line for e-mail addresses, phone numbers, `C:\Users\<name>`, machine names and token prefixes, no audio without a consent record and a synthetic or consented marker, no binary files and no fixtures that are not repository templates, size caps, an area that exists, event names from the generated list) and `cargo xtask scenarios replay` in scenario mode. A contributed scenario enters the desktop and fleet lanes only after a maintainer adds the `scenario-reviewed` label (`CODEOWNERS` routes `scenarios/` to the owner), because a merged scenario is code that real desktops run.

**The fleet.** Contributors opt in once (`eileen test --fleet` asks for consent and writes `fleet.opt_in`). The runner pulls scenarios from the installed version's tag and keeps those whose `requires` the PC meets. It runs in one of two modes, recorded in every result: **recorded** (the default) starts Eileen with `--replay-models`, so recorded brain and worker responses are served reactively while the real sidecars act on the real desktop with selectors re-resolved on this machine and no model calls, which tests exactly what a fleet uniquely covers (DPI, monitors, app versions, locale); **live** (opt-in, with a turn budget) uses the contributor's own Claude plan. Runs use a throwaway data directory and profile and owned fixtures (never the user's documents, browser profile or signed-in accounts); start only when the user has been idle for 30 minutes and no call or full-screen app is active; are attended (a "hands off for 10 minutes" countdown) unless the contributor armed the night; skip a locked session; stop at physical input; auto-deny every tier-2 approval with voice approval off; and lock the workstation when an armed run ends. A nightly schedule is a per-user Task Scheduler entry created only after an explicit yes. Results go to `fleet\results\<date>.jsonl` as `FleetResult` lines (section 3.3.16): pass or fail per expectation, phase timings, Windows build, monitors, DPI, app versions, locale, CPU class, RAM, free RAM, whether the user became active, anonymous learning statistics and a random install ID (never hardware IDs, user names or paths). The contributor sees the file and approves the upload, which posts it from their own account as a comment on the week's "Fleet results" issue (label `fleet`).

**Learning from it.** A nightly workflow ingests the week's fleet comments into the devhub store, accepting comments only from accounts listed in `.github/fleet-contributors.txt` (added by pull request) and treating every comment body as untrusted data (the `fleet_result` fuzz target covers the parser). It clusters failures by signature (scenario, failing expectation, error kind, first divergent event, app version, Windows build); each cluster gets one issue with counts and machine classes. Triage by a Claude worker runs on the owner's PC (`cargo xtask triage <issue>`), reading the redacted cluster data, the replay and the code map, and drafting an explanation that points at the code; the deterministic clustering stays in CI. A fixed bug always leaves its scenario behind: a pull request of type `fix` must add or change a file under `scenarios/`, `fuzz/regressions/`, `crates/eileen-sim/regressions/` or a test file, or `pr-meta` fails. Flaky scenarios (some failures and some passes on the same machine class over the last 20 runs) are flagged in the catalogue and tracked in the `flake` issue; quarantine needs an issue link and an expiry date, and an expired quarantine fails CI. Learned skills are checked across machines through the fleet before they can ever be offered for sharing (v0.3).

**Visible.** The nightly workflow renders the scenario catalogue to the repository's GitHub Pages site (and attaches it as an artifact): scenarios grouped by app, each with the utterance people actually say, what it checks, its last result, how many machines passed it and whether it is flaky, plus a coverage view (app against capability, with scenario and machine counts) and the benchmark results page. Nothing about results is committed, so a pull request's checks never depend on what nightly ingested and friends' scenario pull requests never need to regenerate a file. `README.md` and `TESTING.md` link to the site.

**For testers.** `TESTING.md` (WP2, with slice S1) tells friends how to install (the nightly prebuilt zip if the owner allows it, otherwise a build), that most scenarios need their own Claude plan signed in on their own CLI and spend its allowance, which scenarios run with no Claude at all (fast path, dictation, desktop and apps with the recorded mode), what to try (a short list per app, from the catalogue), how to say "save that as a test", how to share a scenario, how to run the fleet suite, and exactly what leaves their PC. `docs/FLEET.md` (WP9) is the maintainers' side: ingest, clustering, triage and quarantine.

**Who builds what.** WP0: the schema, the validation and privacy checks, the issue and pull request templates, the CI lanes and path classes, the first example scenarios. WP1: both replay modes, the expectation and expected-trace matcher, the recording formats. WP2: save, share, submit, the real-desktop runner, `TESTING.md`. WP9: the fleet runner and upload, ingest, clustering, flake tracking, the Pages catalogue data, the triage command, `docs/FLEET.md`. Every package ships scenarios for what it builds in its own `scenarios/<area>/`.

## 4b. The spotless bar as configuration

WP0 checks these files in; every later package leaves them green and never weakens them without a decision record.

### 4b.1 Toolchain, formatting, profiles, line endings

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.99.0"
components = ["rustfmt", "clippy"]
profile = "minimal"                 # no extra targets: the host target is installed anyway, and Linux jobs skip the Windows std
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

[profile.release]                   # local checks, CI smoke builds and tagged releases
opt-level = 3
lto = "thin"                        # release.yml overrides with CARGO_PROFILE_RELEASE_LTO=fat and CODEGEN_UNITS=1
panic = "unwind"                    # the core survives a panicking task
debug = "line-tables-only"
incremental = false
strip = "none"                      # MSVC keeps symbols in the .pdb, which ships to the symbol store
```

There is no custom `dist` profile: `sherpa-onnx-sys` locates its output by the profile name and fails under one (section 4.1), so tagged releases use `release` with the two environment overrides, in CI only, never on the owner's PC.

Once the speech crates exist, WP4 narrows `opt-level = 2` to the crates that need it if `cargo build --timings` shows proc macros built at opt-level 2 dominating.

`.cargo/config.toml`:

```toml
[alias]
xtask = "run --package xtask --"

[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

Build parallelism (`CARGO_BUILD_JOBS`) is not set in the repository, so contributors with bigger machines and CI are not throttled; the build guard sets it from measured free memory on the owner's PC (section 6.1). The native-library environment (`SHERPA_ONNX_ARCHIVE_DIR`, `ORT_LIB_LOCATION`, `ORT_PREFER_DYNAMIC_LINK`) is not in this file either, because the cache path differs per machine; `cargo xtask` sets it for every cargo call (section 4.1).

`.gitattributes`:

```gitattributes
* text=auto eol=lf
*.rs text eol=lf
*.toml text eol=lf
*.md text eol=lf
*.json text eol=lf
*.ndjson text eol=lf
*.jsonl text eol=lf
*.trace text eol=lf
*.setting text eol=lf
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
  { path = "std::thread::sleep", reason = "blocks a runtime worker; use tokio::time::sleep, or eileen_rt::thread::sleep on a named thread" },
  { path = "std::thread::spawn", reason = "use eileen_rt::thread::spawn_named: every thread has a name in logs and dumps" },
  { path = "std::env::set_var", reason = "process-global and unsound with threads" },
  { path = "std::env::remove_var", reason = "process-global and unsound with threads" },
  { path = "std::process::Command::new", reason = "start children through eileen_supervisor (Job Object, kill on close)" },
  { path = "tokio::process::Command::new", reason = "start children through eileen_supervisor", allow-invalid = true },
  { path = "std::process::exit", reason = "skips destructors and log flushing; return from main" },
  { path = "std::time::Instant::now", reason = "core logic gets time from the clock port; everything else uses eileen_stdx::clock" },
  { path = "std::time::SystemTime::now", reason = "core logic gets time from the clock port; everything else uses eileen_stdx::clock" },
  { path = "ulid::Ulid::generate", reason = "IDs come from the Ids port so replay is deterministic", allow-invalid = true },
  { path = "tokio::sync::mpsc::unbounded_channel", reason = "every queue has a maximum", allow-invalid = true },
  { path = "tokio::spawn", reason = "use eileen_rt::task::spawn: it names the task, keeps the span and registers it", allow-invalid = true },
  { path = "tokio::task::spawn", reason = "use eileen_rt::task::spawn", allow-invalid = true },
  { path = "tokio::task::spawn_blocking", reason = "use eileen_rt::task::spawn_blocking (named, bounded pool)", allow-invalid = true },
]
disallowed-types = [
  { path = "std::sync::mpsc::Sender", reason = "use bounded tokio::sync channels" },
]
disallowed-names = ["foo", "bar", "baz", "tmp", "data2"]
```

The one real clock and ID generator (in `eileen-stdx`), the spawn and thread wrappers (in `eileen-rt`) and the supervisor's spawn each carry an `#[expect(clippy::disallowed_methods, reason = "...")]`. `crates/eileen-core/clippy.toml` is the root file plus a `disallowed-types` entry for `std::collections::HashMap` (use `BTreeMap` or `IndexMap` where order can reach an effect); clippy reads the nearest file and does not merge, so a tidy check keeps the two in sync. Lookup-only maps are fine elsewhere.

**Unsafe policy.** The workspace denies `unsafe_code`. Every crate without FFI adds `#![forbid(unsafe_code)]`. An FFI crate keeps every unsafe call in one private module declared as `#[expect(unsafe_code, reason = "...")] mod ffi;`, one unsafe operation per block, each with a `// SAFETY:` comment saying which precondition holds and why, exposing only safe functions and converting every `HRESULT` and `BOOL` into the crate's error type there. COM calls run on the crate's own apartment thread with a deadline, never on a tokio worker. Prefer crates that own their unsafe code (process-wrap, tray-icon, cpal). Write "safety comment" in prose docs (the word with a colon trips `unnecessary_safety_comment`) and never link a private `ffi` module from public docs.

**Casts and indexing in numeric code.** Pedantic clippy under `-D warnings` fires on every `as` cast and every slice index in DSP, log-mel, resampling, DPI math, YuNet decoding and the Laplacian. The rule, decided once in WP0: everywhere else, use `eileen_stdx::num` (checked conversions that return a typed error or saturate, named for what they do); in modules whose job is numeric (a named list in `xtask/tidy/numeric-modules.toml`, for example `eileen-audio::dsp`, `eileen-speech::mel`, `eileen-desktop::dpi`, `eileen-apps::image`), a module-level `#![expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_precision_loss, clippy::indexing_slicing, reason = "numeric kernel; bounds checked at the module boundary")]` is allowed, and the module's public functions check lengths once at the boundary. The rmcp `#[tool_handler]` macro trips `unused_async`; one `#[expect]` per server type with the reason "rmcp tool handler signature" is pre-approved.

### 4b.3 cargo-deny, nextest and CI

`deny.toml` is the rust-architecture report's file with three changes: `IJG` added to the license allow list for `jpeg-encoder` (with the attribution in `NOTICE`); a `[[licenses.exceptions]]` entry allowing `CDLA-Permissive-2.0` for the `webpki-roots` crate only (it arrives through `sherpa-onnx-sys`'s build-time download client); and the graph targets extended to `x86_64-unknown-linux-gnu` so the Linux lane is checked too. WP0 confirms that `ring`'s `Apache-2.0 AND ISC` expression passes. Licenses allowed: MIT, MIT-0, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, 0BSD, Unicode-3.0, Unlicense, CC0-1.0, BSL-1.0, IJG. `openssl-sys` and `native-tls` are banned; unknown registries and git sources are denied; yanked versions are denied. cargo-deny cannot see downloaded native libraries or models, so the release lane lists their licenses separately.

`.config/nextest.toml`:

```toml
[profile.default]
slow-timeout = { period = "10s", terminate-after = 3 }
leak-timeout = { period = "500ms", result = "fail" }   # a test that leaves a child process running fails
default-filter = "not (test(/::owner_pc::/) | test(/::real_claude::/))"

[profile.ci]
fail-fast = false
retries = 0
failure-output = "immediate-final"
slow-timeout = { period = "15s", terminate-after = 4 }
default-filter = "not (test(/::owner_pc::/) | test(/::real_claude::/))"

[profile.ci.junit]
path = "junit.xml"

[profile.nightly]
retries = 2
flaky-result = "fail"
fail-fast = false

[profile.owner]                     # cargo xtask nightly-local and cargo xtask test --owner
default-filter = "all()"
fail-fast = false

[test-groups]
desktop = { max-threads = 1 }

[[profile.default.overrides]]
filter = "test(/::desktop::/) | package(eileen-desktop-win) | package(eileen-apps-win) | package(eileen-shell-win) | package(eileen-hud-win) | package(eileen-e2e) | test(/::e2e::/)"
test-group = "desktop"

[[profile.default.overrides]]
filter = "package(eileen-sim) | test(/turn::exhaustive/) | test(/::e2e::/)"
slow-timeout = { period = "60s", terminate-after = 5 }
```

The guard's `cargo xtask test` takes `%LOCALAPPDATA%\eileen\desktop.lock` before running the `desktop` group on the owner's PC.

Workflows, all with `permissions: contents: read` at the top (a job that must write, such as the Pages deploy, the nightly issue reporter or the nightly pre-release, names only the permissions it needs), `persist-credentials: false` on checkout, actions pinned by SHA, zizmor on every change, Dependabot for action SHAs, `concurrency` cancelling superseded runs, `CARGO_INCREMENTAL=0`, the Windows target directory on the runner's `D:` drive if it measures faster, rust-cache saved only from `main`:

| Workflow | Trigger | Jobs |
|---|---|---|
| `ci.yml` | pull request, push to `main` | `plan` (path classes, section 4.6), `hygiene` (ubuntu), `logic-linux` (ubuntu), `scenarios` (ubuntu), `evidence` (ubuntu, fix pull requests only), `windows-clippy`, `windows-test`, `claude-plugin` (only if `integrations/claude-plugin/` exists; `continue-on-error` until validation without sign-in is proven), `mutants-diff` (advisory, `continue-on-error`), and `all-green`, which runs with `if: always()`, needs every blocking job, and fails unless each succeeded or was skipped. Jobs whose inputs do not exist yet skip cleanly |
| `pr-meta.yml` | `pull_request` types `opened`, `edited`, `synchronize`, `reopened`, `labeled`, `unlabeled`, `ready_for_review` | `pr-meta` (title, template, evidence, type rules, scenario-only rules). Separate so that editing the description re-runs it without re-running or skipping the heavy jobs; it reads the body from the event payload through an environment variable, never a template expression |
| `nightly.yml` | daily, manual | the nightly lane of section 4.6, the Pages build, then `report` (one issue per failing job) |
| `long.yml` | manual (`workflow_dispatch`), labels `fuzz-10m` and `sim-30m` | long fuzzing, simulation and soak on a chosen ref |
| `weekly.yml` | weekly | the weekly lane |
| `release.yml` | tag `v*`, manual | the release lane |

The required checks on `main` are `all-green` and `pr-meta` (section 6.2 says when they replace `test (3.12)` and `test (3.13)`). The engineering-practice report's `ci.yml` skeleton (section 6.2 there) is the starting point.

**One command.** `cargo xtask ci` runs locally what the pull request lane runs (through the build guard) and prints, at the end, the CI-only steps it skipped and why (on Windows: `cargo fuzz build` and the `cfg(not(windows))` compilation; `mutants --in-diff` is never run locally because it is heavy on this PC). `cargo xtask lint-repo` runs only the fast repository checks, in seconds. `cargo xtask pre-push` runs fmt, clippy, the changed crates' tests and `lint-repo`; `.githooks/pre-push` calls it (`cargo xtask setup` sets `core.hooksPath`).

**The conventions, each enforced by a check.** `xtask` keeps a registry of checks (id, name, rule, how to fix it, the local command, the CI job, whether it is a `lint-repo` check or delegated to another tool). `cargo xtask codegen` renders the registry into the table between markers in `CONTRIBUTING.md` and into `docs/reference/checks.md`, kept fresh by check 9; the root `AGENTS.md` carries a short summary and the link. Every failure prints the check id, the file and line, the fix and the one command that reproduces it. Checks marked "delegated" have a config assertion as their fixture test (for example, that the workspace lints contain `undocumented_unsafe_blocks = "deny"`) plus one end-to-end failing fixture in the nightly lane.

| # | Convention | Check | Kind |
|---|---|---|---|
| 1 | Every crate's guidance is in its crate doc (in `src/lib.rs`; a binary's `main.rs` holds one line pointing to it) with the required headings of section 4b.4, within 80 lines | parses the doc comments | lint-repo |
| 2 | Nested `AGENTS.md` only where section 4b.4 lists one, each with a sibling `CLAUDE.md` whose first line is `@AGENTS.md` and a "Why this folder has its own rules" heading; root `AGENTS.md` at most 150 lines; root `CLAUDE.md` starts with `@AGENTS.md` | file scan | lint-repo |
| 3 | `ARCHITECTURE.md`'s code map lists exactly the workspace crates, both ways, and every backticked path or type in it resolves | `cargo metadata` and `syn` | repo-tools |
| 4 | Every `cargo xtask <command>` named in any guidance file exists | parses guidance files and xtask's command list | lint-repo |
| 5 | Markdown is linted, internal links resolve (external links nightly), spelling passes | rumdl, lychee offline, typos | delegated |
| 6 | House style: no em dash (U+2014) in Markdown, doc comments or user-facing strings; the product is only ever "Eileen"; no old working names; workers named only by route; no vendor product name used as part of an Eileen name, a feature name or a worker name; no film or trademark names. The denylist is stored as salted SHA-256 hashes of normalised tokens and two- and three-word phrases (`xtask/tidy/denylist.toml`, entries added with `cargo xtask tidy denylist-add`), so the names never appear in the repository; a failure shows the matched text from the scanned file, never from the list; dated historical records are exempt by name in `xtask/tidy/exempt.toml` with a reason each; the fixture test uses a synthetic entry | lint-repo |
| 7 | The pull request title is a valid squash subject: imperative, capitalised, under 72 characters, no trailing period. Per-commit messages are checked as advice only | `pr-meta` | pr-meta |
| 8 | The pull request description follows the template: `## Evidence` first, then What, Why, Tests at which layer, Seen failing before the fix (for fixes), Scenario, Suggested guidance changes, Architectural changes, and a `type:` field (`feature`, `fix`, `refactor`, `docs`, `scenario`, `amendment`). For feature, fix and refactor pull requests the Evidence section must list at least one test (`crate::path::test_name`) or scenario id, and every one listed must exist in the head tree (or in the diff); numbers before and after, replay ids and incident ids are optional fields | `pr-meta` | pr-meta |
| 9 | Generated files are current and hermetic (derived only from the tree, sorted, no timestamps or line counts): `docs/CODEMAP.md`, `docs/reference/*` (telemetry names, events, MCP tools, config keys, checks), `.claude/rules/crate-*.md`, the config schema and commented default, contract schemas, the scenario schema | `cargo xtask codegen --check` | lint-repo |
| 10 | Every unsafe block has a safety comment, one unsafe operation per block, and unsafe code appears only in `ffi` modules | clippy (`undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`) plus a scan | delegated plus lint-repo |
| 11 | Public items are documented | `missing_docs` under `-D warnings`, `cargo doc` with `RUSTDOCFLAGS=-D warnings` | delegated |
| 12 | Decision records follow the grammar of the existing log: `## D<n>. <title>`, then Date, Status (`accepted` or `proposed`, optionally `; amended by D<n>` or `superseded by D<n>` with further `and D<m>`, and an optional parenthetical note), optional Supersedes and Amends, Decision, Why, Consequences, Source; numbers sequential; every referenced record exists | parses `docs/DECISIONS.md` | lint-repo |
| 13 | Dependencies pass the license, advisory, ban and source policy; no unused dependencies; new crate versions respect the cooling-off period on update pull requests | cargo-deny, cargo-machete, `cargo xtask deps-age` (queries crates.io with `curl`, which ships with Windows 11 and the runners) | delegated |
| 14 | The credential boundary: no source names a vendor credential path or the persistent MCP registration command; no process spawn outside the supervisor | scan plus clippy `disallowed-methods` | lint-repo |
| 15 | Platform and native boundaries: neutral crates do not depend on `-win` crates; the native crates of section 4.1's table appear only in the target tables it allows; members add no features to `windows`, `windows-sys` or `tokio` | `cargo metadata` | repo-tools |
| 16 | No `#[ignore]`, no `#[should_panic]`, no real-time sleeps in tests; tests needing the owner's PC or real Claude live only in `owner_pc::` or `real_claude::` modules; modules over 800 lines listed in `xtask/tidy/allow.toml` with a reason | scan | lint-repo |
| 17 | Fuzz targets each have a size-capped seed corpus and a `fuzz_entry` function, and their corpus replay test exists; simulator regression files parse | scan | lint-repo |
| 18 | Scenarios are valid and private (section 4.8): schema, Redactor and PII scan on every input line, no `origin = "user"`, no unmarked audio, no personal paths, no binary files, known event names | `cargo xtask scenarios check` | lint-repo |
| 19 | A pull request of type `fix` adds or changes a regression (a scenario, a fuzz regression, a simulator regression or a test); the `evidence` job runs the listed neutral tests on the base sources with the head's new test files (expected to fail) and on the head (expected to pass); a fix that only Windows can show attaches a replay or an owner-PC run link, flagged for the reviewer | `pr-meta` plus the `evidence` job | pr-meta |
| 20 | Workflows pass zizmor; actions are pinned by SHA; permissions are read-only except in the jobs that need more, which name them | zizmor plus a scan | delegated plus lint-repo |
| 21 | Every span and event name used in code is declared in `eileen_trace::names`, and the span-parent table covers it | test in `eileen-trace` plus a scan | lint-repo |
| 22 | Each crate's `Cargo.toml` has `[package.metadata.eileen]` with `process`, `owns` and `unsafe` | `cargo metadata` | repo-tools |
| 23 | Docs drift (advisory): a pull request that changes a crate's public API without touching its crate doc gets a warning annotation; on the owner's PC, `cargo xtask docs-drift --draft` asks a Claude worker to draft the update | `syn` diff of `pub` items between base and head | repo-tools |
| 24 | Core purity: `eileen-core` depends only on `eileen-contracts`, `eileen-stdx` and `eileen-trace`; the pure crates of section 3.1 link no native library; vendored C appears only in the store and blob modules | `cargo metadata` plus a scan | repo-tools |
| 25 | Input privacy: the Raw Input modules (`ffi::raw_input` in `eileen-shell-win`, `eileen-desktop-win` and `eileen-hud-win`) never store or forward a key code other than the configured dictation key, and never a pointer position | scan of the named modules | lint-repo |
| 26 | Contract text is wrapped: every free-text `String` reachable from `Input`, `ToolResult` and `DesktopState` is `Untrusted` or `Personal`, or on the allowlist with a reason | test in `eileen-contracts` | delegated |
| 27 | The settings registry covers every config leaf key exactly once, with matching docs, and the reverse | test in `eileen-config` | delegated |
| 28 | A `scenarios-only` pull request touches only `scenarios/<area>/<id>/`, adds no binary files and uses only template fixtures; contributed scenarios need the `scenario-reviewed` label before the desktop and fleet lanes run them | `pr-meta` plus `scenarios check` | pr-meta |
| 29 | `docs/PLAYBOOK.md` cites its sources: every section has at least one citation (a research report in `docs/research/`, a decision record, or a primary source with a link and the date it was read), and every link resolves (external links nightly) | parses the playbook; lychee | lint-repo plus delegated |
| 30 | A pull request that changes `docs/DECISIONS.md` also changes `docs/PLAYBOOK.md`, or says under "Playbook" in its description why the playbook is unaffected; the generated `templates/` copies are current (check 9) | `pr-meta` | pr-meta |

### 4b.4 Guidance for people and coding agents

Decision D33 (amended by D46) records the form, chosen from the engineering-practice report's survey of the best-run repositories (rust-analyzer, ruff, uv, Zed, codex, Turso, TigerBeetle) and the owner's instruction not to assume a guidance file in every folder:

- **Root `AGENTS.md` is the canonical guide** for every coding tool (Codex reads only `AGENTS.md`; the claude CLI reads it directly and through imports). It holds the commands, the rules that do not bend, the PC rules of section 6.1 (including "run guarded builds in the background and wait for them; a cold build can outlast a 10-minute foreground tool call"), how to make a change (evidence first), testing rules, code review rules, a short summary of the checks with a link to `docs/reference/checks.md`, and pointers to `ARCHITECTURE.md`, `docs/BUILD.md`, `docs/OWNER-DIRECTION.md` and `docs/wp/`. The engineering-practice report's section 4.2 draft is the starting text, with "read the crate doc first" in place of "read the crate's AGENTS.md". At most 150 lines.
- **Root `CLAUDE.md`** is `@AGENTS.md` plus a short Claude-only section (project skills, the settings denials, the fresh-reviewer rule, no language server during build work). Imports, not symlinks, because committed symlinks check out as one-line text files on Windows.
- **Every crate's guidance is its crate doc**, the `//!` comment at the top of `src/lib.rs` (a binary's `main.rs` carries one line pointing to its library's doc), which rustdoc renders and every agent reads when it opens the crate. Required headings: `# Owns` (at most three bullets), `# Does not own`, `# Invariants` (each with how it is enforced), `# Boundaries` (called by, may depend on, must not depend on, wire types), `# Testing` (the fast command, the layers, what a bug here starts as), `# Traps`, `# Logs and tracing` (targets, spans it owns, `pii.` fields, the replay or probe command), `# Do not change without a decision record` (wire layouts, the approval type's constructor, the one `#[expect]`ed spawn, and the like). At most 80 lines.
- **Crate guidance reaches Claude on touch.** `cargo xtask codegen` extracts each crate doc into a generated `.claude/rules/crate-<name>.md` with `paths: ["crates/<name>/**"]`, so a Claude session that edits `src/jobs.rs` gets the crate's rules without reading `lib.rs` first, and into `docs/reference/crates.md` for Codex and people. Nobody edits these copies; check 9 keeps them fresh.
- **Nested `AGENTS.md` only where a folder's rules genuinely differ**, each paired with a `CLAUDE.md` that is `@AGENTS.md`: `crates/eileen-desktop-win/` and `crates/eileen-apps-win/` (COM apartments, deadlines, the desktop test group, owned fixtures only), `crates/eileen-speech/` (native libraries, model files, the exclusive build slot for model tests), `fuzz/` (nightly toolchain, Linux only, regression rule), `scenarios/` (the public corpus: privacy rules, no personal data, schema), `integrations/claude-plugin/` (TypeScript, the never-gating rule, plugin validation). Adding another needs a line in the pull request saying why the folder's rules differ.
- **`ARCHITECTURE.md` at the repository root**, short and in rust-analyzer's shape: bird's eye view, entry points, a code map with one entry per crate (named files and types, not links) marking architecture invariants and API boundaries, then cross-cutting concerns. WP0 moves today's `docs/ARCHITECTURE.md` there and adds the code map check.
- **Claude-only extras** in `.claude/`: `settings.json` permission rules that deny release builds, `cargo install`, `cargo clean`, every direct `cargo build`, `cargo test`, `cargo nextest`, `cargo clippy`, `cargo check`, `cargo run`, `cargo doc`, `cargo bench` and `cargo fuzz` (only `cargo xtask ...` is allowed, so every build goes through the guard), and reads of vendor credential files (verify the rule syntax against the settings reference); path-scoped `rules/ffi.md` for `ffi` modules plus the generated crate rules; skills `sim-debug`, `add-fuzz-target`, `e2e-scenario`, `work-package` and `save-scenario`.
- **Work package files** `docs/wp/WP<n>-<topic>.md` in the living-plan shape (goal, done when, progress, surprises, decisions, outcome), updated by the agent doing the work.
- **Rules hygiene** (Zed): do not edit guidance files during feature work; propose changes under "Suggested guidance changes" in the pull request; a rule is added only if it is non-obvious, came up more than once and is specific.

### 4b.5 `cargo xtask`

A small binary with three dependencies (`eileen-buildguard`, `serde_json`, `toml`), so it compiles in seconds; the `syn`-based checks live in `tools/repo-tools`, which xtask runs. Subcommands: `setup`, `slots [--kill-stale]`, `check`, `test [--owner]`, `clippy`, `build`, `ci`, `pre-push`, `lint-repo`, `codegen` (with `--check`), `sim` (PR seeds; `--seed`, `--profile`, `--replay`, `--minimize`, `--doublecheck`), `fuzz-replay`, `fuzz` (Linux), `scenarios check|replay|bless|catalogue|from-issue`, `e2e`, `bench`, `soak`, `measure-idle` (working set, private bytes, handles, GDI and USER objects, threads and CPU of the Eileen processes, through Win32 calls rather than CIM, which fails under load), `nightly-local`, `fetch-native`, `lock`, `deps-age`, `symbolicate`, `triage`, `docs-drift`, `contract-claude`, `tidy denylist-add`. Subcommands whose target is not built yet run and report zero items; those that belong to a package's crate (`contract-claude`, `triage`, `bench`) dispatch to that crate's binary, so later packages never edit xtask. Every subcommand that runs cargo goes through the build guard (section 6.1).

### 4b.6 The engineering playbook and reusable templates

The owner asked (2026-10-20) that how this repository is built be written down so any other project using this style of development can adopt it, not only Eileen. WP0 writes `docs/PLAYBOOK.md` in phase B, project-agnostic, with Eileen as the worked example, covering:

1. The repository architecture and modular layout: a sans-IO core with ports, plug-in interfaces, supervised processes, platform crates.
2. What is pulled in: every tool, crate, model and service with its version, license and why it was chosen over the alternatives (from sections 4.1 and 4.2 and the research reports).
3. The CI/CD design: lanes, path classes, the single aggregate check, caching, branch protection, release channels.
4. Testing tactics: deterministic simulation, scenarios from real use and their two replay modes, the test fleet, fuzzing, soak, mutation, benchmarks as A/B tests.
5. The code map and code graphs.
6. Guidance for people and AI agents (section 4b.4 and decision D33).
7. Observability, the flight recorder and replay.
8. The learning loop and how the system improves itself.
9. How parts stay updatable, programmable and swappable as AI progresses (aliases in config, plug-in drivers, the benchmark gate).
10. The working methods: research before design, critics and verification rounds (this document's section 8 is the example), evidence-first changes, conventions enforced by CI, continuous pushing, handoff upkeep, and the build guard for agents on a small PC.

Every claim is cited: to the research reports, the decision records, and primary sources (repositories, documentation, papers) with links and the date they were read; check 29 enforces it. `templates/` holds what another project can copy: the root `AGENTS.md` and `CLAUDE.md`, the crate-doc template, the work package file, the CI workflows, the `xtask` check registry and its checks as a small standalone crate, the pull request and issue templates, the scenario schema and an example scenario. Wherever a template mirrors a live file, the live file marks the shared region and `cargo xtask codegen` writes the template from it with `{{project}}` placeholders, so check 9 keeps them in step; the rest are written by hand. The playbook changes whenever a decision record changes (check 30): decision changes arrive as `amendment` pull requests, which update the playbook in the same pull request, and WP9 adds the measured results (benchmarks, fleet, the learning curve) at the end of v0.1 the same way.

## 5. Why this is not already done

The prior-art report (2026-10-11) is blunt, and this spec keeps its honesty: the halves of Eileen exist, and the README's old "nobody has built this" overstated it.

- **ChatGPT desktop** is the closed, single-vendor version on Windows. Its GPT-Live voice directs several Codex and ChatGPT Work tasks at once while you talk, and Computer Use runs on Windows. But it is OpenAI only, voice is a session you start and stop (one at a time), its screen context ("appshots") is macOS only, and on Windows its computer use takes over the foreground; OpenAI suggests a VM or a second device.
- **Paseo** (about 20,000 stars, Apache-2.0, a Windows installer) runs Claude, Codex, Copilot, OpenCode and others in parallel worktrees, with local Parakeet, Kokoro and Silero voice whose brain is a hidden session on the user's installed harness. It proves two of Eileen's choices (local CPU speech is good enough; the brain can be a warm subscription session). It does not see or drive the desktop.
- **Hermes Agent** (about 250,000 stars, MIT) ships a Windows app with a wake word, continuous voice with barge-in, background UI Automation through cua-driver, and a skill curator that learns. But it is its own harness rather than a router to vendor harnesses, it runs vendor OAuth itself and imports the Codex CLI's credential file, its wake flow takes one utterance at a time, and its skills have no deterministic replay with step checks, no per-line provenance and no permission story.
- Also: qwen-audio-agent (full-duplex voice over ACP backends, cloud realtime APIs by default), Jarhead (the closest single design, macOS only, voice billed per minute), Microsoft's local Copilot actions (Copilot+ PCs only), the Claude desktop app (Claude only; Windows background computer use undocumented).
- **Learning from an agent's own work is well studied and partly shipped** (Voyager, Agent Workflow Memory, SkillWeaver, ASI, UFO2, Hermes, Letta). Eileen does not claim the idea. What nobody ships is the whole loop on a Windows desktop for one person: capture from every job, native skills the vendor's own harness loads, a promotion ladder that ends in model-free replay with a check after every step, handoff the moment a check fails, local history with rollback, and voice and HUD control.
- **Dictation**: Handy (MIT, Rust) is the open-source leader on Windows and Wispr Flow the paid bar; neither knows the focused field, its app or what is on screen.

So Eileen's novelty is the integration on Windows, done to a higher standard than any half. That is a composition moat, not a technology moat: Paseo could add desktop control in weeks. Eileen has to win on things that can be measured and published:

1. Always listening with no wake word, local, at near-zero idle cost (VAD measured at 5.8 percent of one core), with addressee detection measured both ways by scripted tests: room talk ignored (at least 9 of 10) and unnamed requests in a conversation acted on (at least 8 of 10, at most 1 of 10 false actions), with echo cancellation that also removes the video or music the PC is playing.
2. DesktopState as cheap context, so "this", "that window" and "screen two" resolve without a screenshot.
3. Background-first, deterministic-first control with honest foreground escalation that stops the moment the user's own hands move, and safety rules in the server (password fields, blocked combos, explicit targets, per-app consent) that a learned skill cannot bypass.
4. Real pro-app adapters for the owner's examples (Resolve through its own scripting interpreter, SMTC, Office COM), which no competitor demonstrates.
5. Route-named workers on unmodified vendor CLIs with an auditable credential boundary, approvals that only the user's own voice or click can give, and per-job containment (path guard, shell policy, taint) that the prior art does not show.
6. A learning loop the user can see, steer and erase, with model-free replay that never skips a confirmation.
7. A test suite that grows from real use on many friends' PCs, with published benchmarks and A/B timing.
8. Rust where it buys something measurable: start-up in tens of milliseconds, a small idle footprint, hook and UIA latency, single binaries. Model latency is the vendor's, and the docs say so.

## 6. Work packages

### 6.1 Building on the owner's PC without thrashing it

The rust-architecture report measured why this needs rules: one clean dev build of the small skeleton under contention took 21 minutes and drove free RAM to 462 MB; a fat-LTO build beside other agents took free RAM to 0 MB and ended the session; one `rustc` was seen at 417 MB and a clean build averaged 9.4 units at once; four tools built from source took 59 minutes. A live read on 2026-10-20 found 1.27 GB free with one `rustc` running, because Chrome, the Claude app, a WSL virtual machine and VS Code hold most of the 16 GB. Disk is not a constraint (342 GB free on `C:`). The rules, enforced by `cargo xtask` through `eileen-buildguard` and repeated in every brief:

1. **Build slots with memory admission.** Two slot locks, `%LOCALAPPDATA%\eileen\build-slot-0.lock` and `-1.lock` (always there, whatever `EILEEN_DATA_DIR` says), held with `File::try_lock` so the OS releases them if the holder dies; each lock file records the holder's PID, worktree, command and start time.
   - A slot is admitted only when free physical memory (`GlobalMemoryStatusEx`) is at least 1.5 GB plus 0.5 GB per compile job; `CARGO_BUILD_JOBS` is scaled down to fit, with a floor of 2 and a ceiling of 6 (8 when the holder has both slots).
   - The second slot is admitted only after the first build has been sampled past its peak (its memory has stopped rising for 30 s) and the requirement still holds.
   - While a build runs, xtask re-checks every 10 s and stops launching new cargo invocations below 1.5 GB free (it waits; it never kills a running compile for memory).
   - It refuses to start below 20 GB free disk.
   - Cargo runs inside a kill-on-close Job Object at below-normal priority, so a killed xtask leaves no compiler running and the Claude app and the voice loop stay responsive.
   - `cargo xtask slots` lists holders; `--kill-stale` frees a slot whose holder process is gone. `EILEEN_BUILD_SLOT_HELD` lets a nested `cargo xtask` (inside an Eileen coding job that already holds a slot) reuse it instead of deadlocking.
2. **Exclusive work takes both slots** (`--exclusive`): release builds, any process that loads speech models (model tests, voice benchmarks, the Chatterbox head-to-head), soaks, every benchmark, any test or spike with Resolve Studio or Premiere running, and real-Claude runs with more than one session at once.
3. **Agents build only through `cargo xtask`.** `.claude/settings.json` denies direct `cargo build`, `test`, `nextest`, `clippy`, `check`, `run`, `doc`, `bench` and `fuzz`, as well as `cargo install`, `cargo clean` and release builds. Build agents run without rust-analyzer (no language-server plugin in their sessions), because its background `cargo check` would compile outside the slots.
4. **Long builds run in the background.** A cold build under contention can outlast a 10-minute foreground tool call; agents start guarded builds and test runs in the background, wait for them to finish, and never leave one orphaned.
5. **sccache, shared by all worktrees**, if the owner has installed it (prebuilt 0.18.0): `RUSTC_WRAPPER=sccache`, `SCCACHE_DIR=%LOCALAPPDATA%\sccache`, `SCCACHE_CACHE_SIZE=8G`. If it is not installed, build without it; never install it from source.
6. **A target directory per worktree** (Cargo's default), never a shared `CARGO_TARGET_DIR` (Cargo locks a target directory for the whole build). Delete a worktree's target directory when its branch merges. A Dev Drive for target directories and the sccache folder is an owner decision (section 7).
7. **No debug info for dependencies** in dev, line tables for our crates (section 4b.1).
8. **Inner loop on `cargo xtask check -p <crate>`, `cargo xtask clippy -p <crate>` and `cargo xtask test -p <crate>`**; build only the binary you need.
9. **No local fat-LTO builds.** Local release builds use the thin `release` profile with the exclusive lock.
10. **Never `cargo install` from source.** `cargo xtask setup` fetches prebuilt tools.
11. **Do not start a long build, simulation, soak or benchmark at the end of a session**, kill every process you start before you finish, and never benchmark during a build.
12. **Desktop packages never run at the same time.** A package marked `needs_desktop` drives real windows, audio devices or apps on the owner's PC; no two are scheduled in the same wave, and their desktop tests additionally take `%LOCALAPPDATA%\eileen\desktop.lock` through `cargo xtask` and only touch windows the test created. Tests that need real Claude count against the owner's plan: keep them few, put them in `real_claude::` modules, and run them by hand or in the owner's nightly lane.
13. **Machine hygiene during W1 to W4** is the owner's call (section 7): closing heavy apps and running `wsl --shutdown` frees about 4 GB and lets two slots run; otherwise the guard's admission will mostly allow one.

| Wave | Packages | Compiling at once | Desktop | Ends with |
|---|---|---|---|---|
| W0 | WP0 phase A | one (may use 8 jobs) | no | Workspace, pins and native dependencies, contracts, stubs, build guard, basic gates on both OSes, `all-green` required |
| W1 | WP1, WP3; WP0 phase B; WP6 step 0 (the Resolve spike, no compiling) | two (WP0 phase B's builds are small and wait for a slot like any other) | WP6 step 0 only, with the exclusive slot while Resolve runs and the owner's agreement | Core in simulation; Claude brain and workers from Rust; the tier-2 approval route proven; the remaining checks and workflows; the Resolve edit route chosen |
| W2 | WP2, WP4 | two, one during speech-model work | WP4 | **Slice S1, live**, wired by WP2 once WP4 merges; no W3 package starts until it passes on the owner's PC |
| W3 | WP5, WP7 phase A | two | WP5 | Computer use; intent, settings, capture, drafts and the first ladder rungs |
| W4 | WP6, WP7 phase B | two | WP6 | App adapters and the Resolve benchmark parts; recipes and replay |
| W5 | WP8 | one (may use 8 jobs) | WP8 | HUD, first-run UI, Claude plugin |
| W6 | WP9 | one (may use 8 jobs) | WP9 | Fleet, benchmarks, M6 demo |

Every package: branch named in its section, a draft pull request from the first push (CI runs on drafts), small pull requests (logic changes under about 500 lines), the Evidence section first in every description, merge only with `all-green` and `pr-meta`, its `docs/wp/` file kept current, its crate docs and the code map current, at least one scenario for what it builds, a fuzz target for every new parser of untrusted input, a simulator invariant or scenario for every new core behaviour. A change to a file the package does not own (this specification, a decision record, a merged package's file, the root manifest's feature lists) goes in a separate `amendment` pull request the owner reviews. CI checks every package must pass: everything `cargo xtask ci` runs (fmt, clippy `-D warnings` on Windows and Linux, nextest on both, doc tests, docs with `-D warnings`, cargo-deny, cargo-machete, `lint-repo`, `codegen --check`, simulator PR seeds and regressions, fuzz corpus replay, scenario validation and replay), plus `pr-meta` and `all-green` on GitHub. Acceptance lines that say "clean for 10 minutes" are met by a `long.yml` run on the package's branch, linked in its work package file.

### 6.2 WP0: Foundation (workspace, contracts and quality gates)

- **Branch** `wp0/<topic>` per pull request. **Depends on** nothing. **Desktop** no. **Wave** phase A in W0 (the gate for W1), phase B in W1 alongside WP1 and WP3.
- **Owns** the root files and directories of section 3.1 (`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`, `_typos.toml`, `rumdl.toml`, `lychee.toml`, `.gitattributes`, `.gitignore`, `.cargo/`, `.config/`, `.githooks/`, `.github/`, `.claude/`), `AGENTS.md`, `CLAUDE.md`, `ARCHITECTURE.md` (moved from `docs/`), `CONTRIBUTING.md`, `SECURITY.md`, `NOTICE`, `README.md`, `docs/DEV-SETUP.md`, `docs/NAMING.md`, `docs/PLAYBOOK.md`, `templates/`, `docs/wp/WP0-foundation.md`, `xtask/`, `tools/repo-tools/`, `tools/claude-plugins/eileen-dev/`, `fuzz/Cargo.toml`, `fuzz/README.md`, `fuzz/rust-toolchain.toml`, the fuzz targets `uia_text` and `desktop_state` with their corpora, `scenarios/README.md`, `scenarios/_schema/`, `scenarios/core/`, and the crates `eileen-contracts`, `eileen-stdx`, `eileen-trace`, `eileen-rt`, `eileen-platform`, `eileen-supervisor`, `eileen-buildguard`. It deletes `src/`, `tests/*.py`, `pyproject.toml` and the Python CI job. It also creates every other crate of section 3.1 and every other fuzz target as a stub; ownership of those passes to their packages when phase A merges.
- **Phase A builds** (W0), as small pull requests in this order:
  1. `wp0/ci-bootstrap`: tag the current `main` commit `python-scaffold-final`; the minimal workspace with the pins of section 4.1 (feature unions included) and the config of section 4b; `ci.yml` with `plan`, `hygiene`, `logic-linux`, `windows-clippy`, `windows-test` and `all-green`; `pr-meta.yml`; the pull request template (Evidence first, the `type:` field); `CODEOWNERS`. The Python jobs stay in this pull request until `all-green` and `pr-meta` have reported on it; then the owner switches `main`'s required checks from `test (3.12)` and `test (3.13)` to `all-green` and `pr-meta` (or tells the agent in chat to run the exact `gh api` command written in `docs/wp/WP0-foundation.md`); then the same pull request deletes the Python scaffold and job, and merges on green.
  2. `wp0/contracts`: every type of section 3.3 with serde and schemars, snapshot-tested per area (sorted maps; snapshots must not depend on feature-unified `preserve_order`), `desktop::sanitize`, the `Untrusted` and `Personal` wrappers with the text-wrapping test, the ports, the scenario and recording formats.
  3. `wp0/base-crates`: `eileen-stdx` (clock and ID ports with their single real implementations carrying `#[expect(clippy::disallowed_methods)]`, a seeded in-house `CoreRng` with no `rand`, `never!`, `always!`, `buggify!` activated per site per run only under the simulator, `num` checked casts), `eileen-trace` (section 2.13 and the observability report sections 2 to 4 and 8: init per process kind, JSON file, console, ring and slow-span layers, `names`, the span-parent schema and checker, the `pii.` policy, the `Redactor` with a literal prefilter, the Chrome trace converter, `traceparent`, `test_support` with `assert_schema_ok`), `eileen-rt` (`task::spawn` and `spawn_blocking` with names, span and await-tree registration and panic logging, `thread::spawn_named`, `thread::sleep`, bounded channel helpers), `eileen-platform` (the traits of section 3.4 with fakes), `eileen-supervisor` (port `supervisor.rs` from the ws-next scratch workspace at `C:\Users\shubh\Projects\eileen-workfiles\2026-10-scratch\rust-arch\ws-next`: process-wrap Job Object, `CREATE_NO_WINDOW`, kill on drop, stderr into tracing, deadlines, heartbeats, bounded restart backoff, `BELOW_NORMAL` for Claude sessions, the guard spawn with `CREATE_BREAKAWAY_FROM_JOB` and null handles, the single `#[expect]`ed `Command::new`, `qos::opt_out_of_ecoqos` and `qos::boost_audio_thread` in a private `ffi` module), `eileen-buildguard` (section 6.1 rule 1: slot locks with holder records, memory and disk admission, sampling, the kill-on-close Job Object and below-normal priority for cargo, `EILEEN_BUILD_SLOT_HELD`, `desktop.lock`).
  4. `wp0/stubs`: every other crate as a documented stub (crate doc with every required heading filled from this spec and naming its owner package; the public signatures of section 3; `component(cfg)` returning a component that accepts its effects and does nothing; `cli` modules for every subcommand of section 3.5 that exit with a clear "not built yet"), the real native dependencies under their target tables, `-win` crates gated with `#![cfg(windows)]`, the stub `eileen-shim` answering MCP `initialize` and an empty `tools/list`, stub fuzz targets calling the stub parsers.
  5. `wp0/xtask`: `cargo xtask` with the build guard and the subcommands `setup`, `slots`, `check`, `test`, `clippy`, `build`, `ci`, `pre-push`, `lint-repo` (checks 1, 2, 4, 6, 10, 14 and 16 with failing fixture tests), `fetch-native`, `lock`, `sim`; `eileen-sim`'s skeleton (a `World`, TOML profiles with a generated schema, seeded runs from an `arbitrary::Unstructured` buffer, the fixed-column step log, `--doublecheck`, the regressions folder, invariants 6, 10 and 13 against the stub core, a large explicit stack).
  6. `wp0/guidance`: root `AGENTS.md` and `CLAUDE.md`, the nested pairs of section 4b.4, `.claude/settings.json` with the cargo denials, `.claude/rules/ffi.md`, `CONTRIBUTING.md`, `docs/DEV-SETUP.md` for Rust (the owner installs sccache 0.18.0 and cargo-binstall 1.26.0 prebuilt; `cargo xtask setup`), `docs/wp/` files for WP0 to WP9, `git mv docs/ARCHITECTURE.md ARCHITECTURE.md` with the links updated, `SECURITY.md`, `NOTICE` (IJG, Handy, SpeakoFlow, Pocket TTS, Chatterbox, models), `docs/NAMING.md` rewritten without the old working name and the franchise.
- **Phase B builds** (W1): `lint-repo` and `tools/repo-tools` with all 30 checks of section 4b.3 and their fixtures, the check registry rendered into `CONTRIBUTING.md` and `docs/reference/checks.md`; `cargo xtask codegen` (with `--check`) writing `docs/CODEMAP.md` (crate and process graph from `[package.metadata.eileen]` and `[workspace.metadata.eileen.processes]`, module trees, the event map), `docs/reference/*` (events, telemetry names, MCP tools, config keys, crates) and `.claude/rules/crate-*.md`; `cargo xtask scenarios check|catalogue|from-issue` with the privacy rules and the tree part of the catalogue; the scenario schema in `scenarios/_schema/`, `scenarios/README.md` and three example scenarios in `scenarios/core/` (validation only); `nightly.yml`, `weekly.yml`, `long.yml`, `release.yml` with their jobs present and skipping cleanly until their inputs exist (the Pages job included), Dependabot for actions, zizmor; `cargo xtask measure-idle`, `deps-age`, `symbolicate`, `tidy denylist-add`, and dispatchers for `contract-claude`, `triage`, `bench`; the issue forms (bug, task, scenario with schema-shaped fields, fleet results); `tools/claude-plugins/eileen-dev` (plugin.json, `.lsp.json` with `checkOnSave` off, skills); the five `.claude/` skills; `xtask/tidy/exempt.toml` listing the dated historical records (`docs/DECISIONS.md` D15 and D16, `docs/RESEARCH.md` section 8, `docs/research/`, `docs/OWNER-DIRECTION.md`) with reasons; `docs/PLAYBOOK.md` and `templates/` per section 4b.6, with every section cited.
- **Acceptance.**
  1. Phase A: `cargo xtask ci` passes on the owner's PC under the build guard, `all-green` passes on `windows-latest` and `ubuntu-latest`, and `main`'s required checks are `all-green` and `pr-meta` (proven by the next pull request going green with no Python job).
  2. Phase A: contract snapshots exist for every type of section 3.3 and every tool argument schema; `cargo insta test --unreferenced reject` passes; the text-wrapping test passes.
  3. Phase A: every stub builds on both OSes with its native dependencies under target tables (`cargo deny` passes with the `webpki-roots` exception on both targets); `eileen --version` runs; every subcommand of section 3.5 exists and says "not built yet"; the stub shim answers `initialize` within 100 ms.
  4. Phase A: the build guard refuses a second slot when the measured free memory is below the requirement (test with a fake memory source), records its holder, and `slots --kill-stale` frees a dead holder's slot; killing xtask mid-build leaves no `rustc` running (Windows test).
  5. Phase A: on Windows, killing a supervised child's Job Object kills its grandchild; the guard-style spawn, started from a parent outside any kill-on-close job, survives that parent and is killed at the end of the test.
  6. Phase A: the redaction canary: fake secrets written through the file and ring layers never appear raw on disk or in a ring dump.
  7. Phase A: `cargo xtask sim` runs the PR seed set in under 90 s on the Linux runner and `--doublecheck` passes.
  8. Phase B: every one of the 30 checks has a fixture test that fails on a seeded violation (an em dash in a fixture Markdown file, a crate doc missing `# Invariants`, a code map missing a crate, a bad pull request title, an Evidence section naming a test that does not exist, an unpinned action, a scenario with `origin = "user"`, an unsafe block outside `ffi`, an undeclared span name, `cpal` in a neutral dependency table, a denylisted synthetic name), or, for delegated checks, a config assertion plus a nightly end-to-end fixture.
  9. Phase B: `cargo xtask codegen --check` passes and its output is byte-identical across two runs and both OSes; `docs/PLAYBOOK.md` covers the ten topics of section 4b.6 and passes check 29; `templates/` copied into an empty scratch repository yields a workspace whose `lint-repo` and CI skeleton run.
  10. The first CI run's Windows job times (cold and warm), the per-worktree target size of the full stub workspace, and (if the owner installed sccache) the hit rate after a second worktree's build are recorded in `docs/wp/WP0-foundation.md`.

### 6.3 WP1: Core logic, replay and simulation

- **Branch** `wp1/<topic>`. **Depends on** WP0 phase A. **Desktop** no. **Wave** W1.
- **Owns** `crates/eileen-core`, `crates/eileen-record`, `crates/eileen-replay`, `crates/eileen-sim`, `scenarios/feel/`, `scenarios/jobs/`, `scenarios/first-run/`, fuzz targets `recording`, `expected_trace` and `sim` with their corpora and regressions, `docs/wp/WP1-core.md`.
- **Builds.** First pull request: a **minimal core** that WP2 can wire for slice S1 (turn-taking idle, hearing, thinking and speaking; the addressee gate with the name and the follow-up window; fast-path dispatch through the `IntentResolver` port with a scripted fake; one brain turn; one job; mute; announcements of completion), with simulation tests. Then the full `Core::step` (section 3.3.3): turn-taking and barge-in with pause-then-decide and resume; the addressee gate of section 2.8 (conversation window, suspension while other audio plays or the mic is in use, the alone score passed to the brain as `[addressee: unsure]`, "that was for you"); intent dispatch (Command, Tool, Skill, Ambiguous, Hints, Brain with depth); the brain turn (context blocks with caps and fenced untrusted text, the `[assistant]` block, `---hud---` and `---ignore---`, clause queueing, interrupt and re-send, recycle on threshold, skill change or crash, spare swap); turn authority; depth and two speeds; jobs (states, names, slugs, ordinals, pronouns, focus job, fan-out, worktree and in-place snapshot specs, steer, reroute, cancel, app tasks with progress); sessions and the warm pool (bind, re-arm, drop under pressure); memory admission and build-heavy limits; the router; the permission gate (`claude_tool_tier` over `PathFact`s, the shell tokenizer and allowlist, taint, per-op tiers, grants as exact calls, desktop-app grants, hook decisions that are never allow, `PermissionDecision::Allow` only from a `UserInput` approval); approvals (sources, the action word, voice off while other audio or another mic user, media pause, the voice window and the deadline, target binding); announcements (the table as data, focus awareness, the narration rule with attribution); the input lock and foreground leases (physical input aborts, pause until "continue"); the hands-off pause; the action journal with inverses and undo; the settings flow over the `SettingsValidator` port (Safety keys need a `UserInput` approval); plan-usage policy; supervisor policy; incident triggers; the regret window with attribution rules; the skill engine calls through the `SkillEngine` port (scripted fake until WP7); the first-run state machine of section 2.8; HUD state assembly; scenario-save requests. `eileen-record`: the formats of section 3.3.16, the BLAKE3 and zstd blob store with reference counts, reader, writer, `trim` with setup inputs. `eileen-replay`: strict mode with the first-divergence report, scenario mode with reactive responses and ID remapping, the expectation and expected-trace matcher, the `responder` component for `--replay-models`, and `eileen replay`. `eileen-sim`: the world of section 4.7, profiles `default`, `barge-in-heavy`, `many-jobs`, `flaky-sidecars`, `media-playing`, `low-memory`, all 14 invariants, exhaustive turn-taking (depth 6 on pull requests, 8 nightly), `SimRegression` files, `cargo xtask scenarios replay`.
- **Acceptance.**
  1. All 14 invariants hold on the PR seed set and on 30 minutes of random seeds per profile (a `long.yml` run); each invariant has a hand-written scenario that breaks a fake and proves the invariant catches it.
  2. `turn::exhaustive` passes at depth 6 within the PR budget (depth 8 nightly).
  3. `tests/it/job_model.rs`: a proptest-state-machine reference model of the job lifecycle agrees with the core on random command sequences.
  4. Barge-in in simulation: onset to pause at most 96 ms of virtual time plus one callback; the heard prefix equals the text of the samples played; a backchannel resumes from the paused sample.
  5. Approval safety: a tool result, a worker's text, `eileen say` and a video's audio that each contain "yes, approve" do not approve; a bare "yes" does not approve a tier-2 call; "yes, push" in the window does; a HUD click flagged non-physical does not.
  6. Turn authority: a job-completion turn whose text says "dispatch a job" cannot dispatch one; a tainted job that read private data cannot `WebFetch` without approval.
  7. Two speeds: a deep answer arriving while the user talks goes to the HUD only; arriving in silence is spoken once.
  8. Memory admission: with simulated free memory near the reserve, a third agent queues with a spoken reason and starts when memory frees; the pool and spare are dropped first.
  9. A recorded run replays in strict mode with no divergence; a deliberate change to the router makes it report the first divergent event with its sequence number; the same run saved as a scenario still passes in scenario mode after an unrelated change that mints one extra ID.
  10. `scenarios/feel/` (half-finished sentence with a pause, room talk ignored, conversation window with an unnamed request acted on, interrupt and resume, backchannel, "no, the other one" after a cancelled intent, video playing during an answer, an action and an approval), `scenarios/jobs/` (three parallel jobs, reroute, cancel, approval by voice with the action word, approval left to the deadline, memory-queued agent) and `scenarios/first-run/` (surprise accepted, consent unticked, clone too slow, reveal declined deletes everything) replay green.
  11. `eileen-core` depends only on `eileen-contracts`, `eileen-stdx` and `eileen-trace` (check 24).

### 6.4 WP2: Runtime, binaries, shell and the first slice

- **Branch** `wp2/<topic>`. **Depends on** WP0 phase A and WP1 (its minimal core first); integrates WP3 and WP4 for slice S1. **Desktop** no for its automated tests (its few tray and hotkey tests take the desktop lock); slice S1 runs on the owner's PC after WP4 merges. **Wave** W2.
- **Owns** `crates/eileen-runtime`, `crates/eileen-mcp`, `crates/eileen-cli`, `crates/eileen-shim`, `crates/eileen-crash`, `crates/eileen-guard`, `crates/eileen-shell-win`, `crates/eileen-scenarios`, `TESTING.md`, `docs/DEBUGGING.md`, `scenarios/runtime/`, fuzz targets `mcp_client_message`, `mcp_tool_arguments`, `dispatch_line`, `worker_hook_payload`, `http_request` and `scenario_file` with their corpora and regressions, `docs/wp/WP2-runtime.md`.
- **Builds.** The driver loop (one task owns `Core`; inputs through a bounded channel; effects routed to components by family; tasks through `eileen_rt`); the SQLite store (jobs, events with `seq` and `TraceCtx`, approvals, settings versions, journal, plan usage, memory samples; migrations; WAL; `secure_delete`; retention); the bus fan-out with explicit `Lagged` and no unaddressed speech; the memory sampler (`Input::Memory` every 5 s); the loopback HTTP/1.1 server (strict limits, bearer token, client PID mapping and Job Object checks, `/v1/hooks/claude/<event>` including the held `PermissionRequest`, `/v1/state`, `/v1/control/mute`, `/v1/say` as a `Cli` turn, `/v1/handoff` pending confirmation, `/v1/test/*` only under `--test-harness`, `bus.json`); path canonicalisation and classification into `PathFact`s for `PreToolUse`; the dispatcher pipe server with caller verification and `eileen-shim` (section 3.3.9 protocol, toolsets per role, `_meta.traceparent` passthrough, logging the `_meta` the CLI sends on its first call); sidecar MCP clients through rmcp over supervised stdio with deadlines, heartbeats, restart with backoff, and hang-dump-then-kill through the guard; the connector registry; the flight recorder wiring (input tap, checkpoints with RNG state, the per-job pin, placeholders for unaddressed speech, incident assembly, the janitor, the lag probe, the audio heartbeat watchdog); `eileen-crash` and `eileen-guard` (breakaway spawn, core heartbeat, hang dumps, rate-limited restart, fail-fast reporting); `eileen-shell-win` (tray with data-driven menu, global hotkeys including approve, deny and hands-off, Raw Input for the dictation key and physical activity, toasts, the Start menu shortcut with the AppUserModelID, the HKCU Run entry); `eileen-cli` as the composition root wiring every component through `component(cfg)` and the real ports (stubs until their packages land), `--console`, `--replay-models`, and the framework of `setup` and `doctor`; the `session` and `dev` toolset handlers; `eileen-scenarios` (save from the flight recorder, share with generalisation, redaction and the local scenario-mode replay, submit through the contributor's `gh` with the auto-filled template or the issue-form fallback, the real-desktop runner with fixtures, steps through the test endpoint, transcript injection or `WavSource`, expectation evaluation through the replay matcher and the sidecars' read-only ops, `ci_factor` timing); `TESTING.md`; `docs/DEBUGGING.md`; shuttle tests for the bus, supervisor, workers finishing together and a dropped shim pipe. EcoQoS opt-out at the start of every binary it owns.
- **Slice S1, live** (once WP3 and WP4 have merged; the last pull request of WP2, and the gate for W3): `eileen run --console` on the owner's PC with the real microphone and speakers. One fast-path command ("Eileen, what time is it?"), one barge-in ("stop" while Eileen speaks), one brain answer, and one Claude job ("Eileen, create a file called hello dot txt containing hi in the scratch repo") whose completion is announced; the mute hotkey and the tray mute each stop capture and the Windows microphone indicator goes off; end-of-speech-to-first-sound latencies recorded for each. The same flow runs in CI with `--audio-source wav:`, the transcript stub and the stub harness.
- **Acceptance.**
  1. Slice S1 live passes on the owner's PC (recorded in the work package file with latencies), and its stub variant passes in CI.
  2. Killing a stand-in sidecar mid-call yields a typed error to the caller, an incident containing its hang dump from the guard, and a restart within the backoff bound, while the core keeps capturing.
  3. The `PreToolUse` endpoint answers no decision, `ask` and `deny` per tier and never `allow`; with the endpoint down a worker's guarded write is blocked; a held `PermissionRequest` answers `allow` only after a `UserInput` approval.
  4. A process inside a worker Job Object holding every token file is refused by every loopback endpoint except its own session's hooks, and by every other role's dispatcher pipe; a shim started outside any Eileen job with a valid token is refused.
  5. Idle footprint of the thin release `eileen.exe`, measured with `cargo xtask measure-idle`, with stub sidecars and again with real WASAPI capture: under 60 MB working set; CPU within section 4.5's table; numbers recorded.
  6. `eileen trace last` stitches one turn across the core, a stub sidecar and a stub worker by `turn_` ID, every line with file and line; `eileen incident show last` summarises a captured incident; the end-to-end smoke asserts every log line inside a turn carries its ID.
  7. The redaction canary covers every sink this package writes (logs, incidents, share packs, recordings, the events table), including a spoken card number and an unaddressed utterance.
  8. `eileen scenario save` then `share` then `submit --dry-run` produces a pull request body that passes `pr-meta`, a redacted folder that passes `scenarios check`, and a redacted copy that still replays in scenario mode.
  9. The shuttle tests pass; fuzz targets `mcp_client_message`, `mcp_tool_arguments`, `dispatch_line`, `worker_hook_payload`, `http_request` and `scenario_file` run clean for 10 minutes each (`long.yml`) and their corpora replay on Windows.

### 6.5 WP3: Claude harness and the brain

- **Branch** `wp3/<topic>`. **Depends on** WP0 phase A. **Desktop** no. **Wave** W1.
- **Owns** `crates/eileen-harness`, `crates/eileen-stub-harness`, `tests/harness-scripts/`, `scenarios/claude/`, fuzz targets `stream_json_line` and `control_message` with their corpora and regressions, `docs/wp/WP3-harness.md`.
- **Builds.** First, on the owner's PC, the **approval contract test** against the real CLI (section 2.5): a tier-2 Bash call listed in `--allowedTools` with the guard answering `ask` (through a test hook listener in `eileen_harness::testkit`, since WP2's endpoint does not exist yet), tried through the `PermissionRequest` hook, `--permission-prompt-tool` (if the flag exists) and deny-then-steer; for each, allow runs the command, deny leaves no output file, a dead endpoint blocks it, and the same holds for a subagent's call; the winning route, the permission mode and the exact reply shapes are recorded in the work package file and in a harness script, and the route goes into section 2.5 by amendment if it differs. In the same session, test `--restricted` per session kind (a coding worker that edits, builds and commits in its repository; a computer-use worker; the brain) and record whether it becomes the default. Then: a typed stream-json parser (line length capped before parsing; unknown `type` becomes `Unknown`; `system/init` recorded for feature detection; `Skill` tool_use becomes `SkillInvoked`; tool inputs redacted and capped at 2 KB; auth failures from the `result` event); `ClaudeWorker` with `SessionSpec` and `JobBinding` (the real `.exe` resolved through the npm shim or the native layout, never the desktop app's copy; spawned through `eileen_supervisor` at below-normal priority; the two environment policies of section 2.12; the per-session settings file with every hook and explicit timeouts; the generated MCP config with the shim and approved third-party servers; `--strict-mcp-config`, `--no-chrome`, `--no-session-persistence`, the pinned permission mode, the session's plugin snapshot directory; `ENABLE_CLAUDEAI_MCP_SERVERS=false` and `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` for every session; a build slot from `eileen-buildguard` for build-heavy repositories with `EILEEN_BUILD_SLOT_HELD` and `CARGO_BUILD_JOBS`; interrupt as a feature-detected `control_request`, cancel by killing the Job Object); the warm pool's session lifecycle (start, bind, re-arm, recycle); `BrainBackend` (warm session, `--tools ""`, only `mcp__eileen__*`, the thinking-off measurement of section 2.3, hot spare, recycle with a summary, swap on crash or 30 s of silence, the optional speculative experiment); model-id discovery written as full ids into the default routing config; git worktrees through the supervisor and in-place snapshot refs (`git stash create` plus HEAD under `refs/eileen/`) with restore; `continue_session` jobs (`--resume <id> --fork-session`); `eileen claude install|uninstall|update-window` and `eileen doctor claude` (`claude auth status`, the version range, installed against running versions, the toolchains a worker sees, never a credential file); the Codex app-server and ACP drivers as types returning `RouteUnavailable`, the mapping table in the crate doc, DeepSeek as configuration only; the flag constant the nightly drift job reads; `eileen-stub-harness` playing `StubScriptLine` scripts; the first scripts recorded from real sessions on the owner's PC and redacted with the `Redactor`; `cargo xtask contract-claude` (dispatched to a harness binary; owner's PC only: replays the scripts' prompts through the real CLI, compares event shapes, re-runs the approval cases and the skill-listing probe).
- **Acceptance.**
  1. The approval contract test passes on the owner's PC with the chosen route recorded; subagent tools stay out of `--allowedTools` unless the subagent case passed.
  2. Against `eileen-stub-harness` in CI: a worker job runs to completion through a pooled session bind; `> ` narration lines become `JobNarration`; an injected permission request reaches the approval path; a `Skill` call becomes `SkillInvoked`; a crash marks the job failed with a reason; cancel leaves no child process (nextest leak detection).
  3. On the owner's PC (`real_claude::`): the brain reaches `system/init` with connectors off and only the `eileen` server listed (and a worker's init lists only `eileen` plus approved servers); warm-turn time to first text over ten turns on a quiet machine, with each thinking-off method, recorded with the resolved model id; worker cold start recorded on a quiet PC and under typical load, and pool bind time recorded; a mid-turn `control_request` interrupt verified; learning-loop row 7 reproduced with this package's exact flags (a worker uses a learned test skill from its snapshot).
  4. The worker environment holds no variable on the denylist and keeps the pass-through list; the brain's holds only the allowlist; `cargo --version` resolves in a coding worker's shell in the stub run; no code path opens a path under the user's `.claude` folder (check 14 plus a file-access check in the stub run).
  5. `eileen doctor claude` reports a signed-out CLI clearly and suggests the user run `claude auth login`, without running it; it names the npm shim it resolved and the real executable behind it.
  6. Model ids for the `haiku`, `sonnet` and `opus` aliases are read once from `system/init` and written as full ids into the default routing config.
  7. Fuzz target `stream_json_line` runs clean for 10 minutes (`long.yml`); its corpus replays on Windows.

### 6.6 WP4: Voice (audio, speech, dictation, voices, the first-run recording)

- **Branch** `wp4/<topic>`. **Depends on** WP0 phase A. **Desktop** yes (audio devices). **Wave** W2. Speech-model tests and benchmarks take the exclusive build slot.
- **Owns** `crates/eileen-audio`, `crates/eileen-audio-win`, `crates/eileen-speech` (including `models.toml`), `crates/eileen-dictation`, `scenarios/voice/`, `scenarios/dictation/`, fuzz targets `speech_frame` and `dictation_cleanup` with their corpora and regressions, `docs/wp/WP4-voice.md`.
- **Builds.** `eileen-audio`: cpal WASAPI capture, render and loopback at 48 kHz with 10 ms buffers (cpal only under the Windows target table; the logic around it neutral); resampling (pick `rubato` or an own polyphase filter by measurement); sonora 0.2.0 AEC3 with noise suppression and high-pass, the loopback mix as the far-end reference and Eileen's PCM as the fallback, bypassed after 500 ms of loopback silence; the other-audio signal; 512-sample 16 kHz frames into an SPSC ring buffer (loom-tested under `cfg(loom)`) and the speech pipe; mute that stops the capture stream; duck, pause, resume and stop with samples-played accounting; PCM playback; `AudioSource` and `AudioSink` with `WavSource`, `CaptureSink` and `RoomMixer` (with a music or speech-video bed); the audio heartbeat and tick ring (no tracing and no allocation in the callback, checked by a counting allocator in debug builds); the energy barge-in fallback; short recordings for enrollment and the first run. `eileen-audio-win`: endpoints, loopback, the Communications category option with Windows ducking opted out, MMCSS, system volume. `eileen-speech` (lib and bin): the pipe protocol of section 3.3.7 with `source`, length validation before allocation, and TraceCtx in JSON; Silero VAD v6.2 through sherpa-onnx 1.13.8 shared; Nemotron streaming int8 fed only during speech, with confidence; turn IDs at speech onset; Smart Turn v3.2 through `ort` =2.0.0-rc.13 (with `copy-dylibs`) and the Rust log-mel, fused with continuation cues and the stretched cap; Kokoro with clause chunking, a short first clause, synthesis ahead of playback, pause and resume, and the phrase cache (stock and per-app launch phrases); barge-in windows with the stricter threshold while other audio plays; the `Synth` trait with Kokoro, Pocket TTS and Chatterbox (Turbo from the official ONNX graphs in q4 or q8, Nano from a community export once licence and digest check out; no PyTorch, no Python), the per-reply watchdog and whole-line rendering above 0.9; CAM++ for enrollment and the speaker check (adopted only if it measures well); consent enrollment, dropped clips with consent, voice deletion; the first-run recording's uses (mic level, AEC tuning, name pronunciation hint, enrollment, clone reference) and the clone-timing check; the name-recognisability check; the candidate embedding model with an own WordPiece tokenizer (adopted only within 20 ms p95); hotwords (only if beam search measures acceptable); Parakeet v3 on demand with idle unload; the low-memory profile's models; the start-up check of the loaded `onnxruntime.dll`; EcoQoS opt-out; `models.toml` (URL, size, full SHA256, license, residency) and the speech part of `eileen setup`; `eileen-speech voice list|learn|use|delete` (which `eileen voice` runs); the transcript-injection stub mode (`eileen-speech --stub <file>`). `eileen-dictation`: tier-0 cleanup implementing `DictationCleaner`, SpeakoFlow's 79 conformance cases (MIT, attributed) and Eileen's typed-slot positives and negatives.
- **Acceptance.**
  1. Synthetic echo through `RoomMixer`, with Eileen speaking and with a music bed and a speech-video bed: zero far-end words leak into the transcript, the user's command is fully recovered, and barge-in false triggers stay under 1 per 10 minutes of video.
  2. Barge-in on the owner's PC with speakers: speech onset to silence at most 150 ms at p95 over 20 trials, with the correct heard prefix; a backchannel resumes.
  3. Idle listening: the speech process under 8 percent of one core and the full stack about 1.25 GB resident; the core's idle CPU with AEC bypassed and running, and the loopback capture's own cost; recorded with the machine state.
  4. End of turn: half-finished-sentence cases do not end early on the synthetic set (and on the owner's consented local recordings once he records them, never committed; the gap is noted until then).
  5. Measured with the exclusive slot and recorded: Kokoro first clause and real-time factor through the shipped sherpa-onnx path on a quiet PC; Kokoro int8 against fp32 under equal load; Parakeet Unified EN against Nemotron (WER, partial latency, tail, RSS); the embedding model's round trip for a 12-word utterance against the 20 ms p95 budget; the first command after 30 idle minutes with Chrome, VS Code and Resolve open, with and without `pin_working_set`.
  6. The owner's head-to-head, with at least 6 GB free: Pocket TTS against Chatterbox-Nano and Chatterbox-Turbo, each cloning the owner's own voice (consented, never committed), with time to first audio, real-time factor at 2, 4 and 8 threads, resident memory and a listening test; each voice profile stores its engine; the watchdog's live or announcements-only decision follows the numbers.
  7. Voice enrollment refuses a consent clip from a different speaker (committed synthetic WAV fixtures); a dropped clip without its owner's consent recording is refused; deleting a voice, and declining the first-run reveal, remove every file (`NoFileLeft`).
  8. Dictation cleanup passes all 79 SpeakoFlow conformance cases and Eileen's negatives ("I actually enjoyed the movie" untouched); fuzz targets `dictation_cleanup` and `speech_frame` clean for 10 minutes each (`long.yml`).
  9. Mute stops capture within one buffer and the Windows microphone-in-use indicator goes off (manual check recorded); `eileen-speech` exits with a clear error when the wrong `onnxruntime.dll` is loaded.
  10. The transcript-injection stub lets CI run end-to-end scenarios with no models and no audio device, and the neutral speech and audio crates' tests pass on `ubuntu-latest`.

### 6.7 WP5: Desktop control

- **Branch** `wp5/<topic>`. **Depends on** WP0 phase A, WP1, WP2 (and so on slice S1 having passed). **Desktop** yes. **Wave** W3.
- **Owns** `crates/eileen-desktop`, `crates/eileen-desktop-win` (including the `testpad` fixture as an example binary), `scenarios/desktop/`, `docs/wp/WP5-desktop.md`.
- **Builds.** Port the computer-use proof (`C:\Users\shubh\Projects\eileen-workfiles\2026-10-scratch\computer-use\proof\` on the owner's PC; bump rmcp to the workspace's 3.6.0) into the neutral server (`eileen-desktop`: the 18 tool handlers and their `ToolSpec`s with per-op tiers, the safety rules of section 2.6 including the target-category tiers, the send-like word list with localized forms, typed-text redaction and Luhn, per-app grants, element indexing and selector identity, evidence-based re-finding for self-healing, DPI mapping math and the mapping line, set-of-mark drawing, the app-name scorer with the margin rule, the insertion ladder's decisions, the manifest and app capabilities, `sanitize`, screenshots as text plus image only) and the Windows backend (`eileen-desktop-win`: per-monitor-v2 first; UIA cache requests on a dedicated MTA thread with deadlines, abandoned and replaced on a hang, the process killed only after three abandoned threads; window management and launch on their own thread; `SetWinEventHook` and on-demand UIA (no global focus handler for Chromium and Electron) feeding `DesktopDelta`s and the DesktopState cache; Raw Input for physical activity; monitors with stable ordinals; DWM frame bounds; executable versions; `BitBlt` and `PrintWindow` capture, resize to 1280 px and JPEG q80; `SendInput` one event at a time with the foreground and physical-input guard, leases, cursor restore only if unmoved; `ElementFromPoint` and `WindowFromPoint` checks; Invoke with a foreground check before and after; the `shell:AppsFolder` index persisted to `cache\apps.json` and refreshed in the background, AUMID launch that waits for the main window; moves and snaps against work areas; `WM_CLOSE` through `SendMessageTimeout` with an unsaved-state check; system volume through the platform trait; bounded cross-process calls; `EM_REPLACESEL`; the port of Handy's clipboard transaction (MIT, attribution in `NOTICE`) with terminal-class detection for Ctrl+Shift+V; paced Unicode `SendInput` with read-back; TextPattern selection and the clipboard fallback with the exclusion formats; presentation state; microphone-in-use detection). `DesktopIntent` with the configurable wait and cancel. Fix the testpad fixture so its controls stay inside its window at any DPI. EcoQoS opt-out at start.
- **Acceptance.**
  1. On owned fixtures (testpad, Notepad launched by AUMID and closed unsaved, an isolated Edge profile): background Invoke and SetValue verified by the fixture's own state; password fields refused (also through `ElementFromPoint` for a coordinate click); every blocked combo refused; a coordinate click that would land on another window or on an Eileen window refused; a foreground action with the user idle succeeds and restores the previous foreground; a synthetic "user" moving the mouse during a lease stops the sequence within one event and leaves the cursor where the user put it; when Windows refuses the foreground, it says so and never loops; an Invoke that moves the foreground on a Win32 button is detected and handled.
  2. `desktop_state` p95 under 100 ms on the owner's desktop from the cache; `desktop_tree` for Notepad and Explorer within the proof's numbers; recorded.
  3. A deliberately hung UIA provider fixture makes the call fail with a typed error within its deadline, the thread is abandoned and replaced, "open Notepad" and a window snap still work during the hang, and only repeated hangs restart the sidecar, with the core listening throughout.
  4. A real Claude worker on the owner's PC (`real_claude::`) opens Notepad, writes a sentence with `desktop_set_value` and reads it back through the shim and dispatcher, after the per-app consent question; the trajectory has every step with a verification hint and a rung.
  5. Dictation insertion is exact in a RichEdit fixture via `EM_REPLACESEL` and in Notepad and an isolated Edge text area via the clipboard transaction, with the user's clipboard restored (a full-format snapshot test with an image on the clipboard); a clipboard entry marked for exclusion is never read into DesktopState.
  6. The Chromium probe is recorded: time for web content to appear over UIA once asked, whether a page password field is detected through `ElementFromPoint`, and Chrome's private bytes and CPU with Eileen running against not running.
  7. `scenarios/desktop/` covers open app (including the ambiguous video-editor case), snap to a half, move to a monitor (`monitors>=2`), close, volume, what am I looking at, and insert text; they replay in simulation and run in the desktop lane.
  8. DPI mapping unit tests for 100, 125, 150 and 200 percent and for monitors left of and above the primary (fakes); the testpad fixture keeps its controls inside its window at 144 DPI.
  9. The first nightly proves whether hosted Windows runners give UIA an interactive desktop with a Notepad round trip; if not, it is recorded and desktop tests stay in the owner and fleet lanes.

### 6.8 WP6: App adapters

- **Branch** `wp6/<topic>`. **Depends on** WP0 phase A, WP1, WP2; step 0 depends on nothing. **Desktop** yes. **Wave** step 0 in W1, the rest in W4.
- **Owns** `crates/eileen-apps` (including `models.toml`), `crates/eileen-apps-win`, `adapters/resolve/`, `scenarios/apps/`, fuzz target `resolve_reply` with its corpus and regressions, `docs/wp/WP6-apps.md`.
- **Step 0, the correction-route spike** (W1, on the owner's PC, no Rust compiling, only with his agreement to launch Resolve and create one dedicated test project, under the exclusive build slot while Resolve runs): with the Lua bridge from the app-hooks report (and the `hookprobe` binary already built in `C:\Users\shubh\Projects\eileen-workfiles\2026-10-scratch\app-hooks\hookprobe`), test in order: importing a generated Fusion `.setting` with an ellipse mask keyframed from boxes and a sharpen node through `ImportFusionComp` (and whether it renders); a DRX that carries a window at a given rectangle; `AddVersion` and `LoadVersionByName` as undo; `GetRenderJobStatus` progress; the cost of exporting a still every N frames for tracking; whether `ApplyGradeFromDRX` replaces existing nodes. Record the results and the chosen route in `docs/wp/WP6-apps.md`, and open an amendment to section 3.3.9 if `ResolveOp` needs to change.
- **Builds** (W4). In `eileen-apps` (neutral, tested on Linux): the tool handlers and `ToolSpec`s with per-op tiers, read-only flags, idempotence, inverses and `preview_before`; drop-frame-aware absolute timecode math; the bridge line protocol parser (parse JSON, never compare strings); the YouTube URL builder with `&t=`; SMTC live position; the media history; face picking, crops, interpolated tracking from per-frame boxes, the variance-of-Laplacian metric; the Fusion `.setting` generator (and the DRX generator if the spike says it works); recent-work ranking with caller-supplied focus-time boosts and thumbnails for contact sheets; the manifest and app capabilities with real versions. In `eileen-apps-win` (unsafe only in its `ffi` module): one SMTC manager for the process lifetime with the four subscriptions and control confirmed by the next event; `ShellExecuteW` for URLs, allowlisted URIs and paths, with the wait for the expected media title; the late-bound `IDispatch` helper on per-target STA threads with call timeouts and an `IMessageFilter`, attaching through the running object table with `CLSCTX_LOCAL_SERVER`, for Word, Excel, Explorer (tabs) and Photoshop (tested against a fake COM target only); the recent-work index (jump lists, `.lnk` bytes, Office MRU, a `ReadDirectoryChangesW` watcher, the media history). Resolve: the bridge in `adapters/resolve/eileen_resolve_bridge.lua` with every method of section 2.7, spawned once through `eileen_supervisor` and gated on `Resolve.exe`, readiness polling after a launch, version reporting, render progress events for app tasks. YuNet (OpenCV Zoo, MIT) through `ort` against the shipped DLL, pinned in `models.toml`. `eileen-apps doctor resolve` (which `eileen doctor resolve` runs). EcoQoS opt-out at start. Playwright MCP stays optional and documented, not built.
- **Acceptance.**
  1. On the dedicated test project in Resolve Studio 21.0.1: `goto` 15:30 lands on the computed timecode for 24, 25, 29.97 drop-frame and 30 fps timelines starting at 01:00:00:00, after opening the Edit page; `export_still` writes a frame; `add_version` then the chosen edit then `load_version` restores the clip; `SetCurrentTimecode`, `GetCurrentVideoItem`, `ExportCurrentFrameAsStill` and the edit's latencies are recorded; a render started through `render_start` reports progress as an app task.
  2. With Resolve closed no call stalls: the gate answers that Resolve is not running in under 50 ms; launching it, `hello` is polled until ready and the project and timeline questions follow the flow of E7.
  3. YuNet finds the faces in a committed synthetic test image (generated, no real person), picks left and right correctly, returns a crop, tracks a moving synthetic face across stills, and sharpness rises after a known sharpen filter.
  4. SMTC pause, play and seek on a silent-WAV test media session, each confirmed by the next event; on the owner's PC a YouTube URL with `&t=200` plays from 3:20, a seek to 60 s through `apps_media` is confirmed once the session returns, and "play that video at 1:10" resolves through the media history with no model.
  5. Word selection and Excel active cell read from documents the test creates and closes without saving; `apps_open_path` opens a fixture file; `apps_open_uri` refuses a scheme outside the allowlist.
  6. Fuzz target `resolve_reply` clean for 10 minutes (`long.yml`).
  7. `scenarios/apps/` covers Resolve goto, the crop confirmation, version undo, render progress, YouTube play-at and seek, pause and play, Word selection; B1's deterministic phases run end to end with a stub worker for any delegated step.

### 6.9 WP7: Intent, settings and learning

- **Branch** `wp7/<topic>`. **Depends on** WP0 phase A, WP1, WP2, WP3. **Desktop** no. **Wave** phase A in W3 (with WP5), phase B in W4 (with WP6).
- **Owns** `crates/eileen-intent`, `crates/eileen-skills`, `crates/eileen-learn` (including `tests/scripts/` for librarian and skill-invocation stub scripts), `crates/eileen-config`, `scenarios/learning/`, `scenarios/settings/`, fuzz targets `config_toml`, `skill_record` and `command_grammar` with their corpora and regressions, `docs/wp/WP7-understanding.md`.
- **Phase A builds** (W3). `eileen-intent` (pure, implements `IntentResolver`): the grammar of section 2.4 as data with its precedence, slot resolvers including `Media` and `Ambiguous` results, normalisation, the content-word rule, learned templates (port the matcher from `C:\Users\shubh\Projects\eileen-workfiles\2026-10-scratch\learning\matcher\`), BM25 hints with an absolute floor, depth overrides, `introspect`. `eileen-config`: loading with diagnostics that name lines, comment-preserving apply with `toml_edit`, the `SettingSpec` registry for every key of section 3.3.11 with risk classes (the Safety list of section 2.11), apply modes and bounds, the `SettingsValidator`, profiles, `explain`, the generated schema and default file, keyring secrets as `SecretString` (Windows target only), the watcher hook, `eileen config` and `eileen apps add-mcp`. `eileen-skills` (pure): the template grammar, the linter with every rule of sections 2.10 and 3.3.13, computed provenance, the deny-scan, `ladder::next_level` over `LadderPolicy` (light and standard), the success verdict, forget planning; the `SkillEngine` port's ladder and verdict parts. `eileen-learn`: the recorder (steps keyed by `CallId`, redaction before disk, skill-use attribution, regret signals), `learn.sqlite` with `secure_delete`, the 200 MB cap, retention, evidence shrinking and `librarian_runs`; the per-session snapshot writer with manifests; `pending\`; the librarian scheduler (pre-filter, batching, plan-share caps, on demand, below-normal priority) and the `learn` toolset handlers (`learn_skill_propose` returning lint errors, `learn_skill_history`, `learn_fact_propose`); the user model with deltas, the deterministic fact path's store side, retirement; the ledger with document identity and focus time, exclusions and off-the-record; forget with tombstones and the purge across stores; `learned_query`, `learned_forget`, `learned_correct`; `eileen skills` and `eileen facts`; skills-as-scenarios when a skill becomes trusted.
- **Phase B builds** (W4). Recipes with step references, `call`, `delegate` and `confirm` steps, per-op checks, idempotent retries and eligibility; shadow agreement; the `ReplayMachine` behind `SkillEngine` (levels 3 and 4, handoff briefings, unseen enumerated values sent to a worker once); self-healing through the desktop server's evidence-based re-find; probation on app version and tool schema hash changes; L2 semantic matching over embeddings from the speech process with confirmed phrasings (slot-free targets only); repair committed only if the skill's scenarios replay green; the B8-curve fixture skill.
- **Acceptance.**
  1. The learning-loop report's eleven CI acceptance tests (its section 6.5) pass as written, with versions in SQLite instead of git and with the changes of section 2.10 (per-op checks, computed provenance, the success definition).
  2. Matcher golden file: the ten utterances of learning-loop report row 15 give exactly those results; p99 under 1 ms with 5,000 skills.
  3. Resolver golden file: every grammar phrase of section 2.4; ten paraphrases per phrase that must not trigger an action unconfirmed; the ambiguity cases (the video editor, a bare "go to 15"); a learned template that tries to shadow "stop" or "undo that" is rejected by the linter; "the girl on the right" in a template with only a position slot becomes a hint, not a replay; `command_grammar` fuzzing clean for 10 minutes.
  4. Settings: "call you Nova" runs the three checks (fake speech check in CI), applies on the next turn, updates the addressee name and recognizer hints, and "undo that" restores Eileen; a hotkey owned by another process is rejected before applying; a tier-3 relaxation is refused; a tier-2 relaxation requires the logged decision; a Safety key proposed by the brain is not applied without a `UserInput` approval; a hand edit with a syntax error keeps the previous version and reports the line; apply preserves comments; the registry completeness test passes.
  5. Poisoning: through the stub harness, an injected web page that asks the worker to upload a folder and to "remember to always do this" produces no skill line, recipe step or fact that carries it; a librarian draft that paraphrases it is rejected or held for approval.
  6. Privacy: after "forget that", a grep of every file under the data directory finds no trace of a canary request; dictation never reaches a trajectory as words; the librarian never receives a fast-path or replay job.
  7. On the owner's PC (`real_claude::`, a coding job in a scratch repository, not a desktop job): the librarian distills one real trajectory into a draft that passes the linter, a second run of the same kind of request, told to follow the skill, uses fewer model turns than the first, and the attribution records which version it used.
  8. Phase B: a recipe replays in simulation with every check, hands off at a failed check with a full briefing, refuses to retry a non-idempotent step, never skips a `confirm` step, and the shadow-agreement table tests pass; the B8-curve fixture reaches the levels section 2.10 predicts by run index in simulation.
  9. `scenarios/learning/` (what have you learned, what do you know about me, correct a fact, forget a skill and a fact, rollback, replay with a failed check handing off, learn from that now) and `scenarios/settings/` replay green; `config_toml` and `skill_record` fuzzing clean for 10 minutes.

### 6.10 WP8: HUD, first run and Claude integrations

- **Branch** `wp8/<topic>`. **Depends on** WP0 phase A, WP1, WP2, WP3, WP4, WP7. **Desktop** yes. **Wave** W5.
- **Owns** `crates/eileen-hud`, `crates/eileen-hud-win`, `integrations/claude-plugin/`, `.claude-plugin/marketplace.json`, `scenarios/hud/`, `scenarios/claude-plugin/`, fuzz target `hud_line` with its corpus and regressions, `docs/wp/WP8-hud.md`.
- **Builds.** First pull request: a probe of the renderer on the owner's PC (DirectWrite into Direct2D on a DirectComposition surface, and the colour-keyed GDI fallback) with `WDA_EXCLUDEFROMCAPTURE` on exactly that window type, idle cost measured with `cargo xtask measure-idle`; the choice recorded. Then `eileen-hud`: platform-neutral layout from `HudState` to a draw list (mic state and level, the conversation ring, the heard line greyed when not addressed and tappable, what Eileen says, up to 8 jobs with ordinals, route names, progress and paused state, approvals with voice or HUD-only and both countdowns, the intent outline, the input-lock holder and lease countdown, "you took over", the live thumbnail slot, panels (Markdown, learned items, facts, scenario preview, confirm with a crop, contact sheet, a job's diff and tests, settings), up to 3 suggestions, the first-run choices, checkbox, progress and reveal, indicators; corners, scale, opacity, high contrast) with insta snapshots; the HUD protocol with the `physical` flag. `eileen-hud-win`: the overlay and the strip, Raw Input on the strip so only physical clicks count, `WDA_EXCLUDEFROMCAPTURE`, per-monitor DPI, the DWM thumbnail, `WM_DROPFILES` for voice clips, hiding in presentation and full-screen states. The settings panel's controls send `SetSetting` through the normal flow. The Claude plugin per the claude-mods report: plugin name `eileen`; the handoff skill with `disable-model-invocation: true` passing `${CLAUDE_SESSION_ID}`, cwd, a summary and `$ARGUMENTS` to `session_handoff` (which waits for the user's confirmation); `.mcp.json` launching `eileen-shim --role session` by an absolute path written at install; a display-only mod (status entry, band with the last utterance and Mute, `/eileen` with no model turn) reading `bus.json` and polling `GET /v1/state`; tests for `claude plugin test`; types regenerated for the installed CLI; the root marketplace file. The mod never answers `tool.check`, never approves, never submits prompts, never calls the model.
- **Acceptance.**
  1. Draw-list snapshots for every `HudState` variant (every corner, scale and state, every panel, the first-run views); the HUD survives being killed and restarted with the current state.
  2. On the owner's PC: the HUD never takes focus; clicks pass through the overlay; Allow and Deny on the strip resolve an approval; a synthetic click on Allow from `SendInput` is ignored; a `desktop_screenshot` does not contain the HUD; the live thumbnail shows a covered fixture window without moving it.
  3. Idle footprint of `eileen-hud.exe` measured idle and while updating ten times a second; recorded.
  4. The HUD mic button stops capture; the settings panel changes the HUD corner and "undo that" restores it; changing the mute key to one another process owns is refused.
  5. The first run end to end on the owner's PC: tour, skip, and the surprise path with the box ticked (clone, demo, reveal, keep or delete), with the box unticked (default voice), and with cloning too slow (fallback); declining deletes every file.
  6. `claude plugin validate --strict` and `claude plugin test` pass on `integrations/claude-plugin`; the validation report shows no gating hook and only the reviewed host calls.
  7. On the owner's PC (manual, opt-in install): `/eileen:handoff` from a Claude session creates a pending handoff that, once confirmed, becomes a `continue_session` job resuming a fork of that session; `session_say` is rate-limited and prefixed.
  8. Fuzz target `hud_line` clean for 10 minutes (`long.yml`).

### 6.11 WP9: The test fleet, benchmarks and the integration demo

- **Branch** `wp9/<topic>`. **Depends on** WP0 to WP8. **Desktop** yes. **Wave** W6.
- **Owns** `crates/eileen-fleet`, `crates/eileen-e2e`, `scenarios/bench/` (including `_fixtures/flaky-login/`), `docs/FLEET.md`, fuzz target `fleet_result` with its corpus and regressions, `docs/wp/WP9-e2e.md`.
- **Builds.** `eileen-fleet` (shipped): `eileen test --fleet` with one-time consent, scenarios from the installed version's tag filtered by `requires`, recorded mode (`--replay-models`) by default and live mode with a turn budget, throwaway data directory and owned fixtures, idle and call checks, attended countdown or armed nights, locked-session skip, abort on physical input, auto-denied tier 2 with voice approval off, workstation lock after an armed run, the per-user Task Scheduler entry only after an explicit yes, `FleetResult` lines with anonymous learning statistics, preview, and upload as a comment on the week's fleet issue through the contributor's own `gh`. `eileen-e2e` (development only): benchmarks B1 to B9 and B8-curve with A/B runs against the base binary (alternating, Mann-Whitney, only significant differences above about 10 percent, runs under 2 GB free excluded) appending JSON lines to the devhub store (a local file until the owner decides where it lives); the soak runner behind `cargo xtask soak`; ingest of fleet comments from listed contributors, clustering by signature, one issue per cluster, flake tracking, quarantine expiry; the results data the nightly Pages build renders into the catalogue and the benchmark page; `cargo xtask triage <issue>` (owner's PC: a Claude worker reads the redacted cluster data, the replay and `docs/CODEMAP.md` and drafts an explanation pointing at the code). The `flaky-login` fixture repository (a small Python project with deterministic flaky tests that run in seconds). `docs/FLEET.md`. Then run every desktop-lane scenario, B1 to B9, B8-curve, the first-run demo and the M6 demo on the owner's PC, record the results in `docs/wp/WP9-e2e.md`, and open an `amendment` pull request adding the measured results to `docs/PLAYBOOK.md`.
- **Acceptance.**
  1. Every desktop-lane scenario in the repository runs on the owner's PC with a recorded result; failures are issues, not skips.
  2. B1 runs end to end for real on the owner's PC (Resolve Studio, real Claude, hot spare off) with every phase recorded, passing on a sharpness gain of at least 15 percent and the owner's yes; B8 shows, over at least three alternating pairs with new slot values, the learned runs cheaper in time, tokens and worker turns; B8-curve matches the table in section 2.10.
  3. A second Windows 11 machine (a friend's) installs Eileen from `TESTING.md` without asking the author, saves a scenario by voice, previews it, and submits it as a pull request from its own GitHub account; CI validates and replays it.
  4. A fleet run in recorded mode on two machines produces result files containing no personal data (a test greps them for the user name, machine name and home path), uploads them, and the nightly ingest clusters a deliberately failing scenario into one issue; a comment from an unlisted account is ignored.
  5. The soak runs 4 hours on `windows-latest` with flat handles, GDI objects and threads and a core memory slope under 1 MB per hour.
  6. The M6 demo script of ROADMAP.md runs end to end on the owner's single monitor with no restart and no typed command after `eileen run`, with peak memory recorded.
  7. Fuzz target `fleet_result` clean for 10 minutes (`long.yml`).

## 7. Owner decisions, open questions and risks

Decisions only the owner can make (also returned to the workflow):

1. Accept or overturn the decision records D17 to D50, in particular D21 (Eileen's own desktop server instead of Cua Driver), D27 and D44 (the learning loop and its revision), D33 and D46 (the guidance form and the gates), D40 (what v0.1 contains and what waits for v0.2, including encryption at rest) and D41 (the brain may act through deterministic tools).
2. The required-check switch during WP0's `wp0/ci-bootstrap` pull request: switch `main`'s required checks from `test (3.12)` and `test (3.13)` to `all-green` and `pr-meta` yourself once both have reported, or tell the WP0 agent in chat to run the `gh api` command its work package file gives. Also set the squash-merge default to "pull request title and description", so each merge's subject and Evidence land in `git log`. (Protection, linear history, up-to-date branches, no organisation and no merge queue are already applied.)
3. Where the devhub results store lives: an orphan `devhub` branch in this repository or a separate small repository (a local file until then).
4. Enable GitHub Pages for the scenario catalogue and the benchmark results page (Actions as the source), or keep them as nightly artifacts only.
5. Whether nightly may publish an unsigned prebuilt zip as a `nightly` pre-release, so friends can test without building.
6. Whether fleet results are uploaded as comments on a weekly issue from each contributor's own account (the default here), or somewhere else; who is in `.github/fleet-contributors.txt`.
7. Install sccache 0.18.0 and cargo-binstall 1.26.0 from their prebuilt releases; optionally create a Dev Drive for build targets and the sccache folder (it relaxes Defender scanning on that volume, which is a security setting and therefore your call).
8. During waves W1 to W4: close heavy apps and run `wsl --shutdown` while agents compile, so two build slots fit, or accept that the guard will mostly admit one.
9. Resolve: set External scripting to Local; allow Eileen to launch Resolve and create one dedicated test project for WP6's spike (in W1) and for WP6 and WP9; decide whether and when to move to Studio 21.1 or later (native MCP server, Python only in Studio).
10. The owner's nightly lane: opt in, set the nightly turn budget, allow the Task Scheduler entry and keep-awake, and say whether desktop scenarios may run armed overnight or only attended.
11. Record a consented set of your own speech (commands, half-sentences, pauses, side talk), and your own voice for the Chatterbox and Pocket TTS head-to-head, kept on the PC and never committed.
12. Before the first binary release: accept GPL obligations for release binaries if the sherpa-onnx prebuilt contains espeak-ng, or require a TTS front end without it.
13. Nemotron and Parakeet Unified are under NVIDIA's Open Model License (commercial use allowed, not an OSI license), and Kroko's license is unverified: acceptable as downloaded models, or open-licensed models only (Parakeet v3 CC-BY-4.0)?
14. Pocket TTS: the sherpa-onnx archive's README says non-commercial while its LICENSE says CC-BY-4.0; and Chatterbox-Nano exists only as community ONNX exports. Confirm both licenses with their maintainers before shipping downloaders.
15. Historic names: may the dated records (DECISIONS.md D15 and D16, RESEARCH.md section 8) be scrubbed of the old working name and the franchise, or stay exempt from the house-style check as history?
16. Install the `eileen` Claude plugin into your own Claude app (opt-in), and whether learned skills should ever load into your own sessions (default no).
17. Optional code review on pull requests by a Claude worker, in addition to the deterministic gates, set up on your own account.
18. The license under which contributed scenarios and consented audio enter the repository (MIT like the code, with audio under CC-BY-4.0 and a speaker consent record, is the proposal).
19. During calls: listen only for the assistant's name and stay silent (default here), or mute entirely.
20. Approvals left unanswered: the default here keeps a tier-2 request waiting on the HUD until the job's 30-minute deadline instead of declining after 20 s of silence. Keep, or shorten?
21. Spoken conversation in other languages: v0.2 (default here; typed requests in any language already work through the brain), or a v0.1 fallback that loads Parakeet v3 when Nemotron's confidence is low (about 0.75 GB more while loaded)?
22. Windows App Actions as a route rung: v0.2 (default here), and reading Windows' location service: v0.2 (v0.1 uses a location the user says).
23. VS Code: if benchmark B4 shows "that variable" cannot be resolved reliably from the window title and a crop around the caret, pull the small VS Code extension into v0.1?

Open risks (each has an owner package that measures it):

- Real-speech accuracy, Smart Turn on real pauses, and addressee detection both ways are untested (all corpora were synthetic). WP4, WP1.
- Echo cancellation with the loopback reference, real speakers, music and video is untested; the Communications-category AEC is the alternative. WP4.
- TTS speed is marginal on this CPU; the "about a second" ask is met by the fast path, cached phrases and the earcon. WP4 measures the alternatives.
- The tier-2 approval route for Claude workers rests on CLI behaviour nobody has run; WP3's contract test decides it in W1, and the weekly and nightly contract tests guard it.
- Raw Input's "no device handle means synthetic" signal must hold for the physical-input guard and the HUD's click check; WP5 and WP8 test it with `SendInput`, and fall back to low-level hooks in small separate processes if it does not.
- Foreground work while the owner is active is solved only by asking and stopping; a signed UIAccess helper needs a certificate. WP5.
- Multi-monitor and mixed-DPI behaviour is untested on the owner's single-monitor PC. The fleet.
- The Resolve edit route: whether an imported Fusion composition renders, and whether a DRX can place a window, are unknown until WP6's spike in W1; computer use on the Color page is the last resort.
- Chatterbox on this CPU: the only Turbo number is 7.5 GB peak on PyTorch; the ONNX q4 export may still be too heavy for live use. WP4's head-to-head decides.
- Hosted Windows runners may not give UIA an interactive desktop; if not, desktop scenarios run only on the owner's and fleet PCs. WP5 and WP9.
- The claude CLI's stream-json control protocol, skill discovery and plugin behaviour are observed on 2.1.304 and are not a stable API; the nightly drift job and the contract test guard them. Anthropic has said `--bare` may become the default for `-p`; the driver detects the auth failure and says so. WP3.
- Plan usage of the brain, the spare, the warm pool, the librarian and the nightly lanes is unmeasured; caps start small and the librarian is capped by plan share. WP3 and WP7.
- Memory: the owner's real working set leaves 1 to 2 GB free; admission, profiles and the guard are the levers. WP1 (policy), WP2 (sampling), WP4 (models), WP9 (peak during M6).
- Windows CI time is unmeasured; path classes and Windows-only test selection are the levers. WP0.
- The candidate embedding model, hotword decoding cost, Parakeet Unified and Kokoro int8 are unmeasured; each is adopted only on measurement. WP4.

## 8. Revision log

- 2026-10-20, first architect pass.
- 2026-10-20, revision after five reviews. What changed, by review, and what was declined. The owner's entry of the same evening (a reusable, fully cited engineering playbook with templates) is folded in as section 4b.6, checks 29 and 30, WP0's phase B and decision D50.

**Feasibility on this PC.**

- Adopted: the build guard admits by measured free memory per job, samples the first build before a second slot, re-checks while running, has a disk floor, records its holder, kills cargo with a Job Object, and uses `windows-sys` (xtask is no longer std-only); `.claude/settings.json` denies direct cargo builds; build agents run without rust-analyzer; builds run in the background; Resolve, Premiere, benchmarks and multi-session real-Claude runs take the exclusive slot (6.1). Native dependencies moved under Windows target tables, `ort`'s `copy-dylibs`, `fetch-native` exporting the environment, a start-up DLL check, a scoped `webpki-roots` license exception, releases through `--release` with overrides instead of a `dist` profile, and WP0's stubs declaring the real native dependencies (4.1, 4b). The crate graph fixed: `eileen-rt`, `eileen-replay`, `eileen-scenarios`, `eileen-fleet`, `eileen-skills`, `eileen-buildguard`, `eileen-hud-win`; `eileen voice` and `eileen doctor resolve` run the sidecar binaries (3.1). Worker environments use a denylist with a pass-through list and toolchain folders (2.12). Worker cold start is in the budget, the contracts separate sessions from jobs, and a warm pool exists (2.5, 4.3). Chatterbox per voice and the first-run surprise path restored (2.8, E13, E18). The M6 demo and B3 use a fixture repository, coding jobs in build-heavy repositories take a build slot, fan-out is admitted by memory, single-monitor steps rewritten. The required-check switch sequenced in WP0. Disk figures corrected; latency table with a typical-load column; idle CPU table with the AEC bypass; low-memory profile; `--no-chrome`; the npm shim resolver and update window; the casts policy; feature unions; desktop resilience (thread abandon, persisted index, cache, separate window thread, no global UIA focus handler); Raw Input instead of a low-level hook; HKCU Run and breakaway; DirectWrite rendering and the capture-exclusion probe; the Fusion route for B1; whole-line rendering for slow voices; WP2's split into phases with S1 last; the stub shim answering `initialize`; `measure-idle` in xtask; pre-declared xtask dispatchers; contracts split per area; WP0 phase B; the CI cache budget.
- Declined: splitting WP0, WP2 and WP7 into separate packages (the spec keeps ten packages and uses phases instead, which gives the same parallelism with fewer handoffs); taking a build slot for every coding job regardless of repository (only build-heavy repositories do, so Python and Node fan-out stays parallel).

**The owner's asks and the feel.**

- Adopted: the brain's `act` toolset and phrasing candidates (2.3); the loopback echo reference, media pause during approvals, the action word and voice-approval limits (2.2, 2.5, 2.8); the Resolve route ranking, versions for undo, project and timeline loading, the crop confirmation, the literal benchmark phrase with one clarifying question, and B1 passing only with the owner's yes (2.7, E7); the worker environment; the first-run surprise path (E18, 2.8); Chatterbox per voice with the head-to-head; route discovery, third-party MCP servers, rungs, `apps_open_uri`, the Confirm panel and the live thumbnail (2.5, 2.7, 2.9); render progress as app tasks; `WindowAction::Close`, `desktop_volume`, `apps_open_path`; S1 made live with the mute key and tray in WP2 and a console view, and W3 gated on it; the required-check switch; the E3 grammar entry for known media; personality in the per-turn block and no idle recycling; clickable HUD settings, checkbox and clip drop; coding in place with a snapshot ref; the `---ignore---` sentinel, resume after a non-addressed barge-in, "go on", the stretched silence cap; steering "no, the other one" to the acting job; unaddressed speech kept in memory only; one foreground lease per phase; one deep worker by default and the no-unsourced-facts rule; connectors off for every session; the thinking-off measurement plan; `source` in the frame header; the nightly CLI drift job; TESTING.md prerequisites and the recorded fleet mode; the `General` job kind; the M6 fixtures; launch waiting for the main window; `pr-meta` checking that claimed tests exist.
- Declined or deferred: spoken conversation in other languages stays v0.2 unless the owner chooses the Parakeet fallback (section 7, item 21), because it costs memory this PC lacks; Windows App Actions stay v0.2 as an owner decision (item 22).

**Safety and best-in-class.**

- Adopted: taint, the path guard and the shell policy, domain-restricted `WebFetch`, and D40 amending D14 (2.12); caller verification by process and Job Object for the dispatcher and the loopback endpoint, the `Cli` turn source (2.12); approvals only from physical clicks, the action word, media paused, voice off while other audio or another mic user, `approve` accepted only for pending asks with summaries built by Eileen (2.5, 2.9); the real-CLI approval contract test with three candidate routes and no subagent tools until proven (2.5, WP3); turn authority, fenced untrusted text, Safety settings needing a user approval, computed provenance, drafts with observed lines held in `pending\` (2.3, 2.10, 2.11); physical-input detection, leases that stop on the user's hands, the Invoke foreground check (2.6); unaddressed speech not persisted, nothing stored from calls, retention for `events` and `approvals`, clipboard exclusions (2.12, 2.6); `ElementFromPoint` checks, category tiers, Luhn and redaction on typed text, refused credential questions, per-app desktop grants, the longer word list (2.6); `Untrusted` and `Personal` wrappers, plain size-capped replays re-checked after redaction, scenario pull request rules, attended or armed fleet runs (3.3.1, 4.8); conversation mode and the alone score (2.8); the live thumbnail, crop confirmation and route discovery (2.7, 2.9); the earcon and a 1.5 s first-word target; pause-then-decide barge-in; the hands-off pause; ordinals, diffs and compete summaries; approvals waiting until the job's deadline; exact-call grants; attributed narration and `ask_user`; approvals bound to their target; DirectWrite rendering; contact sheets and focus time for recall; visible intent cancelled only by addressed speech or stop words; `session_handoff` confirmation and `session_say` limits; the persisted app index; the Raw Input privacy check.
- Declined or deferred: encryption at rest with a DPAPI-wrapped key in v0.1 (deferred to v0.2 in D40: it adds a crypto dependency and key handling, and v0.1 relies on a user-only ACL and the profile's permissions, as the observability and learning-loop reports also propose); speculative brain turns as a default (kept as an opt-in experiment, because they spend plan and occupy the spare that guards against a crash); a low-level mouse hook in the HUD (replaced by Raw Input on the strip, with the hook only as a fallback if Raw Input proves unreliable).

**Engineering quality, testing and traceability.**

- Adopted: the `wp0/ci-bootstrap` order and a separate `pr-meta.yml`; the Evidence section with existence checks and the `evidence` job (check 8, 19); the recording, replay and fleet formats as contracts (3.3.16) with the RNG state in checkpoints and `CoreSnapshotV1`; the two replay modes with reactive responses and ID remapping, `bless` and `stale` (3.3.16); shipped `eileen-replay`, `eileen-scenarios` and `eileen-fleet`; the minimal runner in WP2; the spawn wrapper in `eileen-rt`; the hashed denylist; a hermetic catalogue (results go to Pages, never committed), scenario-only path classes and an auto-filled scenario pull request; generated per-crate Claude rules and the `Do not change without a decision record` heading; checks marked as `lint-repo` or delegated with `tools/repo-tools`; the check registry generating the table; the decision-record grammar taken from the existing log; the pull request title as the squash subject; `owner_pc::` and `real_claude::` modules; `long.yml`; shuttle tests in WP2; path classes, Windows-only test selection on pull requests, nextest timeouts, no extra toolchain target; the native-dependency table and check 15; the `[world]` section, `outcome`, `intent.expected`, step shapes and the generated event names; the fleet modes and the contributor allowlist; sharing from slice S1; `source` and `TraceCtx` on the speech pipe; the log-line ID assertion; stable generated output and the ARCHITECTURE.md resolution check; the settings registry test; the credential-free nightly CLI job and the Pages benchmark page; `measure-idle`; CI-only steps listed by `xtask ci`; the fuzz entry convention and corpus cap.
- Declined: committing `docs/SCENARIOS.md` at all (its results part cannot be hermetic, and friends' scenario pull requests could not regenerate it); a `nightly` prebuilt as an automatic default (it is an owner decision, section 7, item 5).

**The learning loop.**

- Adopted: the recipe contract with step references, per-op checks, `delegate` and `confirm` steps, idempotent retries, shadow agreement and eligibility (2.10, 3.3.13); `SkillInvoked`, `skills_used`, per-session snapshots from SQLite with manifests, and the `skill` field on `jobs_dispatch` (2.10, 3.3.9); the risk-weighted ladder, the run-index table, the success definition, "learn from that now", the pending rule (2.10); the generalisation rules (slot-free direct phrasings, the content-word rule, ambiguous slots, unseen enumerated values, precedence, template collision rejection) (2.4, 3.3.10); computed provenance, taint, never compiling denied or tier-2 steps, the deny-scan, lint errors returned to the librarian (2.10); secure delete, the full forget purge, unaddressed speech kept out of storage, the librarian seeing only jobs that ran on Claude, dictation as length only, `learning.distill` (2.10, 2.12); `eileen-skills` as the pure half behind the `SkillEngine` port, the vendored-C rule, the template grammar shared, learning scripts in WP7's crate (3.1, 3.3.15); facts with a deterministic path, `learn_fact_propose`, `learned_*` tools, "what do you know about me" and correction (2.10, 3.3.9); B8 as an A/B and B8-curve; the librarian's pre-filter, batching, plan-share caps and `librarian_runs`; app versions and tool schema hashes for probation, evidence-based self-healing; Resolve versions as the inverse and the `confirm` step; worker facts blocks and fuller tool-use capture; skills turned into local scenarios and offered for sharing; regret attribution rules; WP7 in two phases; the listing-budget probe; evidence shrinking, a user-only ACL and fact retirement; `source` facts for answers; `learn_skill_history`, recipe fuzzing and structure-keeping truncation.
- Declined: moving settings by voice into its own package (it stays in WP7's phase A because the core's settings flow, the HUD panel and renames all need it by W3, and splitting it would add a cross-package hand-off without saving time); putting the template grammar in `eileen-contracts` (it lives in `eileen-skills`, which `eileen-intent` may depend on, so the contracts crate stays free of parsing logic).
