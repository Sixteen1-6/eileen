# Gaze: eye tracking for Eileen

A proposal with a build brief, written 2026-10-26. It is for the owner, who decides what it changes in [BUILD.md](BUILD.md), and for the coding agent that builds it, the way [LINUX.md](LINUX.md) proposed WP10. Part A is the brief for the agent. Part B is the design. Statements marked [to confirm] are reasoning or vendor facts that the G0 spike (section 17) must verify before code depends on them. Sources are linked where they are used.

Scope. This plan uses a hardware eye tracker: a consumer device such as the Tobii Eye Tracker 5, read either through the gaze input API Windows provides for such devices or through the vendor's own runtime. A webcam estimator is set aside (section 18). Section 7 adds an experimental mode in which the eyes scroll a page from a set point.

One sentence: an eye tracker tells Eileen where the user is looking, and a spoken word acts on it. "Enhance" fills the screen with the dashboard card under the user's eyes. "Close this", "scroll down here", "what's this?" and "that one" stop needing a mouse. Gaze is a slot, never a trigger: it only resolves "this" and "there", and it never starts, approves or changes anything on its own. The one exception is the experimental scroll mode, which only a spoken word or a held key starts, which can do nothing but scroll one pane, and which stops when the reader's eyes come back to the set point.

---

## Part A. Brief for the coding agent

You are working in the Eileen repository, a Rust workspace with a sans-IO core (`Core::step`), supervised sidecars, one contracts crate whose JSON Schemas are snapshot-pinned, and a specification of record in `docs/BUILD.md`. Read these before touching anything, in this order:

1. `README.md`, `ARCHITECTURE.md` (the map and the invariants), `AGENTS.md` and `CONTRIBUTING.md` (the rules and the writing style).
2. `docs/BUILD.md` sections 2.2 (one request end to end), 2.4 (the fast path), 2.6 (the desktop server), 2.8 (the addressee gate), 2.9 (the HUD), 2.12 (tiers and taint), 2.14 (connectors), 2.16 (the machine profile), 3.3.3, 3.3.5, 3.3.6, 3.3.9, 3.3.10, 3.3.11, 3.3.12 (the contracts this touches), 3.4 (platform traits), 4.3 (budgets), 4.7 (tests), 6.1 (the build rules on the owner's PC).
3. The crate doc of `crates/eileen-contracts/src/lib.rs`, and `modules.toml` beside it, which says which contract modules are pinned and which are provisional.
4. `docs/LINUX.md` section 8 and `docs/wp/WP10-linux.md`: the precedent for proposing a package and the shape of a living plan.

### What you are building

Part B, in full. The short version: a new sidecar `eileen-gaze.exe` serving a room-sensing connector `pc.gaze` over a hardware tracker; fixation events into the core through the existing `Input::Sidecar` path; a `Gaze` slot in the intent resolver; one new desktop tool, `desktop_hit_test`; an "enhance" view drawn by the HUD from a clipped DWM thumbnail; the grammar entries of Part B section 5; and, behind an experimental setting, the set-point scroll mode of section 7. No new top-level `Input`, `Effect` or `BusEvent` variant is needed, and every contract change lands in a provisional module (section 8).

### Context you cannot see

The owner's reference for "enhance" is three screenshots of a dark finance dashboard ("Loud") in a browser: a line chart card, an "Activity by time" heatmap card (weekdays across, 1 pm to 6 pm down), and a recent-transactions card. In the first, a cyan "Gaze point" marker sits on the heatmap's Thursday 4 pm cell while the whole page shows. In the second, after "enhance", the heatmap card alone fills the screen, cells three times larger, with a back arrow top right and the gaze marker still on the Thursday 4 pm cell. That is the three-step system: look, say "enhance", the section under your eyes fills the screen. The fixture page of Part B section 10 reproduces that layout.

### Pull requests, in order

Every pull request is small (logic changes under about 500 lines), a draft from the first push, with the Evidence section first in its description and the `type:` line set (BUILD.md section 4b.3; `.github/PULL_REQUEST_TEMPLATE.md`). `all-green` and `pr-meta` must pass. Files no package owns (`docs/BUILD.md`, `docs/DECISIONS.md`, `docs/OWNER-DIRECTION.md`) change only through `amendment` pull requests the owner reviews (`.github/CODEOWNERS`, AGENTS.md rule 8), so this proposal lands on its own and the owner's answers are recorded by the amendment.

| Order | Branch | Type | What it holds | Stop after? |
|---|---|---|---|---|
| 1 | `wp11/gaze-proposal` | docs | This file as `docs/GAZE.md`; nothing else | Yes. Wait for the owner's answers to section 16 |
| 2 | `wp11/g0-spike` | docs | `docs/research/2026-11-gaze.md`: the measurements of section 17, with numbers and licence findings. Code written for the spike stays in `docs/research/fixtures/gaze/`, never in a crate | Yes. The owner picks the tracker and the route |
| 3 | `wp11/build-amendment` | amendment | The changes of section 14: BUILD.md (section 6.13 from section 12, a new 3.3.18 from section 8, the grammar, the tool, the settings, the trait, the budgets, invariant 17, the risks, a revision-log line), D81 from section 13 in `docs/DECISIONS.md` with `docs/PLAYBOOK.md` updated or a line saying why it is unaffected, a "Proposed 2026-10-26" entry in `docs/OWNER-DIRECTION.md` with the owner's answers, a v0.2 row in `docs/ROADMAP.md`, the process and code-map rows in `ARCHITECTURE.md` | No |
| 4 | `wp11/contracts` | feature | `crates/eileen-contracts/src/gaze.rs` (provisional, `pinned_by = "WP11"` in `modules.toml`), the fields it adds to the `connectors`, `hud`, `intent`, `config`, `scenario` and `tools/desktop` modules, one insta snapshot, `text-allowlist.toml` entries with reasons, a `fuzz_entry` for the recorded-sample format | No |
| 5 | `wp11/gaze-crate` | feature | `crates/eileen-gaze`: the fixation filter, the smoothed track for the scroll mode, the recorded-sample file format and its fuzz target `gaze_samples`, the sidecar's server logic against a fake `GazeSource`, the rate limiter, the nine-point check; all platform-neutral, tested on Linux | No |
| 6 | `wp11/core-gaze` | feature | In `eileen-core`: the gaze ring, the onset rule, `CoreCommand::Gaze`, the continuation that waits for `desktop_hit_test`, the back-mapping through an enhance view, the addressee feature, simulator invariant 17 part (a); the `Gaze` slot resolver in `eileen-intent` with golden-file cases | No |
| 7 | `wp11/hud-gaze` | feature | In `eileen-hud` and `eileen-hud-win`: the reticle, the `Enhance` panel from a clipped DWM thumbnail, the `gaze_live` indicator, `GazeOver` lines; draw-list snapshots | No |
| 8 | `wp11/desktop-hit-test` | feature | In `eileen-desktop` and `eileen-desktop-win`: `desktop_hit_test` and the section rule, with table tests over fixture trees; agreed with WP5, which owns the crate | No |
| 9 | `wp11/source-<kind>` | feature | `crates/eileen-gaze-win`: the `GazeSource` the spike chose, the `test-gaze` injector feature; the desktop-lane test on the dashboard fixture | No |
| 10 | `wp11/scenarios` | scenario | `scenarios/gaze/` of section 10, the dashboard fixture page, `docs/wp/WP11-gaze.md` kept current throughout | No |
| 11 | `wp11/scroll-law` | feature | The set-point law and session state machine of section 7 as pure code in `eileen-core`, with its unit, property and closed-loop model tests; nothing calls it yet | No |
| 12 | `wp11/scroll-glide` | feature | In `eileen-desktop` and `eileen-desktop-win`: `Glide` on `desktop_scroll`, `HitWant::ScrollContainer`, the server-side stops; agreed with WP5 | No |
| 13 | `wp11/scroll-session` | feature | The session wired end to end behind `gaze.scroll.enabled`: the `Track` stream, the anchor and band on the HUD, invariant 17 part (b), the reading fixture, the `scroll-` scenarios, the closed-loop injector | No |

Phases G3 and later of section 11 follow as further `wp11/<topic>` pull requests.

### Rules that do not bend here

- Gaze never approves, never starts, never changes. No approval, mute change, job start, setting change, first-run choice or consent is ever built from a gaze event. The scroll mode is the only place gaze moves anything, and it moves only the one pane its session opened on, only after a spoken start or while its key is held. Simulator invariant 17 (section 10) enforces both; a pull request that weakens it is closed. Approvals come only from `UserInput` (ARCHITECTURE.md, `eileen-core` invariants).
- Eileen never receives an image of the user's eyes. A hardware tracker's runtime gives points; no code path in Eileen asks for its camera frames, and none writes an image of the user to disk, calibration and incidents included. A repository check greps `eileen-gaze` and `eileen-gaze-win` for image encoders and fails on one.
- No new top-level `Input`, `Effect` or `BusEvent` variant. Gaze rides `Input::Sidecar(SidecarId::Gaze, ...)`, `Effect::Hud` and ordinary tool calls. If you believe one is needed, stop and write an amendment instead.
- Text from the screen is `Untrusted`. A section's label, an element's name, a window title: through `desktop::sanitize`, wrapped, fenced in the brain's context, never spoken beyond the bounded template of section 5.
- Build only through `cargo xtask` with the build guard (BUILD.md 6.1). Never `cargo build` directly, never `cargo install` from source, never a long build at the end of a session. Desktop tests take `desktop.lock` and drive only windows the test created.
- The owner's PC is memory-tight (1 to 2 GB free). The gaze sidecar is an optional resident the machine profile fills by value (BUILD.md 2.16). The vendor's own tracker service runs outside Eileen but on the same PC, so G0 measures it too.
- Licences. Nothing under a licence that conflicts with MIT enters a binary (CONTRIBUTING.md). A vendor runtime is loaded at run time from the user's own install, never bundled, unless its terms allow redistribution.
- Writing style. Plain, short sentences, tables where they beat prose, no em or en dashes anywhere in Markdown you touch, no emoji, no marketing words. Relative links must resolve.
- Evidence first. Each pull request leads with the tests, scenarios, numbers or replay that show it is right, in the form `pr-meta` checks.

---

## Part B. The design

## 1. Summary

- What exists. `DesktopState` already answers "what is the user looking at" from the foreground window, the focused element and the selection (BUILD.md 2.6). The desktop server already has `window_from_point` and `element_from_point` (3.4, `Grounding`), physical-input detection through Raw Input, and a visible-intent outline before every click. The HUD already draws a live DWM thumbnail of a window a job works in (2.9). The intent resolver already has typed slots and an `Ambiguous` path (2.4, 3.3.10). Connectors already have a `senses_room` flag with an opt-in rule (2.14, 3.3.12). Gaze makes "what the user is looking at" literal and plugs into each of those.
- What is new. A tracker behind a platform trait; a sidecar that turns its samples into fixations; a ring of recent fixations in the core; a rule that picks the fixation at speech onset; a `Gaze` slot that resolves "this", "that", "here" and "there"; a hit test that turns a point into a window, an element and a section; an enhance view; the grammar of section 5; and an experimental scroll mode that moves a page by how far the eyes sit above or below a set point.
- What it is for. The owner's "enhance" (section 4), hands-free control of what the eyes already point at (section 5), reading long pages with no hands (section 7), better grounding for the brain, and a step toward the accessibility goal that no task requires a mouse (OWNER-DIRECTION.md, "Accessibility as a first-class goal").
- What it costs. One more supervised process, about one percent of a core and tens of MB [to confirm], plus the vendor's service, and a tracker on the owner's desk.
- When. Proposed for v0.2, as WP11 (section 12), after WP5 and WP8 exist. v0.1 needs no change for it. The owner may pull G0 (the measurement spike) forward at any time, since it compiles nothing on the owner's PC.

## 2. Where gaze stands in the plan today

| Already in the design | Where | What gaze adds to it |
|---|---|---|
| "What am I looking at?" answered from DesktopState | E4, 2.6 | The literal answer: the section under the eyes, not only the foreground window |
| `Grounding::window_from_point`, `element_from_point` with deadlines | 3.4 | A point from the eyes instead of the mouse |
| Visible intent: outline and label 400 ms before any click, cancelled by "no, the other one" | 2.6 | The same outline on the gazed target before "click that" runs |
| The live DWM thumbnail on the HUD strip | 2.9, `HudState.watch` | The enhance view: the same mechanism, clipped to a section and scaled to the monitor |
| `desktop_scroll { hwnd, x, y, dx, dy }` at a point | 2.6, 3.3.9 | "Scroll down here" at the eyes, and a glide that the scroll mode renews ten times a second |
| `recent_references`, `ReferenceKind::{Window, Element}` | 3.3.5 | A gaze target becomes a reference the brain and the slot resolvers see |
| Slot resolvers that return `Ambiguous` instead of guessing | 2.4 | A `Gaze` slot with the same honesty: two candidates under the accuracy disc make a question |
| Room-sensing connectors: opt-in per device, local only, derived events only | 2.14, 3.3.12 | `pc.gaze` is exactly that kind of connector |
| The local addressee score, one point per feature | 2.8 rule 5 | One more feature: the user was looking at the HUD or the screen at onset |
| "No task requires a mouse" as a tested rule | 2.9 | Gaze plus a word reaches what a click reaches |
| The machine profile fills optional residents by value | 2.16 | The gaze sidecar is one of them |

## 3. Principles

1. Gaze is a slot, never a trigger. A spoken word, a hotkey or a physical click starts every action. Gaze only answers "which one". Looking at something for a long time does nothing, which is why the classic "Midas touch" problem of gaze interfaces does not arise. The scroll mode of section 7 keeps this: a word or a held key opens it, and inside it the eyes set only the speed and direction of one pane.
2. Deterministic first. "Enhance", "close this", "scroll here" and "click that" run on the fast path with no model: a fixation, a hit test, a tool call. The scroll mode is a pure control law in the core. The brain gets gaze as context for the requests that need interpretation.
3. Local, opt-in, and visible. Nothing leaves the PC. The connector starts only with a recorded opt-in. A HUD indicator and the tray show whenever the tracker is live, like the recording indicator. "Stop watching my eyes" pauses it at once.
4. The user's hands still win, and tiers do not move. A gaze-resolved click is the same click: the same visible intent, the same per-op tier, the same refusals (password fields, Eileen's own windows, blocked combos, other processes' windows). A gazed Send button still asks. Any physical input ends a scroll session.
5. Honest degradation. No tracker, lost eyes, a coarse calibration or an ambiguous disc each fall back to today's behaviour or to one short question, never to a guess. Every command of section 5 works without gaze exactly as it does now.
6. Accuracy decides what is offered. The tracker's measured accuracy and precision, in pixels, come from Eileen's own nine-point check on this PC. A section needs about 60 px, an element 25 px, a heatmap cell 12 px on the owner's display. Below the need Eileen asks or says it cannot tell, and the enhance view is how a tracker reaches small targets, because inside it a cell is three times larger. The scroll mode needs precision more than accuracy (section 7.4).

## 4. Enhance, in three steps

The worked example, from the owner's screenshots. Edge shows the dashboard. Eileen's check measured the tracker at 40 px accuracy. Times are the budgets of section 9.

| Step | What happens | Under the hood |
|---|---|---|
| 1. Look | The user looks at the "Activity by time" card. If `gaze.show_reticle` is on, the overlay draws a faint ring where Eileen thinks the eyes are | The sidecar's fixation filter emits `Fixation { Started }`, then updates; the core keeps it in the gaze ring (section 6.4) |
| 2. Say | "Eileen, enhance." | At `SpeechStarted` the core snapshots the fixation current at onset as the turn's `GazeRef`. At `EndOfTurn` the fast path matches the built-in entry and returns `CoreCommand::Gaze(Enhance { In })` |
| 3. See | Within half a second of the end of speech, the card fills the monitor's work area, letterboxed, live. It keeps updating because it is a composition of the real window, not a screenshot. The reticle, if on, keeps following the eyes inside the view. The page beneath is untouched: no focus change, no input, nothing to undo in Edge | The core issues `desktop_hit_test { point, radius_px: 40, want: Section }` with origin `Core`. The desktop server runs `WindowFromPoint`, then `ElementFromPoint` on Edge's web content (the on-demand UIA request WP5 already makes before typing into a Chromium window), walks up the ancestors and applies the section rule of section 6.6: the nearest named group whose rect is between 3 and 60 percent of the window is "Activity by time". The core emits `Effect::Hud` with `HudPanel::Enhance { hwnd, source, label, level: 1 }`. The HUD registers a DWM thumbnail of that window with the card's rect as its source rectangle and the work area as its destination |
| 4. Ask | "What's this one?" while looking at the Thursday 4 pm cell in the view | The onset fixation lies inside the view, so the core maps it back through `EnhanceMap` to the real cell's pixels; at three times the scale, 40 px of error in the view is about 13 px on the page, inside a cell. It hit-tests again with `want: Element`, and the fast path speaks the bounded template "That's Thursday, 4 pm, 93" from the sanitized name; a question that needs interpretation goes to the brain with a `gaze:` line and `LookTarget::Gaze` |
| 5. Deeper | "Enhance" again | Level 2: the section rule runs on the back-mapped point with the current section as the ceiling, so the heatmap grid alone fills the screen. `gaze.enhance_max_level` (3) caps it |
| 6. Back | "Back", "done", Esc, or any physical click | The panel closes. `Back` is the inverse the journal records, so "undo that" works too |

What enhance refuses, in one sentence each: a minimized or cloaked window (DWM renders no thumbnail of it), a private window ("that window is private"), a window whose thumbnail DWM leaves black (protected content, or a window that excludes itself from capture; whether that can be told in advance from its display affinity is a G0 question [to confirm]), and the HUD itself.

Later rungs. When an app can enhance itself better than a pixel scale can, the route table of 2.5 prefers it: a browser's own fullscreen of the element through the Playwright `Dom` rung when the user installed it, Resolve's viewer zoom through `apps_resolve`. The first version ships the thumbnail route only, because it touches no app and needs no grant.

## 5. The use cases

Each line works without gaze exactly as today. With gaze, the `Gaze` slot binds the demonstrative. The tier is the existing tool's tier. G1 to G9 are phases G2 and G3; G10 to G14 are phase G3; G15 is phase G4 (section 11).

| # | You say | What happens | Under the hood | Tier |
|---|---|---|---|---|
| G1 | "Enhance" / "zoom in on that" / "make that bigger"; "enhance more"; "zoom out"; "back" | Section 4 | `CoreCommand::Gaze(Enhance)`, `desktop_hit_test`, `HudPanel::Enhance` | none (HUD only), journaled with `Back` as its inverse |
| G2 | "What's this?" / "what am I looking at?" (E4, now literal) / "read this to me" / "summarise this" / "translate this" | A bounded spoken line from the element's or section's accessible name when one exists; otherwise, or for a question, the brain answers from `desktop_look { target: Gaze }` (one downscaled capture of the section's rect) and the fenced tree text | `Gaze(Describe)`; `LookTarget::Gaze` and `LookTarget::Region { rect }`; observed source `Screen` or `Tree` by the existing table | 0 |
| G3 | "Click that" / "press that" / "tick that" / "open that" (two clicks) | The outline and label appear on the gazed element for 400 ms, then `desktop_click_element` on it. A send-like name is tier 2 and asks. Eileen's own windows, other processes' windows and password fields are refused by the server's existing rules | `Gaze(Click)`; the hit test returns the element's index and `Selector`; an `Act` job holds the input lock for one event; the outline stands in for the lease countdown [to confirm with WP5's lease rules] | 1, 2 ByArgs |
| G4 | "Scroll down" / "scroll up here" / "scroll down a lot" | The pane under the eyes scrolls one step, not the focused one | `Gaze(Scroll { dx, dy })` to `desktop_scroll { hwnd, x, y, dx, dy }` at the fixation point, which that tool already takes | 1 |
| G5 | "Type here" / "put it here" / "dictate here"; or hold the dictation key while looking at a field | If the gazed element is editable and not focused, the outline appears, the server focuses it (`desktop_click_element`), then the normal insertion ladder runs. Password fields refused as always | `Gaze(FocusField)`; `InsertTarget::Element` | 1 |
| G6 | "Close this" / "minimise this" / "maximise this" / "snap this to the left" | On the gazed window rather than the foreground one | `Gaze(Window(WindowAction))` to `desktop_window` with the hit test's `WindowRef` | 1, close 2 when unsaved state is unknown |
| G7 | "Move this there" / "put this on that screen" | "This" binds to the onset fixation, "there" to the fixation current at the end of the utterance, so one sentence names a window and a monitor by eye | The `Gaze` slot's two values, `Onset` and `End` (section 6.5); `WindowAction::Snap { monitor }` | 1 |
| G8 | While a question is open: "that one" / "this one" | The option, contact-sheet image or clarifying choice under the eyes is chosen. The spoken words are the answer; gaze only picks which. Never an approval | `HudInput::GazeOver { item }` from the HUD (section 6.7), `HudInput::Answer { id, option }` built by the core from the spoken turn | 0 |
| G9 | Looking at a job row on the HUD: "cancel it" / "how's this one?" / "show me its changes" / "pause this one" | The job under the eyes is the referent, ahead of ordinals and names | `GazeOver { Job(id) }` fills `JobRef` for the existing commands | as today |
| G10 | Speaking while looking at the HUD or the screen, with no name | One more point in the local addressee score, so unnamed requests to the PC pass more often and room talk toward another person does not. Looking away never subtracts a point, because people look at the keyboard | 2.8 rule 5 gains the feature `gaze_on_eileen` (two points when the fixation is on an Eileen window, one when on any screen); `GazeRef.on_eileen` | none |
| G11 | Unnamed requests about what is on screen: "why is Thursday so busy?" | The brain's `[desktop]` block carries a `gaze:` line (section 6.8), so the question is grounded without a screenshot | The gaze target as a `Reference` with a new `ReferenceKind::Gaze` in `DesktopState.recent_references` | 0 |
| G12 | "Where was I?" / "what was I reading before the call?" | Answered from the episodic ledger's dwell rows: window, document identity and seconds looked, never positions or content. Off by default, and refused while the tracker's terms allow live use only | `gaze.ledger`; a `Dwell` row in the ledger; the 14-day retention and off-the-record rules of 2.10 apply | 0 |
| G13 | Every HUD control by "that one" plus a look | The accessibility test of 2.9 extends: every clickable `HudInput` is reachable by its grammar entry or hotkey, and, with gaze, by `GazeOver` plus "that one". The physical-only inputs (Allow, Deny, mute, consent) stay physical-only | the test table gains a column | as today |
| G14 | "Calibrate my eyes" / "check my eyes" / "stop watching my eyes" / "watch my eyes again" / "forget my gaze check" | Calibrate opens the vendor's own calibration, then runs Eileen's nine-point check; check runs the check alone (nine targets on the overlay, read aloud) and says what it found ("sections and buttons will work; small cells only in the enhanced view"); pause stops the tracker and the indicator goes off; forget deletes `gaze\check.toml` and says so, and that the vendor keeps its own calibration | `CoreCommand::GazeControl`; `gaze_control` served by `pc.gaze`, core-only | settings flow; `gaze.enabled` is a Safety key |
| G15 | "Scroll with my eyes" (or hold the eye-scroll key); then just read; "stop" | Experimental. A faint line marks the set point on the page under the eyes. Looking below it scrolls down, above it scrolls up, faster the farther from the line, and the page eases to a stop when the text you were looking at reaches the line. Ends at "stop", any touch of the mouse or keyboard, or when the eyes leave the page | Section 7: `CoreCommand::Gaze(ScrollMode(Start))`, the `Track` stream, the set-point law, `desktop_scroll` with `Glide` | 1 |

What gaze never does. It never approves, denies, mutes, unmutes, consents, answers the first run, starts a job, changes a setting or takes dictation. A look at the Allow button does nothing; "click that" on it is refused, because the HUD is one of Eileen's own windows. A look never pauses a job, never wakes a lease, never cancels visible intent (only addressed speech and stop words do). It never selects text and never moves the mouse. It scrolls only inside a scroll session the user opened by voice or by holding its key, and only the pane that session opened on. A tracker fooled by a photo therefore changes nothing by itself: an attacker would need the user's spoken command too, which is the same threat class as another voice in the room, with the same mitigations (2.8).

## 6. Architecture

### 6.1 Process and connector

A new supervised sidecar, `eileen-gaze.exe`, joins the processes table of 2.1:

| Process | Contains | What a failure costs | Transport to the core |
|---|---|---|---|
| `eileen-gaze.exe` | The tracker source (the Windows gaze input API or the vendor's runtime), the fixation filter, the smoothed track, the nine-point check, the rate limiter | Gaze stops; every command of section 5 falls back to today's behaviour; an open scroll session ends; the HUD says "eyes off" on the indicator | MCP over stdio, like the desktop and apps sidecars: a `connector_manifest` answer, then `ConnectorEvent::Gaze` events, and the `gaze_control` tool |

It is connector `pc.gaze`, the third PC connector beside `pc.desktop` and `pc.apps`: `RunsIn::Sidecar { binary: "eileen-gaze.exe" }`, `senses_room: true`, `requires_opt_in: true`, events `["gaze.fixation", "gaze.track", "gaze.lost", "gaze.found", "gaze.check", "gaze.health"]`, no checks (nothing of it is a recipe check), tools `[gaze_control]`. The registry refuses to start it without the recorded opt-in (2.14), which `gaze.enabled` writes through the settings flow as a Safety key. It is one of the optional residents of the machine profile's plan (2.16): kept when it fits, dropped under pressure with the usual hold, and the reason shown on the machine panel. If D79's general `Input::Connector` lands before WP11, gaze moves to it with the other connectors; until then it is a sidecar like desktop and apps.

The core never links it (rule 1 of 2.1 holds: `eileen.exe` links nothing that can hang). The HUD never talks to it: the reticle and the scroll band come through `HudState` at the HUD's ten snapshots a second, which is enough for an indicator and keeps every data path through the core.

### 6.2 Sources

Two routes to a hardware tracker sit behind one trait, `GazeSource` (section 6.12). The G0 spike measures both on the owner's PC with the tracker the owner picks, and the owner picks the first route to build.

| Route | How | Accuracy [to confirm] | Licence and consent questions |
|---|---|---|---|
| Windows gaze input (`Windows.Devices.Input.Preview`: `GazeInputSourcePreview`, `GazeDeviceWatcherPreview`) | The OS-level API behind Windows Eye Control, which supported trackers feed. Vendor-neutral, samples in screen coordinates. Microsoft's reference for [`GetForCurrentView`](https://learn.microsoft.com/en-us/uwp/api/windows.devices.input.preview.gazeinputsourcepreview.getforcurrentview) says it must be called on the UI thread and that the first call shows a one-time privacy prompt; it says nothing about desktop apps. Whether a Win32 process receives samples, and whether it needs a view or a package identity, is the spike's first question | About 0.5 to 1 degree, roughly 30 to 60 px on the owner's display at arm's length | The OS ships it; the user grants consent once in Windows' own prompt. What the tracker vendor's terms say about apps reading its device through Windows is still read in G0 |
| The vendor's runtime (for Tobii consumer trackers, its game-integration or stream API) | The vendor's DLL at run time, loaded only if present, never bundled | As above | Tobii's [developer licence page](https://developer.tobii.com/license-agreement) limits its consumer SDK to interactive applications and sends analytical uses, such as processing gaze data after the fact or across users, to its Pro line. That page describes the older EyeX SDK, so the current runtime's terms are read in G0. Eileen's live use is interaction; the dwell ledger (G12) is analytical, so such a source reports `stores_nothing: true` and the ledger stays off. Redistribution is likely not allowed, so it is the user's own install |

The tracker. Tobii's [specifications for the Eye Tracker 5](https://help.tobii.com/hc/en-us/articles/360012483818-Specifications-for-Eye-Tracker-5) list a 133 Hz gaze sampling rate, Windows 10 and 11, USB, and screens from 15 to 27 inches at 16:9; they state no latency or accuracy, which is why G0 measures both here. Other trackers that feed Windows gaze input work through the first route with no Eileen change.

Every source reports `GazeSourceInfo { kind, accuracy_px, precision_px, rate_hz, monitors, stores_nothing, checked }`. Accuracy (the mean distance from a target) and precision (the spread of samples while the eyes hold still) come from Eileen's last nine-point check, not from a datasheet, so the "accuracy decides what is offered" rule (principle 6) rests on this PC.

Calibration. Hardware trackers calibrate through the vendor's own app (and Windows Eye Control through its own flow), and Eileen does not replace that. "Calibrate my eyes" opens the vendor's calibration where the route allows it [to confirm], then runs Eileen's check; the check may also fit a small per-monitor offset correction (two numbers) when the measured error has a consistent direction, kept in `gaze\check.toml`.

Multi-monitor: a tracker covers the monitor it is mounted on. The first version covers that monitor only (the owner's PC has one). A point off every covered monitor reports `Lost { OffScreen }`.

### 6.3 From samples to fixations, and the smoothed track

The sidecar's neutral crate, `eileen-gaze`, owns the filters and is tested on Linux over recorded sample files.

- Samples arrive at the tracker's rate (around 33 to 133 Hz) as `GazeSample { at, point: Option<Point>, confidence_pct }`, a sidecar-internal type that is never a contract type and never written to disk by a shipped binary. The `test-gaze` Cargo feature (never enabled in a shipped binary, like `test-physical`) adds a named-pipe injector that feeds synthetic samples in the same type, so the desktop lane can drive every path without a tracker.
- Fixations come from a dispersion-threshold filter: a run of samples at least `gaze.fixation_ms` long (default 120) whose points stay within `gaze.dispersion` (default `auto`: the larger of `accuracy_px` and one degree at the measured distance) is one fixation; its centroid is the point. A fixation emits `Started`, then `Updated` at most ten times a second, then `Ended`. Saccades emit nothing.
- The smoothed track exists for the scroll mode only, and only while the core has asked for it (`gaze_control { Track { on: true } }`). Reading moving text is smooth pursuit, which a dispersion filter turns into a string of short fixations or none, so the scroll mode cannot use fixations. The track is the sample stream through an adaptive low-pass filter of the One Euro kind (strong smoothing when the eyes are nearly still, little lag when they move), sent as `Track { point }` at ten a second, `None` while the eyes are lost. While the track is on, fixation updates are thinned so the total stays at ten events a second.
- Lost and found. No eyes for 300 ms is `Lost { NoEyes }`; a point off every covered monitor is `Lost { OffScreen }`; a device gone is `Lost { DeviceGone }`; `Found` ends it.
- Rate. At most ten events a second reach the core; raw samples never do, as PCM never does from the speech process.
- The reticle is drawn at the fixation centroid, so it sits still on a fixation and jumps between them. That is what the owner's screenshots show, and it is honest about what the core knows.

### 6.4 The core: the gaze ring and the onset rule

The core keeps a ring of the last eight seconds of fixations, timestamped with the `now` passed to `Core::step`, so the simulator replays it exactly.

The onset rule. When the speech process mints a turn (`VoiceEvent::SpeechStarted`), the core snapshots a `GazeRef` from the ring:

1. The fixation current at onset, if its point is not on one of Eileen's own windows.
2. Else the longest fixation that ended within `gaze.onset_window_ms` (default 1,500) before onset and was not on an Eileen window. People look at the assistant when they address it, so the fixation before the glance at the HUD is the target.
3. Else none: every command of section 5 falls back.

A second `GazeRef` is taken at `EndOfTurn` for the `End` slot value (G7). Whether the onset fixation was on an Eileen window or on any screen is kept as `on_eileen` and `on_screen` for the addressee feature (G10), with no hit test.

Why onset. Gaze leads speech by about half a second to a second: by the time "enhance" is finished the eyes have often moved to the HUD or elsewhere. The rule is a golden-file case in WP7's resolver tests, and the desktop-lane trials of section 10 measure how often it picks what the user meant.

Inside an enhance view. If the fixation's point lies within the open panel's destination rect, the core maps it back through `EnhanceMap` (`source.origin + (point - destination.origin) / scale`) before any hit test, and marks `GazeRef.through`. The round trip is a property test.

The continuation. The core is sans-IO, so a hit test is an effect and its answer an input. `CoreCommand::Gaze(...)` makes the core emit a `ToolCallRequest` for `desktop_hit_test` with origin `Core` (the origin the intent highlight and the media pause use, 3.3.2) and park the command under a timer (deadline 300 ms, section 9). On the `ToolResult` it finishes: builds the real tool call (`desktop_window`, `desktop_scroll`, `desktop_click_element`), the HUD panel, the spoken template, the scroll session, or the brain turn. On the timer, or an error, it falls back to today's behaviour and says why in one line when the command cannot run at all. A brain turn whose `GazeRef` exists waits for the hit test too (in parallel with the earcon), so the `[desktop]` block can carry the target.

### 6.5 Resolving "this": the Gaze slot

A new slot resolver, `Gaze`, joins `Offset`, `Count`, `App`, `File`, `Monitor` and the others of 2.4. It binds the demonstratives `this`, `that`, `this one`, `that one`, `here`, `there`, `this window`, `that window`, `this app`, `this section`, `this part`, `this bit`, and a demonstrative followed by a generic noun (`that button`, `this field`, `that cell`, `that row`), to one of two values:

| Value | Bound to | Used by |
|---|---|---|
| `Onset` | The `GazeRef` taken at speech onset | Every demonstrative, by default |
| `End` | The `GazeRef` taken at the end of the turn | `there`, `that screen`, and any second demonstrative in a sentence that already bound one at onset (G7) |

The resolver sees only `ResolveCtx.gaze: Option<GazeCtx { usable, accuracy_px, on_eileen, enhanced }>`, never the point; it decides whether the slot can bind. The core substitutes the `GazeRef` and runs the hit test. Rules, each a golden-file case:

- An explicit name beats gaze: "close Chrome" never looks at the eyes, and "close this" with a usable fixation on Edge closes Edge even when Chrome has focus.
- No usable fixation: the slot falls through to today's resolvers (the foreground window, `recent_references`, `last_turn`), then to `Ambiguous` ("which window?").
- A generic noun narrows the hit test: `that button` asks for `want: Element` and refuses a hit whose control type is not a button-like one, so "click that button" over a label asks "the Save button or the Cancel button?" from the candidates.
- Two or more candidates whose rects meet the accuracy disc make the slot `Ambiguous { candidates }`; the core asks with the HUD outlining each, as for any ambiguous slot, and "that one" (G8) or a name answers.
- Replay never runs on an ambiguous or missing gaze slot (the existing rule), so a learned skill with a gaze slot resolves it live or hands off.
- Dictation mode never consults gaze except for `FocusField` through the dictation key (G5).

### 6.6 The hit test and the section rule (desktop server)

One new tool in the desktop toolset, the nineteenth `desktop_*` tool, served by `eileen-desktop` and backed by `Grounding::window_from_point`, `Grounding::element_from_point` and a new `Automation::ancestors`:

```rust
pub struct DesktopHitTest { pub point: Point, pub radius_px: u16, pub want: HitWant }   // Window | Element | Section | ScrollContainer.
    // Tier 0, read_only, idempotent, observes ByTarget (Tree; Web for a browser's web content or a mail, chat or social
    // app, by 2.12's table); deadline 300 ms; output GazeHit
```

The section rule, applied to the ancestor chain nearest first, with the window's client rect as the frame (or the current enhance section's rect as the ceiling at level 2 and above):

1. The element has a non-empty name, or is a landmark or region (web `section`, `article`, `figure`, `region`, a heading-labelled group; Win32 and WinUI `Group`, `Pane`, `Custom` or `Table` with a name).
2. Its rect covers between 3 and 60 percent of the frame.
3. It is not a scroll container that holds three or more elements that themselves pass rules 1 and 2 (a page body is not a section).
4. The first ancestor that passes is the section. If none passes, the section is the quarter of the frame around the point, labelled by the window's title, so "enhance" always does something sensible.

The scroll-container rule (for `want: ScrollContainer`, section 7.6): the nearest ancestor that supports UIA's Scroll pattern and is vertically scrollable, else the window when it accepts wheel input at that point; the answer names the route (`Pattern` or `Wheel`) and the visible share of the content when the pattern reports it.

The thresholds are data in `eileen-desktop` (`gaze-sections.toml`), tested over fixture trees: the dashboard page, the reading page, the testpad, Notepad, Explorer, and a Resolve page recorded as a tree. `candidates` lists every element whose rect meets the disc of `radius_px` around the point, nearest first, so the resolver can ask. A private window returns `private: true` with no element, no section, no scroll target and no label. An Eileen window returns the window only.

Cost: `WindowFromPoint` is microseconds; `ElementFromPoint` is a UIA call on the dedicated MTA thread with the usual deadline and abandon-on-hang rule (2.1 rule 2). On a Chromium window it needs the on-demand accessibility request WP5 measures in its probe (acceptance 6); the gaze package records its own numbers beside them.

### 6.7 The enhance view, the reticle and the scroll band (HUD)

- The reticle. `HudState.gaze: Option<HudGaze { point, accuracy_px, lost }>` draws a ring of the accuracy's radius on the click-through overlay, at the HUD's ten updates a second. Off by default (`gaze.show_reticle`), on during the nine-point check and for the first week after enabling, when Eileen says once "I'll show you where I think you're looking; say 'hide the ring' when you trust it".
- The enhance panel. `HudPanel::Enhance { hwnd, source: Rect, label: Untrusted<String>, level: u8 }`. The HUD draws it on a full-work-area window that is not click-through (so the user's next physical click dismisses it, as a lightbox does), that keeps `WS_EX_NOACTIVATE` (it never takes focus) and `WDA_EXCLUDEFROMCAPTURE` (it never appears in Eileen's screenshots). The content is a DWM thumbnail (`DwmRegisterThumbnail`) with `rcSource` set to the section's rect and `rcDestination` the letterboxed work area, so it costs no capture and shows covered windows, as the strip's thumbnail already does. `HudSurface::set_watch` gains a source clip; a new `set_enhance` sets the panel. The label is drawn as the view's title with a back arrow, and the panel sends `HudInput::Back` on the arrow (physical) or on any click.
- The scroll band. While a scroll session is open, `HudState.gaze_scroll` (section 7.8) draws, on the click-through overlay and clipped to the pane, a thin line at the set point and a faint tint over the stop band, plus a small chip at the pane's edge ("eye scroll: say stop"). The band is what the user aims for, so it is visible intent for the whole session.
- Gaze over HUD items. The HUD computes its own layout, so it is the one that knows which job row, approval, option or contact-sheet image a point lies on. It reports `HudInput::GazeOver { item: HudItem }` lines with `physical: false`, at most twice a second and only when the item changes. The core uses them for G8, G9 and G13 only. The accessibility test's table marks `GazeOver` as not clickable, so it needs no grammar entry.
- The indicator. `Indicators.gaze_live: bool`, drawn whenever the tracker is live, never hidden by a setting, with the same tray mirror the recording indicator has.
- The nine-point check. Nine targets drawn on the overlay in turn while Eileen reads "look at the dot" and counts; the sidecar collects samples per target and reports `GazeCheck { points, accuracy_px, precision_px, offset, ... }`. A result worse than `gaze.max_accuracy_px` (default 120) is kept, and Eileen says that only sections will work.

### 6.8 The brain

- The `[desktop]` block of 2.3 gains one line when the turn has a target: `gaze: Edge "Loud" > section "Activity by time" (2.3 s)` or `gaze: cell "Thursday 4 pm, 93" in section "Activity by time"`, inside the fenced data block with every other screen text. Caps: one line, 120 characters.
- `LookTarget` gains `Gaze` (the turn's section or window) and `Region { rect }`, so `desktop_look` can capture the section alone, downscaled, which is cheaper and more useful than the whole screen. Observed source `Screen`, as today.
- `DesktopState.recent_references` gains the target as `Reference { kind: Gaze, label, window, at }`, so follow-ups inside the conversation window ("and the one next to it?") resolve through `last_turn` as they do for other references.
- `gaze_control { action: Status }` is the only `pc.gaze` op the brain may call (tier 0), so "is the eye tracker on?" is answered honestly. The brain cannot open a scroll session.
- The brain never receives points, samples or tracks from the gaze path.

### 6.9 Settings

A new `[gaze]` section of `Config` (3.3.11), every key reachable by voice and on the HUD like every other:

```toml
[gaze]
enabled = false            # Safety (NeedsUserApproval): starts the tracker; the recorded opt-in of 2.14
source = "auto"            # auto | windows | vendor
show_reticle = false       # Low, Live
fixation_ms = 120          # Low
dispersion = "auto"        # px, or auto from the check's accuracy; Low
onset_window_ms = 1500     # Low
enhance_max_level = 3      # Low
max_accuracy_px = 120      # above this check result, element commands are not offered; Low
addressee_feature = true   # G10; Low
ledger = false             # G12; Safety, and refused while the source reports stores_nothing
# [gaze.scroll], experimental: section 7.8
```

`gaze.*` joins the Safety prefix list of 2.11 beside `desktop.*`, `hotkeys.*` and `fleet.*` for the keys marked Safety. "Calibrate my eyes", "check my eyes" and "stop watching my eyes" are grammar entries, not settings; "hide the ring" and "show the ring" are the `show_reticle` phrases.

### 6.10 Privacy, security and tiers

| Rule | How it is kept |
|---|---|
| Eileen never sees an image of the user's eyes | The sources it uses give points only; no image encoder is linked into `eileen-gaze` or `eileen-gaze-win`, and a repository check greps both crates for image encoders and fails on a hit; the check stores only error figures and the offset in `%LOCALAPPDATA%\eileen\gaze\check.toml` |
| Only fixations and, during a scroll session, the track reach the core, at most ten a second | The rate limiter in `eileen-gaze`; a property test |
| Gaze is never `UserInput` | `GazeEvent` arrives as `Input::Sidecar`; `GazeOver` lines always carry `physical: false`; the core's approval type cannot be built from either (ARCHITECTURE.md invariant). Simulator invariant 17 (section 10) |
| Tiers do not move | A gaze-resolved call is dispatched like any other, with its own op tier, taint and journal; the fast path's new ability to issue `desktop_click_element` and `desktop_scroll` is an `Act` job with the desktop toolset, which the dispatcher's origin table of 3.3.9 gains for origin `FastPath` with those two tools only |
| The scroll mode moves one pane and nothing else | Opened only by an addressed turn or the physical hotkey; only `desktop_scroll` glides on the pane it opened on; the desktop server stops a glide by itself on physical input and when a glide's lifetime runs out; invariant 17 part (b) |
| Private windows | A fixation on an `is_private` window hit-tests to the window alone, with no element, no section, no scroll target and no label; the enhance view and the scroll mode refuse it; the ledger never records it |
| Off the record | The sidecar keeps running (the user may still say "enhance"), but no fixation, dwell row or `GazeRef` is persisted; the recorder's existing rule |
| Recordings and incidents | Fixation and track events are in the flight recorder as points under the `pii.` policy, so an incident bundle holds them only with `logging.include_personal` on; the scenario redactor drops them from a shared scenario unless the scenario's steps need them, in which case they are the synthetic points of its gaze steps |
| The indicator cannot be hidden | `gaze_live` has no setting; the tray mirrors it |
| Spoofing | A photo or a video before the tracker steers nothing by itself (section 5, "What gaze never does"); the spoken command is the trigger, with the speaker-gate and voice-window rules of 2.8 unchanged. At worst a spoofed gaze scrolls the pane of a session the user opened, and the user's own hands stop it |
| Vendor terms | `stores_nothing` from the source's terms keeps the ledger off; nothing analytical runs on such a source |

### 6.11 Learning

- Every gaze-resolved action is an `Act` job, so it gets a trajectory, the regret window and undo as every action does (2.10). The trajectory step records the resolved `WindowRef`, `Selector` and section label, never points.
- A skill drafted from such a job gets a `Gaze` slot in its template ("close {this:gaze}"), resolved live at replay; a lost or ambiguous gaze refuses the replay and the brain or a worker takes over, by the existing rule.
- A scroll session drafts no skill: it is a manual control, recorded in the journal as one entry with its net distance (section 7.7).
- Phrasings work as today: "make that bigger" confirmed once runs "enhance" from the fast path.
- "Which one did I mean?" errors flow into the regret window: "no, the other one" after a gaze-resolved click counts as a correction for that job, and the resolver's golden file grows with the case.

### 6.12 Platform

A new trait in `eileen-platform`, implemented in `eileen-gaze-win`. A Linux implementation waits for a tracker with a Linux runtime; the neutral crate stays green on Linux either way.

```rust
pub trait GazeSource: Send + Sync {        // eileen-gaze-win: Windows gaze input, or the vendor's runtime
    fn info(&self) -> Result<GazeSourceInfo, PlatformError>;
    /// Starts the tracker; samples go to the sink until the subscription drops. Raw samples never leave the sidecar.
    fn start(&self, sink: Box<dyn Fn(GazeSample) + Send + Sync>) -> Result<Subscription, PlatformError>;
    /// Opens the vendor's own calibration where the route can, and says what it did: Started | OpenedVendorApp | Unsupported.
    fn calibrate(&self, deadline: Deadline) -> Result<CalibrationStart, PlatformError>;
}
pub struct GazeSample { pub at: MonoTime, pub point: Option<Point>, pub confidence_pct: u8 }   // sidecar-internal, never a contract
```

The nine-point check needs no trait method: the sidecar collects samples per target from the same sink. The fake under `cfg(any(test, feature = "fake"))` plays recorded sample files, and the `test-gaze` injector feeds the real sidecar the same type over a named pipe; for the scroll tests it also runs closed loop (section 10). `Automation` gains `ancestors(element, deadline)`; `HudSurface` gains the clip and `set_enhance`.

## 7. Experimental: scrolling by gaze from a set point

### 7.1 The idea

The user fixes a reading line on the page. Looking below the line scrolls down, looking above it scrolls up, and the speed grows with how far the eyes are from the line. When the eyes come back within a small band around the line, scrolling stops. The reason this works is that the reader's eyes stay on the text they want: as the page moves that text toward the line, the eyes follow it there, the distance shrinks, the page slows, and it stops when the text sits on the line. The eyes are the error signal of a closed loop, and the stop is the moment the wanted text has entered the reader's focus.

It is experimental because it is the one place gaze drives something continuously. It lives behind `gaze.scroll.enabled` (off), it is measured by the trials of 7.9 before it loses the label, and every rule of 7.7 holds from its first pull request.

### 7.2 How it feels

| Step | What the user does | What Eileen does |
|---|---|---|
| 1. Start | Says "scroll with my eyes" (or holds the eye-scroll key) while looking at a long page | Hit-tests the point for the scroll container (section 7.6). Sets the anchor at the current gaze height, clamped to the middle half of the pane. Outlines the pane for 400 ms, then draws the anchor line and the stop band. One earcon |
| 2. Look down | Looks at a paragraph near the bottom of the pane | After the eyes have stayed beyond the start band for 200 ms, the page starts to move up, gently at first, faster for a paragraph far below the line |
| 3. Follow | Keeps looking at the paragraph as it rises | The paragraph's distance to the line shrinks, so the page slows |
| 4. Settle | Nothing | When the eyes, still on the paragraph, enter the stop band, the page stops. The paragraph sits on the reading line |
| 5. Read on | Reads down the paragraph | Reading a line or two below the band does nothing. Reading past the start band for 400 ms eases the text back up to the line, so long reading needs no hands |
| 6. Back up | Looks above the line at a heading that is scrolling out of view | The page moves down until the heading reaches the line |
| 7. End | Says "stop", touches the mouse or keyboard, or looks away from the page for 3 s | The band disappears; one earcon; the journal holds one entry, so "undo that" scrolls back to where the session began |

Looking at something outside the pane (the HUD, the taskbar, another window) pauses motion at once; it does not scroll at the speed that distance would give.

### 7.3 The control law

Screen y grows downward. Let `a` be the anchor's height, `g` the smoothed gaze height from the track, and `d = g - a` the offset (positive below the anchor). Two bands around the anchor: the stop band of half-height `s` and the start band of half-height `b`, with `b > s`.

```text
state Holding (no motion):
    if the gaze is inside the pane and |d| > b for start_dwell (200 ms, or resume_dwell 400 ms after a settle):
        go to Moving
state Moving:
    target speed   v* = clamp(k * d, -v_max, +v_max)      # positive scrolls down: content moves up
    speed-up limit |v| may grow by at most a_max per second; slowing down is immediate
    if |d| <= s:                     v = 0, go to Holding     # settled: the text is on the line
    if the gaze leaves the pane:     v = 0, go to Holding     # a glance elsewhere is not a command
    if the eyes are lost:            v = 0, go to Holding     # a blink stops nothing permanently
```

Speed is proportional to the distance from the set point, as asked, with three guards: the start band and dwell keep a glance or a jitter from starting motion, the gap between the start band and the stop band (hysteresis) keeps the boundary from chattering, and the speed-up limit keeps a sudden look at the bottom of the pane from jerking the page. The stop is a step from about `k * s` (around 70 px/s with the defaults below) to zero, which reads as the page settling, not braking.

Defaults, each `auto` from the G0 measurements of this PC:

| Parameter | Default | Why |
|---|---|---|
| Stop band half-height `s` | the larger of 3 x `precision_px` and 48 px | Noise of a still gaze stays inside it; 48 px is about two text lines at the owner's 150 percent scale [to confirm] |
| Start band half-height `b` | 1.6 x `s` | Hysteresis |
| Gain `k` | 0.3 / tau, at most 2.5 per second | The stability bound of 7.4, with margin; tau is the measured loop delay |
| Maximum speed `v_max` | 1,500 px/s | A ceiling for fast loops, rarely reached: with `k` = 1.5 per second, a look 600 px below the line, near the bottom of a 1,050 px pane, gives 900 px/s |
| Speed-up limit `a_max` | 3,000 px/s per second | Reaches 900 px/s in 0.3 s |
| Anchor | the gaze height at the start command, clamped to the middle half of the pane | People read in a preferred part of the screen that varies by person (7.10), so the user sets it by looking |

If 3 x `precision_px` would make the stop band taller than 15 percent of the pane, the tracker is too noisy for this pane, and Eileen says so and offers "scroll down" instead.

### 7.4 Why it stops where the reader wants, and how fast it may go

Let `x` be the wanted text's height relative to the anchor. Scrolling at speed `v` moves it by `dx/dt = -v`. The reader's eyes are on the text, so the gaze reports `x` late by the loop delay `tau`: the tracker's latency, the smoothing, the event period, the dispatch, the app's repaint, and the eye's own lag in following moving text. Outside the stop band the law gives

```text
dx/dt = -k * x(t - tau)
```

the classic first-order loop with a delay. Its behaviour is known: with `k * tau` at most 1/e (about 0.37) the text approaches the line without overshooting it; between 1/e and pi/2 it overshoots and oscillates while settling; above pi/2 it oscillates more and more. So the gain is not a matter of taste: it is capped by the measured delay, and the default `k = 0.3 / tau` stays under the no-overshoot bound with margin. Because the speed near the band's edge is small (about `k * s`), the page travels only about one delay's worth of that speed after the text reaches the band and before the eyes report it there: about 20 px with the defaults, well inside the band.

Planning figures for tau, all [to confirm] in G0:

| Part of the loop | Planning figure |
|---|---|
| The tracker, from the eye to the API | 20 to 50 ms |
| Smoothing in the sidecar | about 30 ms |
| The event period at ten a second | 50 ms on average |
| The core step, the dispatch and the desktop server | under 10 ms |
| The glide's next frame and the app's repaint | 20 to 40 ms |
| The eye catching up with text that moves | the rest of what G0 measures |
| Total tau | about 150 to 250 ms, so `k` about 1.2 to 2.0 per second |

A worked example, from a step-by-step simulation of this loop with the speed-up limit and a pure delay standing in for the eye. Tau is 200 ms, so `k` is 1.5 per second. The wanted paragraph sits 600 px below the line. The page starts toward 900 px/s, the offset then shrinks roughly as `600 * exp(-1.5 t)`, and the eyes report the paragraph inside the 48 px band about 1.5 s after the start, with the paragraph about 29 px below the line and no overshoot. With a 120 ms loop (`k` = 2.5) the same move takes about 1.0 s. With `k * tau` at 1.0, far over the bound, the same move overshoots the line by about 150 px, past the start band on the other side, so the page would reverse. That is the cost of latency, and why section 9 budgets it.

Two consequences of the closed loop matter for the design:

- Accuracy matters less than precision. The anchor is set by the user's own gaze through the same tracker, so a constant calibration offset moves the anchor and the gaze together and cancels. What remains is precision (the noise), which sets the band, and drift over a session, which the maximum session length and a re-anchor ("set the line here") cover.
- A PC whose real delay is longer than measured would hunt around the line. The session watches for it: two overshoot reversals (motion stops, then starts the other way within 600 ms) halve `k` for the rest of the session, and the event is logged with the session's numbers so the next G0-style measurement can correct tau.

### 7.5 Starting, pausing and ending

| Event | Effect |
|---|---|
| "Scroll with my eyes", "eye scroll", "follow my eyes" (addressed) | Opens a session on the pane under the onset fixation (`CoreCommand::Gaze(ScrollMode(Start))`) |
| The eye-scroll hotkey held (`hotkeys.gaze_scroll`, unset by default) | Opens a session while held, closes it on release; motion only while held. A physical key press, never a synthetic one |
| "Set the line here" | Re-anchors to the current gaze height |
| "Slower" / "faster" | Scales `k` by 0.75 or 1.25 for the session, never above the cap of 7.4 |
| The gaze leaves the pane, or the eyes are lost | Motion stops at once; the session pauses |
| Paused for `away_end_ms` (3 s) | The session ends |
| No motion for `idle_end_s` (60 s) | The session ends |
| `max_session_s` (600 s) | The session ends, with one line: "say scroll with my eyes to keep going" |
| "Stop", "that's good", "done", any stop word | The session ends |
| Any physical mouse or keyboard input, other than the held eye-scroll key | The desktop server stops the glide within one event and the session ends; the user's hands win |
| The pane's window closes, minimizes, or becomes private | The session ends |
| A job asks for the screen | An open session counts as the user being active, so the job asks by voice and on the HUD instead of taking the screen after the idle countdown of 2.6; if the user agrees, the session ends first |
| `gaze.scroll.end_on_settle = true` (a trial arm) | The first settle ends the session: a one-shot "find" instead of read-along |

### 7.6 Picking the pane and moving it smoothly

The pane. At the start command the core asks `desktop_hit_test { point, radius_px, want: ScrollContainer }` at the onset fixation. The answer is the nearest vertically scrollable ancestor with its rect and route. An Eileen window, a private window, a pane with no route, or a pane already at both ends refuses with one line ("I can't scroll that").

The glide. A step of `desktop_scroll` per event would move in jumps ten times a second. Instead the existing tool gains an optional `glide: Glide { vy_px_s, ms }`: "move this pane at this speed for at most this long". The core renews it at each track event (ten a second) with `ms` at most 150, and the desktop server animates it at the display's rate on its own thread. Three properties follow:

- A dead man's switch. If renewals stop (the core, the sidecar or the tracker fails), the glide ends by itself within 150 ms. Nothing keeps scrolling on a stale command.
- Stops without a round trip. The desktop server already tracks physical input through Raw Input (2.6); on any physical input, or when the pane's window stops being visible, it ends the glide itself and says so in the next result.
- One journal entry. The calls of one session share the session's `Act` job; the dispatcher journals the session once, with the net distance, and "undo that" scrolls back by it (by scroll position where the pattern reports one).

The route, in order:

1. UIA's Scroll pattern on the container, in the background: no cursor movement, no foreground needed. The server converts pixels to the pattern's percent with the container's height and the visible share it reports (`px per percent = height * (100 / view_pct - 1) / 100`), and reads the position back, so each result carries `moved_px`. Whether apps repaint smoothly at 30 to 60 calls a second through the pattern is a G0 probe [to confirm].
2. Wheel messages to the pane at the point, in fractional deltas where the app scrolls smoothly on them, through the route `desktop_scroll` already uses. If that route can only reach the pane by moving the cursor, the scroll mode refuses the pane rather than move the user's cursor [to confirm with WP5].

Gain matching. An app may move more or fewer pixels than asked. When results carry `moved_px`, the core keeps the ratio of moved to asked over the session (clamped between 0.5 and 2) and divides it out, so the loop's real gain stays under the bound of 7.4. Without readback, the ratio is 1 and the reversal guard of 7.4 is the backstop. At the end of the content a result reports `at_end`, the session holds, and the HUD chip says "end of page".

Visible intent. The 400 ms outline of 2.6 runs once, on the pane, before the first glide; the band then stays on screen for the whole session. A per-call wait would make continuous motion impossible, so later glides in the same session do not wait.

### 7.7 What keeps it safe

- Opened only by the user: an addressed turn or the physical hotkey. No gaze input, no brain turn, no worker and no skill can open a session; the brain cannot call `ScrollMode`.
- One pane, one tool: inside a session, gaze inputs can cause only `desktop_scroll` glides on the pane the session opened on. No click, no key, no focus change, no window action.
- Tier 1, as scrolling is today. Scrolling can have side effects in some apps (a chat app may mark messages as read), exactly as a wheel turn does; the scroll mode adds none.
- Bounded: at most ten glide calls a second, each with a lifetime of at most 150 ms, sessions of at most 10 minutes.
- The user's hands win: any physical input ends the session within one event, in the desktop server itself.
- Visible: the indicator, the band and the chip are on screen whenever a session is open.
- Simulator invariant 17 part (b) checks all of it (section 10).

### 7.8 Settings, contracts and HUD state for the mode

```toml
[gaze.scroll]                 # experimental; Low unless noted
enabled = false               # the mode exists only when on
trigger = "voice"             # voice | hold_key | both
anchor = "gaze"               # gaze (clamped to the middle half of the pane) | center
end_on_settle = false         # true: the first settle ends the session (trial arm)
stop_band_px = "auto"         # half-height; auto = max(3 x precision_px, 48)
start_band_px = "auto"        # half-height; auto = 1.6 x stop band
gain_per_s = "auto"           # auto = 0.3 / measured tau, at most 2.5; a manual value is capped the same way
max_speed_px_s = 1500
accel_px_s2 = 3000            # speed-up only
start_dwell_ms = 200
resume_dwell_ms = 400
away_end_ms = 3000
idle_end_s = 60
max_session_s = 600
show_band = true              # the tint over the stop band; the anchor line always shows
```

The contract additions are in section 8: `GazeEvent::Track`, `GazeAction::Track`, `GazeCommand::ScrollMode`, `Glide` on `DesktopScroll`, `HitWant::ScrollContainer` with `ScrollTarget`, `HudState.gaze_scroll`, and the scenario's `GazeFollow` step with a simulated page.

### 7.9 How to tell whether it works: the trials

On the reading fixture of section 10 and two real apps (a long article in Edge and a long file in VS Code), with tasks of the form "bring paragraph N to your reading line" from random starting positions, twenty per arm, the owner runs:

| Arm | What it is |
|---|---|
| A | The law of 7.3, started by voice, read-along on |
| B | The same, with the hold key |
| C | A steeper curve: speed grows with the 1.5th power of the offset, capped by the same bound at its steepest point |
| D | One-shot: `end_on_settle = true` |
| E | Baselines: the mouse wheel, and "scroll down" by voice (G4) |

Measured: time until the target sits in the band; overshoot reversals; false starts, counted over two minutes of plain reading with a session open; ends by stop word against ends by touch; and a one-to-five comfort rating. Proposed bar to drop "experimental" [to confirm with the owner]: no reversal in 95 percent of trials, fewer than one false start per two minutes of reading, a median under 3 s for a target one pane away, and the owner's yes. The results go in `docs/wp/WP11-gaze.md` and B11.

### 7.10 Prior art, and what is new here

- Kumar, Winograd and Paepcke at Stanford proposed [gaze-enhanced scrolling techniques](https://hci.stanford.edu/publications/paper.php?id=75) in 2007, manual and automatic, with gaze as the main or an added input. As a later study describes them, they used fixed screen regions that scroll when looked at, turned automatic scrolling on and off with a dedicated key, and some assumed people read in the central third of the view.
- A [2015 study presented at the PETMEI workshop](https://microsoft.com/en-us/research/wp-content/uploads/2016/07/PETMEI2015scrolling_petmei_15_final.pdf) watched 18 people scroll by hand. It found they read in a preferred region that varied by person (about half of scroll events came while looking at the middle third), and that they scrolled in chunks of a few lines (a median of about 3.5 lines while reading) rather than smoothly. It also summarises Sharmin and colleagues' auto-scrolling, which starts when the gaze crosses the lower edge of the user's usual reading range and speeds up beyond it.
- What that means here: the anchor is set by the user's own gaze, not fixed at the centre; the stop band is a few lines tall; and the trials include a one-shot arm and a steeper arm, so the chunked habit is tested rather than assumed away.
- What is new: speed proportional to the offset from a user-set point, with the gain capped by the loop delay measured on this PC; the stop as the closed loop's own settling point, with hysteresis; and the safety envelope of 7.7 (a spoken or held-key start, one pane, a server-side dead man's switch, hands that always win).

## 8. Contracts

Sketches in the style of 3.3; the amendment writes them in full. New module `gaze`, provisional, `pinned_by = "WP11"`. Every string from the screen is `Untrusted`; every other plain string is listed in `text-allowlist.toml` with its reason.

```rust
// eileen_contracts::gaze (new; provisional, pinned by WP11)
pub struct GazeSourceInfo { pub kind: GazeSourceKind, pub accuracy_px: u16, pub precision_px: u16, pub rate_hz: u16,
    pub monitors: Vec<u8>, pub stores_nothing: bool, pub checked: Option<WallTime> }   // kind: WindowsGazeInput | Vendor { name }
pub enum GazeEvent {
    Ready(GazeSourceInfo),
    Fixation(Fixation),                   // Started, then at most ten Updated a second, then Ended
    Track(Track),                         // only while the core asked for it (a scroll session); shares the ten a second
    Lost { reason: LostReason },          // NoEyes | OffScreen | DeviceGone | Paused
    Found,
    Check(Option<GazeCheck>),             // after "check my eyes"; None after "forget my gaze check"
    Health { rate_hz: u16, latency_ms: u16, cpu_pct: u8 },
}
pub struct Fixation { pub id: u64, pub phase: FixationPhase, pub point: Point, pub monitor: Option<u8>, pub age_ms: u32,
    pub dispersion_px: u16, pub confidence_pct: u8 }                   // point: physical virtual-screen pixels, like every Point
pub struct Track { pub point: Option<Point>, pub age_ms: u16, pub confidence_pct: u8 }   // smoothed; None while the eyes are lost
pub struct GazeCheck { pub points: u8, pub accuracy_px: u16, pub precision_px: u16, pub offset: Option<(i16, i16)>,
    pub monitor_device_path: String, pub at: WallTime }
/// What an addressed turn captured at speech onset and at its end (section 6.4).
pub struct GazeRef { pub point: Point, pub fixation_ms: u32, pub accuracy_px: u16, pub age_ms: u32,
    pub on_eileen: bool, pub on_screen: bool, pub through: Option<EnhanceMap> }
pub struct EnhanceMap { pub hwnd: i64, pub source: Rect, pub destination: Rect }
/// desktop_hit_test's output.
pub struct GazeHit { pub window: Option<WindowRef>, pub element: Option<ElementInfo>, pub section: Option<Section>,
    pub scroll: Option<ScrollTarget>, pub candidates: Vec<ElementInfo>, pub private: bool }   // candidates: nearest first
pub struct Section { pub rect: Rect, pub label: Untrusted<String>, pub control_type: String, pub selector: Selector, pub depth: u8 }
pub struct ScrollTarget { pub rect: Rect, pub element: Option<u32>, pub route: ScrollRoute, pub view_pct: Option<u16>,
    pub at_top: bool, pub at_bottom: bool }                           // route: Pattern | Wheel

// connectors (provisional, WP2): SidecarId gains Gaze; ConnectorEvent gains Gaze(GazeEvent)
// tools/desktop (provisional, WP5):
pub struct DesktopHitTest { pub point: Point, pub radius_px: u16, pub want: HitWant }   // Window | Element | Section | ScrollContainer. Tier 0
pub struct DesktopScroll { pub hwnd: i64, pub x: i32, pub y: i32, pub dx: i32, pub dy: i32, pub glide: Option<Glide> }   // tier 1
pub struct Glide { pub vy_px_s: i32, pub ms: u16 }   // ms at most 150; dx and dy must be 0; output gains moved_px: Option<i32>, at_end: bool
// tools/brain (provisional, WP2): LookTarget gains Gaze and Region { rect: Rect }
// pc.gaze's own tool, core-only except Status:
pub struct GazeControl { pub action: GazeAction }
    // Status (tier 0, the brain may call it) | Pause | Resume | Calibrate | Check { monitor: u8 } | Forget | Track { on: bool }
// intent (provisional, WP7):
pub struct ResolveCtx { /* ... */ pub gaze: Option<GazeCtx> }
pub struct GazeCtx { pub usable: bool, pub accuracy_px: u16, pub on_eileen: bool, pub enhanced: bool }
pub enum CoreCommand { /* ... */ Gaze(GazeCommand), GazeControl(GazeAction) }
pub enum GazeCommand {
    Enhance { step: EnhanceStep },        // In | Out | Off
    Describe { how: DescribeHow },        // WhatIsThis | Read | Summarise | Translate { to: String }
    Click { count: u8 },                  // click that, press that, tick that; open that is 2
    Scroll { dx: i32, dy: i32 },          // one step at the eyes (G4)
    FocusField,                           // type here, put it here, the dictation key with a gazed field
    Window(WindowAction),                 // close this, snap this left, move this there (Snap { monitor } from the End value)
    Which,                                // "that one" while a question is open
    Where,                                // "where was I?" (G12)
    ScrollMode(ScrollModeAction),         // Start | Stop | Reanchor | Faster | Slower (section 7); never from the brain
}
// The Gaze slot's value, substituted by the core: {"gaze": "onset"} | {"gaze": "end"}
// desktop (provisional, WP5): ReferenceKind gains Gaze
// hud (provisional, WP8):
pub struct HudState { /* ... */ pub gaze: Option<HudGaze>, pub gaze_scroll: Option<HudGazeScroll> }
pub struct HudGaze { pub point: Point, pub accuracy_px: u16, pub lost: bool }
pub struct HudGazeScroll { pub pane: Rect, pub anchor_y: i32, pub stop_band_px: u16, pub start_band_px: u16,
    pub state: GazeScrollState }          // Holding | Moving { down: bool } | Paused | AtEnd
pub enum HudPanel { /* ... */ Enhance { hwnd: i64, source: Rect, label: Untrusted<String>, level: u8 } }
pub struct Indicators { /* ... */ pub gaze_live: bool }
pub enum HudInput { /* ... */ GazeOver { item: HudItem } }   // physical is always false; references only, never an answer or approval
pub enum HudItem { Job(JobId), Approval(ApprovalId), Option { id: u64, option: u8 }, Strip, Nothing }
// config (provisional, WP7): Config gains gaze: GazeConfig with scroll: GazeScrollConfig (sections 6.9 and 7.8);
//     hotkeys gains gaze_scroll: Option<String>
// scenario (provisional, WP2): Step gains Gaze { point: Point, hold_ms: u32 }, GazeFollow { target_y: i32, delay_ms: u16,
//     noise_px: u16 } (a model eye that follows a line of the simulated page) and Lost {}; SimWorld gains
//     gaze: Option<SimGaze { accuracy_px: u16, precision_px: u16 }> and page: Option<SimPage { pane: Rect, content_px: u32 }>
// learn (provisional, WP7): the ledger gains a Dwell row { window, document, seconds }, behind gaze.ledger
```

Wire: `GazeEvent` travels as `Input::Sidecar(SidecarId::Gaze, SidecarEvent::Event(ConnectorEvent::Gaze(event)))`; nothing new at the top level. Gaze events are not bus events: they are not "what happened" for listeners, and they would flood the plugin and the `events` table. So `BusEvent` gains nothing, the HUD learns the reticle and the band from `HudState`, and the ledger learns dwell from `LearnCmd`.

## 9. Budgets

| Stage | Budget | Where measured |
|---|---|---|
| Sample to `Fixation { Started }` | `fixation_ms` plus at most 50 ms | unit test on recorded samples; the owner's PC |
| Gaze events into the core | at most 10 a second, fixations and track together | property test |
| Idle cost of the sidecar | under 1 percent of one core, under 40 MB [to confirm]; the vendor's service measured beside it | `cargo xtask measure-idle`; the G0 report |
| `desktop_hit_test` | p95 under 60 ms from the cache for the window, under 150 ms with `ElementFromPoint`; deadline 300 ms | the desktop lane |
| "Enhance", end of speech to the view visible | under 500 ms (the fast-path budget of 4.3) | timed scenario |
| A brain turn with a gaze target | plus one hit test, in parallel with the earcon: first word still within the 1.5 s target on a quiet PC | B10's cases with gaze |
| Reticle and scroll band | 10 updates a second through `HudState`; no extra HUD cost beyond the snapshot it already draws | `measure-idle` |
| Accuracy needed | section 60 px, element 25 px, cell 12 px (about 1, 0.4 and 0.2 degrees on the owner's display); the check's `accuracy_px` decides which are offered | the nine-point check |
| Scroll loop delay tau, gaze sample to the page moving | under 200 ms, aim 150 ms; it sets the gain (7.4) | trace spans, plus the G0 step test |
| Scroll stop, eyes enter the band to the page still | under 150 ms | desktop lane, closed-loop injector |
| Scroll stop on physical input | within one input event, in the desktop server | desktop lane with `test-physical` |
| A glide with no renewal | ends within its `ms`, at most 150 ms | unit test on the server's glide timer |
| Scroll settling, target one pane away | under 3 s median (the trial bar of 7.9) | the owner's trials |

## 10. Testing

- Unit and property. The dispersion filter on synthetic streams (every fixation at least `fixation_ms`, centroid inside the dispersion window, saccades emit nothing); the track filter (lag and jitter on recorded pursuit); the onset rule over scripted rings; the back-mapping through `EnhanceMap` round-trips; the rate limiter never passes more than ten events a second; the section and scroll-container rules over fixture trees.
- The scroll law. Table tests: zero inside the stop band, the sign of the speed follows the sign of the offset, the speed never falls as the offset grows, the cap holds, no start before the dwell, no chatter on a noisy stream at the band's edge. A closed-loop model test, pure and seeded (randomness passed in): a simulated page and a model eye that follows a target line with delay tau and noise; for `k * tau` at most 0.3 every run settles with the target inside the stop band, with no reversal, within `tau + ln(d0 / s) / k` plus the speed-up time; a deliberately unsafe `k * tau` of 1.0 shows reversals and the reversal guard halving the gain, which is why the cap exists.
- Fuzz. `gaze_samples`: the recorded-sample file format and both filters; clean for ten minutes in `long.yml`. Every new parser of untrusted input gets a target (6.1), and a vendor stream parser, if one is written, gets its own.
- Snapshots. The `gaze` contracts module and the desktop toolset's input schemas; HUD draw lists for the reticle, the enhance panel at each level and corner, the scroll band in each state, and the indicator.
- Simulator invariant 17. (a) Over any input stream that holds gaze events and no `UserInput` and no addressed turn, the core emits no `ApprovalResolved`, no `JobAccepted`, no `ToolForward`, no `Persist(SettingsVersion)`, no `Speech(Speak)`; only `Hud` snapshots, `Learn` dwell rows when `gaze.ledger` is on, and timers. (b) A scroll session opens only from an addressed turn or a physical hotkey; while one is open, the only tool calls gaze inputs cause are `desktop_scroll` glides on the session's pane, at most ten a second, each with `ms` at most 150; after a stop word, a physical input, `away_end_ms`, `idle_end_s` or `max_session_s`, no further glide is issued. Checked on the PR seeds and the regressions.
- Resolver golden file. The `Gaze` slot cases of 6.5, including "close Chrome" ignoring gaze, "move this there" binding onset and end, every fallback, and "scroll with my eyes" never resolving from a brain turn.
- Scenarios in `scenarios/gaze/`, replayed in simulation with scripted gaze steps and run in the desktop lane with the `test-gaze` injector: `enhance-section`, `enhance-cell-through-view`, `enhance-minimized-refused`, `enhance-private-refused`, `this-window-close`, `scroll-under-gaze`, `click-that-send-asks` (a Send-named control stays tier 2), `click-eileen-window-refused`, `gaze-lost-falls-back`, `that-one-contact-sheet`, `move-this-there` (`monitors>=2`, the fleet), `addressee-looking-at-hud`, `never-approves` (gaze on Allow plus silence: nothing; gaze on Allow plus "click that": refused). For the scroll mode: `scroll-mode-settles-on-target`, `scroll-mode-look-up-scrolls-up`, `scroll-mode-stop-word-ends`, `scroll-mode-physical-input-ends`, `scroll-mode-eyes-away-pauses-then-ends`, `scroll-mode-end-of-page-holds`, `scroll-mode-eileen-window-refused`, `scroll-mode-private-refused`, `scroll-mode-never-starts-from-gaze` (looking at the bottom of a page for a minute with no command scrolls nothing), `scroll-mode-job-asks-for-screen`.
- The fixture pages, loaded in the isolated Edge profile WP5 already uses. `fixtures/dashboard/index.html`: three cards laid out as the owner's screenshots (a line chart, the "Activity by time" heatmap with `role="grid"` and each cell named like "Thursday 4 pm, 93", recent transactions), with `section` landmarks and `aria-label`s so the section rule and the cell names are deterministic. `fixtures/reading/index.html`: a long article of numbered, labelled paragraphs in one scroll container, so a test can read each paragraph's rect.
- The closed-loop injector. For the scroll scenarios in the desktop lane, the `test-gaze` injector acts as a model eye: every 30 ms it reads the target paragraph's rect over UIA and injects its centre, delayed and with noise. The scenario then checks the paragraph ends inside the band, with no reversal, and that no glide follows the session's end.
- The owner's measurements, recorded in `docs/wp/WP11-gaze.md`: the check's accuracy and precision per route; thirty "enhance" trials on the fixture and on three real apps with the share that picked the intended section; thirty "click that" trials with the share that outlined the intended element on the first try; the scroll trials of 7.9; idle cost of the sidecar and of the vendor's service; the hit test's p95 on Edge, Notepad and Resolve.
- A benchmark, B11, beside B1 to B10: the enhance, click and scroll trials as timed A/B scenarios, on the `devhub` results page.

## 11. Phases and acceptance

1. G0, the spike (compiles nothing on the owner's PC; a research report, section 17). Acceptance: `docs/research/2026-11-gaze.md` records, for each route, whether it delivers samples to a Win32 process, the check's accuracy and precision on the owner's display, the loop delay tau, idle cost (Eileen's and the vendor's), the scroll-route probe, and the licence findings with quotes and dates; the owner picks the route.
2. G1, the contracts and the neutral crate (pull requests 3 to 6 of Part A). Acceptance: the `gaze` module snapshot; `eileen-gaze` passes its filter, limiter and fuzz tests on Linux; invariant 17 part (a) holds on the PR seeds; the resolver golden file has every case of 6.5; the reticle and indicator draw-list snapshots exist; `gaze.enabled` is a Safety key that needs a `UserInput` approval and the registry refuses to start `pc.gaze` without it.
3. G2, enhance (pull requests 7 to 10). Acceptance: on the owner's PC with the chosen route, `enhance-section` passes thirty times out of thirty on the fixture page within the 500 ms budget; `enhance-cell-through-view` names the Thursday 4 pm cell; a minimized, cloaked, private or Eileen window is refused with one sentence; "back", Esc and a physical click close the view and the source window never gained focus or input (checked by the fixture's own state); the view never appears in `desktop_screenshot`; the `test-gaze` injector drives the same scenarios on the hosted lane with no tracker.
4. G3, the commands. Acceptance: G2 to G14 scenarios pass; a gazed Send button still asks; "click that" on the HUD is refused; the addressee feature's scenario shows at least nine of ten unnamed requests to the screen passing with a look at the HUD and no change to the room-talk result of M3; the accessibility test's gaze column is complete; the ledger's dwell rows exist only with `gaze.ledger` on and never for a source that `stores_nothing`.
5. G4, scrolling by gaze, experimental (pull requests 11 to 13; after G2's hit test, beside G3). Acceptance: the law's table and closed-loop tests pass; invariant 17 part (b) holds on the PR seeds; every `scroll-mode-` scenario passes in simulation and, with the closed-loop injector, in the desktop lane; physical input ends a session within one event and a glide with no renewal ends within 150 ms; the owner has run the trials of 7.9 and recorded them. Dropping "experimental" is a separate owner decision on those results.

## 12. Proposed work package: WP11, Gaze

For BUILD.md as section 6.13, in the form of 6.2 to 6.12.

- Branch `wp11/<topic>` for each pull request.
- Depends on WP0 phase A (the traits), WP1 (the core and the simulator), WP2 (the dispatcher's origin table and the sidecar client), WP5 (`desktop_hit_test` and `Glide` land in its crate), WP7 (the `Gaze` slot in the resolver, the `[gaze]` settings), WP8 (the panel, the reticle, the band, `GazeOver`). G0 depends on nothing.
- Desktop yes: the owner's PC with the chosen tracker for G2 and later; the hosted lane through the `test-gaze` injector.
- Wave after W5, as v0.2's first package; G0 at any time, since it compiles nothing. One slot; its builds are small.
- Owns `crates/eileen-gaze`, `crates/eileen-gaze-win`, the `gaze` contracts module, `scenarios/gaze/`, `fixtures/dashboard/`, `fixtures/reading/`, `fuzz` target `gaze_samples` with its corpus, `docs/GAZE.md`, `docs/wp/WP11-gaze.md`, `docs/research/2026-11-gaze.md`.
- Does not own the platform traits, the pinned contracts, `eileen-desktop` (WP5 reviews the hit-test and glide pull requests), `eileen-hud` (WP8 reviews the panel and the band), `eileen-intent` and `eileen-core` (WP7 and WP1 review their pull requests). A change to any of them goes in a pull request that package reviews, or an amendment.
- Builds sections 6, 7 and 8 of `docs/GAZE.md`: the sidecar and connector, the routes behind `GazeSource`, the fixation and track filters and the rate limiter, the nine-point check, the core's gaze ring and onset rule, the `Gaze` slot, `desktop_hit_test` and its rules, the enhance view and the reticle, the grammar entries of section 5, the `[desktop]` gaze line and `LookTarget::Gaze`, the `[gaze]` settings, invariant 17, the experimental scroll mode with its law, glide and band, the scenarios, the fixture pages, B11.
- Acceptance the phase lists of section 11.

## 13. Proposed decision record

For `docs/DECISIONS.md`, the next free number (D81 at the time of writing).

- Date: 2026-10-26
- Status: proposed (not yet confirmed by the owner)
- Amends: none. Extends D21 (the desktop server gains a hit test and a glide), D38 (one more platform trait), and the room-sensing connector rule of BUILD.md 2.14.
- Decision: Eye tracking enters Eileen through a hardware tracker, as a slot, never a trigger. A room-sensing connector, `pc.gaze`, served by a supervised sidecar behind the `GazeSource` platform trait, turns the tracker's samples into fixations and sends only those to the core, at most ten a second; Eileen never receives an image of the user. The core keeps a ring of recent fixations, snapshots the one current at speech onset for each addressed turn, and resolves the demonstratives of a spoken command ("this", "that", "here", "there") through a `Gaze` slot and one hit test in the desktop server. A gaze-resolved action is dispatched like any other, with its tool's tier, taint and journal; the "enhance" view is a clipped DWM thumbnail the HUD draws, touching no app. No approval, mute change, job, setting, consent or first-run choice is ever built from a gaze event, which simulator invariant 17 checks. An experimental scroll mode, off by default, is the one continuous use: opened only by an addressed turn or a held physical key, it scrolls only the pane it opened on, at a speed proportional to the gaze's distance from a set point with the gain capped by the loop delay measured on the PC, stops when the gaze returns to a band around the point, and ends at a stop word, any physical input or when the eyes leave the pane. The connector is opt-in through a Safety key, local only, visible on the HUD and the tray whenever the tracker is live, and a source whose terms allow live use only keeps every stored use (the dwell ledger) off.
- Why: The owner's "enhance": look at a part of a dashboard, say the word, and it fills the screen, with the eyes still tracked inside the view. The same slot removes the mouse from the commands that only needed a target, and the scroll mode removes it from reading, which serves the accessibility goal of OWNER-DIRECTION.md. Deterministic first keeps it on the fast path; "a slot, never a trigger" keeps the approval invariant intact and avoids the Midas touch problem; a hardware tracker gives the accuracy the use cases need without a model, a camera stream or a memory budget the owner's PC cannot spare.
- Consequences: WP11 (BUILD.md 6.13) builds it after WP5 and WP8, with G0's measurement spike first; the contracts gain a provisional `gaze` module and small additions to the connectors, hud, intent, config, scenario and desktop tools modules, none at the top level; `eileen-desktop` gains `desktop_hit_test` and `Glide`; the dispatcher's origin table lets the fast path issue `desktop_click_element` and `desktop_scroll` as `Act` jobs; the machine profile treats the sidecar as an optional resident; ROADMAP's v0.2 table gains a row; the webcam route stays out until a later proposal.
- Source: `docs/GAZE.md`; `docs/research/2026-11-gaze.md` once G0 has run; BUILD.md 2.6, 2.8, 2.9, 2.12, 2.14, 3.3.9, 3.3.10, 3.4; OWNER-DIRECTION.md, "Accessibility as a first-class goal".

## 14. Amendments this needs

One `amendment` pull request (Part A, order 3), with each change at the place named:

| File | Section | Change |
|---|---|---|
| BUILD.md | 1 | E20, "Enhance": the table of section 4 in E-table form, with one line on the scroll mode |
| BUILD.md | 2.1 | The `eileen-gaze.exe` row of 6.1 in the processes table and the diagram |
| BUILD.md | 2.4 | The grammar entries of section 5 under built-in grammar; the `Gaze` slot resolver beside the others |
| BUILD.md | 2.6 | `desktop_hit_test` as the nineteenth tool; the section and scroll-container rules; `Glide` on `desktop_scroll` with its server-side stops; visible intent once per scroll session; an open session counts as the user being active for the lease rule |
| BUILD.md | 2.8 | Rule 5 gains the `gaze_on_eileen` feature (G10), with "looking away never subtracts" |
| BUILD.md | 2.9 | The reticle, the enhance panel, the scroll band, `GazeOver`, the indicator, the accessibility table's gaze column |
| BUILD.md | 2.11 | `gaze.*` in the Safety prefix list for the keys marked Safety; `hotkeys.gaze_scroll` |
| BUILD.md | 2.12 | A row in the observed-read table for `desktop_hit_test` (ByTarget) and `desktop_look` with `Gaze` or `Region` (Screen); "what gaze never does" under approvals; the origin table's `FastPath` row |
| BUILD.md | 2.14 | `pc.gaze` as a room-sensing connector; `stores_nothing` |
| BUILD.md | 2.15 | `gaze\check.toml` in the data directory |
| BUILD.md | 2.16 | The gaze sidecar as an optional resident |
| BUILD.md | 3.1 | `eileen-gaze`, `eileen-gaze-win` in the crate list |
| BUILD.md | 3.3 | A new 3.3.18 with section 8's types; the additions to 3.3.5, 3.3.6, 3.3.9, 3.3.10, 3.3.11, 3.3.12, 3.3.14 |
| BUILD.md | 3.4 | `GazeSource`; `Automation::ancestors`; `HudSurface::set_enhance` and the clip |
| BUILD.md | 4.3 | The rows of section 9 |
| BUILD.md | 4.7 | Invariant 17 in both parts; the gaze scenario steps and the simulated page; B11 |
| BUILD.md | 6.13 | Section 12 |
| BUILD.md | 7 | The risks of section 15 under open risks |
| BUILD.md | 8 | A revision-log line |
| DECISIONS.md | end | D81 from section 13 |
| PLAYBOOK.md | as needed | What D81 changes, or the reason it changes nothing, under the pull request's "Playbook" heading |
| OWNER-DIRECTION.md | Proposed 2026-10-26 | The proposal in one entry, with the owner's answers to section 16 |
| ROADMAP.md | v0.2 | "Gaze: a hardware eye tracker as a slot for 'this', 'that' and 'enhance', and an experimental scroll by gaze; WP11" |
| ARCHITECTURE.md | Processes, Code map | The sidecar row; two crate rows; a sentence under "Connectors" |

## 15. Risks

- No tracker on the owner's PC today. The spike needs one; the owner picks it (section 16, question 1). A tracker mounts on a monitor of 15 to 27 inches, and a laptop screen needs a mount [to confirm for the owner's laptop].
- Windows gaze input from a Win32 process is unverified. If the API needs a view or a package identity, the sidecar hosts a hidden core window or gets a sparse package, or the vendor route is first. G0, question 1.
- Vendor terms. Interaction against analytical use, redistribution of the runtime, and whether a desktop assistant is within the consumer runtime's scope are read from the current licence text, quoted in the report, and settle `stores_nothing` and the install route. The licence page found while writing this describes the older EyeX SDK. Until the current text is read, nothing analytical is designed on a vendor source.
- Chromium's accessibility cost. `ElementFromPoint` on web content needs the on-demand request WP5 measures; if it is too costly on this PC, the hit test falls back to `want: Window` for browsers and the section is the quarter around the point, which still enhances. The scroll mode then uses the wheel route on the window.
- DWM thumbnail limits. Minimized, cloaked and DRM-protected windows render nothing; some DirectComposition apps may render black [to confirm]. Enhance should say so in one sentence rather than show a black view; how to tell in advance is question 4 of section 17.
- The onset rule picks the wrong fixation sometimes. People glance at the HUD, at the keyboard or at the next thing before they finish speaking. The trials of section 10 measure it, and `onset_window_ms` and "the longest fixation before the glance" are the levers; the fallback is always one short question.
- The scroll loop hunts. If the real loop delay is longer than measured, the page overshoots and reverses. The gain is capped from the measured delay, the reversal guard halves it within a session, and the trials count reversals.
- Read-along surprises. Reading down a paragraph may start motion the reader did not want. The start band, the resume dwell and the one-shot arm of the trials address it; "stop" always ends it.
- Panes the mode cannot reach. Apps with no Scroll pattern that ignore wheel messages sent to a point are refused rather than driven by moving the cursor.
- Motion discomfort. Continuous scrolling can tire the eyes. The speed cap, the speed-up limit and the session limit bound it, and the comfort rating in the trials watches it.
- Privacy perception. A device watching the user's eyes worries people more than a microphone. The indicator that cannot be hidden, "stop watching my eyes", the no-image rule with its repository check, and the opt-in Safety key are the answer; the reticle shows the user what Eileen knows and nothing more.
- Multi-monitor and mixed DPI are untested on the owner's single monitor, as for the desktop server; the fleet covers them.

## 16. Open questions for the owner

1. Which tracker for the spike: a Tobii Eye Tracker 5, or another tracker that feeds Windows gaze input?
2. Placement: v0.2 as proposed, or G0 and G1 beside W5 so the contracts land before WP8 pins the HUD module?
3. The dwell ledger (G12): keep it in the design as an opt-in, or leave it out entirely?
4. The reticle: on for the first week after enabling, as proposed, or off until asked?
5. The word: "enhance" as the canonical entry, with "zoom in on that" and "make that bigger" as phrasings, or another word?
6. The scroll mode's start: by voice ("scroll with my eyes"), by holding a key, or both?
7. After the text settles: keep following as the user reads on (read-along), or end the session (one-shot)?

Per the standing rule of BUILD.md section 7, the recommendation applies unless the owner says otherwise: a Tobii Eye Tracker 5, v0.2, keep the ledger as an opt-in, reticle on for a week, "enhance", the scroll mode by voice with the hold key as an option, and read-along with the one-shot as a trial arm.

## 17. The G0 spike: what to confirm

A research report, `docs/research/2026-11-gaze.md`, in the form of the October reports, with dates and quotes. Scripts and fixtures under `docs/research/fixtures/gaze/`, never in a crate.

1. Windows gaze input. Whether `Windows.Devices.Input.Preview` delivers samples to a Win32 process on the owner's Windows 11 build with the chosen tracker, in which coordinates and at what rate; whether it needs a view, a package identity or only a watcher; what the consent prompt looks like.
2. The vendor runtime. The current licence text of the consumer runtime: interaction against analytical use, redistribution, scope; whether its DLL can be loaded at run time from the user's own install; whether its calibration can be opened from another app.
3. Accuracy, precision and drift. The nine-point check on the owner's display at the measured viewing distance, in daylight and lamp light, with and without glasses, on each route; repeated after ten minutes to see drift.
4. DWM thumbnails with a source clip on a layered, no-activate overlay: that `rcSource` clips as expected at 150 percent DPI, that covered windows render, and what minimized, cloaked, DRM and DirectComposition windows render, with screenshots in the report (of fixtures only).
5. The hit test's cost on Edge, Notepad, Explorer and Resolve: `WindowFromPoint`, `ElementFromPoint`, ancestors, with and without the Chromium accessibility request, in the numbers WP5's probe uses.
6. The onset rule, by hand: twenty "enhance" trials on the fixture page, logging the fixation at onset and the intended card, to set `onset_window_ms` before G2.
7. Degrees to pixels on the owner's display at the measured viewing distance, so the defaults of 6.3, 7.3 and 9 are this PC's numbers.
8. The scroll loop delay tau. The software part exactly, from trace spans (sample received, track sent, glide issued, glide applied); the tracker's latency from its own timestamps where the route gives them, else from a step test on a jumping target; the eye's lag following text that moves at 300, 600 and 900 px/s. The sum sets the default gain.
9. The scroll route. Which panes in Edge (web content and a PDF), Notepad, Explorer and VS Code expose a vertically scrollable Scroll pattern; how smoothly each repaints at 30 and 60 pattern calls a second; whether wheel messages can reach a pane at a point without moving the cursor; and how each app treats fractional wheel deltas.
10. Idle cost, Eileen's sidecar and the vendor's service separately, against the machine profile's usual free memory.

## 18. Set aside: the webcam route

The other way to get gaze is a webcam estimator: face and iris landmarks from an ONNX model and a per-monitor regression fitted at a nine-point calibration. It is not part of this plan, for four reasons: its accuracy of 2 to 4 degrees [to confirm] reaches sections but not elements and is too noisy for the scroll mode's band; the model and frames would cost 100 to 250 MB and several percent of a core on a PC with 1 to 2 GB free [to confirm]; the best open gaze models were trained on datasets whose licences often allow research only; and it would put a camera stream inside an Eileen process, where the no-image rule of this plan keeps none. The `GazeSource` trait leaves room for it, and a later proposal can bring it back with its own spike, its own model licence checks and its own memory budget.
