# Gaze: eye tracking for Eileen

A proposal with a build brief, written 2026-10-26; the webcam route (section 18) was added on 2026-10-27. It is for the owner, who decides what it changes in [BUILD.md](BUILD.md), and for the coding agent that builds it, the way [LINUX.md](LINUX.md) proposed WP10. Part A is the brief for the agent. Part B is the design. Statements marked [to confirm] are reasoning or vendor facts that the P0 spike (section 17) must verify before code depends on them. Sources are linked where they are used.

Scope. This plan reads gaze from a hardware eye tracker: a consumer device such as the Tobii Eye Tracker 5, read through the gaze input API Windows provides for such devices, or through the vendor's own runtime. It also has a webcam route (section 18), for a PC with no tracker and for Linux, where no consumer tracker has a supported runtime. The webcam serves the scroll mode only; the tracker stays the route for everything finer. Section 7 adds an experimental mode in which the eyes scroll a page from a set point.

In one sentence: an eye tracker tells Eileen where the user is looking, and a spoken word acts on it. "Enhance" fills the screen with the dashboard card under the user's eyes. "Close this", "scroll down here", "what's this?" and "that one" stop needing a mouse. Gaze is a slot, never a trigger: it only resolves "this" and "there", and it never starts, approves or changes anything on its own. The one exception is the experimental scroll mode. Only a spoken word or a hotkey opens it, it can do nothing but scroll one pane, and it stops when the reader's eyes come back to the set point.

Use cases are numbered G1 to G15 (section 5) and phases P0 to P4 (section 11).

---

## Part A. Brief for the coding agent

You are working in the Eileen repository, a Rust workspace with a sans-IO core (`Core::step`), supervised sidecars, one contracts crate whose JSON Schemas are snapshot-pinned, and a specification of record in `docs/BUILD.md`. Read these before touching anything, in this order:

1. `README.md`, `ARCHITECTURE.md` (the map and the invariants), `AGENTS.md` and `CONTRIBUTING.md` (the rules and the writing style).
2. `docs/BUILD.md` sections 2.1 (processes), 2.2 (one request end to end), 2.3 (the brain's `[desktop]` block), 2.4 (the fast path), 2.6 (the desktop server), 2.8 (the addressee gate), 2.9 (the HUD), 2.10 (the journal and what the librarian sees), 2.11 (settings), 2.12 (tiers and taint), 2.14 (connectors), 2.16 (the machine profile), 3.3.2, 3.3.3, 3.3.5, 3.3.6, 3.3.9 to 3.3.14 (the contracts this touches), 3.4 (platform traits), 4.3 to 4.5 (budgets), 4.7 (tests), 4b.3 (the checks), 6.1 (the build rules on the owner's PC).
3. The crate doc of `crates/eileen-contracts/src/lib.rs`, and `modules.toml` beside it, which says which contract modules are pinned and which are provisional.
4. `docs/LINUX.md` section 8 and `docs/wp/WP10-linux.md`: the precedent for proposing a package and the shape of a living plan.

### What you are building

Part B, in full. The short version: a new sidecar `eileen-gaze.exe` serving a room-sensing connector `pc.gaze` over a hardware tracker or, for the scroll mode, a webcam (section 18); fixation events into the core through the existing `Input::Sidecar` path; a `Gaze` slot in the intent resolver; one new desktop tool, `desktop_hit_test`; an "enhance" view drawn by the HUD from a clipped DWM thumbnail; the grammar entries of section 5; and, behind an experimental setting, the set-point scroll mode of section 7. No new top-level `Input`, `Effect` or `BusEvent` variant is needed. The contract modules this touches are provisional today, but by the time WP11 runs most will have been pinned by their packages, so those changes come as an amendment (pull request 4).

### Context you cannot see

The owner's reference for "enhance" is three screenshots of a dark finance dashboard ("Loud") in a browser: a line chart card, an "Activity by time" heatmap card (weekdays across, 1 pm to 6 pm down), and a recent-transactions card. In the first, a cyan "Gaze point" marker sits on the heatmap's Thursday 4 pm cell while the whole page shows. In the second, after "enhance", the heatmap card alone fills the screen, cells three times larger, with a back arrow top right and the gaze marker still on the Thursday 4 pm cell. That is the three-step system: look, say "enhance", and the section under your eyes fills the screen. The dashboard fixture of section 10 reproduces that layout.

### Pull requests, in order

Every pull request is small (logic changes under about 500 lines), a draft from the first push, with the Evidence section first in its description and the `type:` line set (BUILD.md section 4b.3; `.github/PULL_REQUEST_TEMPLATE.md`). `all-green` and `pr-meta` must pass. A change to another package's merged files, to the root manifest's feature lists, or to `docs/BUILD.md`, `docs/DECISIONS.md` or `docs/OWNER-DIRECTION.md` is an `amendment` pull request on an `amend/<topic>` branch, reviewed by the owner and by the package that owns the files (AGENTS.md, "Making a change", item 8; CONTRIBUTING.md). This proposal therefore lands alone, and the owner's answers are recorded by the amendment that follows it.

| Order | Branch | Type | What it holds | Phase | Stop after? |
|---|---|---|---|---|---|
| 1 | `wp11/gaze-proposal` | docs | This file as `docs/GAZE.md`, nothing else | P0 | Yes. Wait for the owner's answers to section 16 |
| 2 | `wp11/p0-spike` | docs | `docs/research/2026-11-gaze.md` with the measurements of section 17, the webcam route's of item 11 included; `docs/wp/WP11-gaze.md`, the living plan, kept current from here on. Spike code stays in `docs/research/fixtures/gaze/`, never in a crate | P0 | Yes. The owner picks the tracker, the route, and whether the webcam route goes ahead |
| 3 | `amend/gaze-build` | amendment | The document changes of section 14; the `windows` crate's gaze feature in the root manifest's feature list | P1 | No |
| 4 | `amend/gaze-contracts` | amendment | `crates/eileen-contracts/src/gaze.rs` (a new module, `pinned_by = "WP11"`), the additions to the `connectors`, `desktop`, `hud`, `intent`, `config`, `learn`, `scenario`, `tools/brain` and `tools/desktop` modules (section 8), one insta snapshot, `text-allowlist.toml` entries with reasons | P1 | No |
| 5 | `wp11/gaze-crate` | feature | `crates/eileen-gaze`: the fixation and track filters, the recorded-sample parser with its `fuzz_entry` and fuzz target `gaze_samples`, the sidecar's server logic against a fake `GazeSource`, the rate limiter, the nine-point check; platform-neutral, tested on Linux. Its root-manifest member line, `[package.metadata.eileen]` (check 22) and its `ARCHITECTURE.md` code-map row (check 3) | P1 | No |
| 6 | `amend/gaze-runtime` | amendment | Starting `eileen-gaze.exe` through the supervisor and its `SidecarId::Gaze` client (WP2), the registry's refusal without the opt-in (WP2), the `gaze.*` setting specs (WP7), the machine plan's row for the sidecar (WP1), the tray's gaze indicator (WP2) | P1 | No |
| 7 | `amend/gaze-core` | amendment | In `eileen-core` (WP1): the gaze ring, the onset rule, `GazeOnset`, the continuation that waits for `desktop_hit_test`, the back-mapping through an enhance view, the control commands of G14, simulator invariant 17 parts (a) and (c). In `eileen-intent` (WP7): the `Gaze` slot and its golden-file cases | P1 | No |
| 8 | `amend/gaze-hit-test` | amendment | In `eileen-desktop` and `eileen-desktop-win` (WP5): `desktop_hit_test`, the section and scroll-container rules, table tests over fixture trees | P2 | No |
| 9 | `amend/gaze-hud` | amendment | In `eileen-hud` and `eileen-hud-win` (WP8): the reticle, the `Enhance` panel from a clipped DWM thumbnail, `CloseEnhance`, the `gaze_live` indicator, `GazeOver`; draw-list snapshots | P2 | No |
| 10 | `wp11/source-<route>` | feature | `crates/eileen-gaze-win`: the `GazeSource` the spike chose and the `test-gaze` injector with its closed-loop mode; the fixture pages `scenarios/gaze/_fixtures/dashboard/` and `scenarios/gaze/_fixtures/reading/`; the crate's manifest and code-map rows | P2 | No |
| 11 | `wp11/enhance-scenarios` | scenario | `scenarios/gaze/<id>/` for G1 and G14 (section 10), using the template fixtures of pull request 10 | P2 | No |
| 12 | `amend/gaze-commands` | amendment | G2 to G13 in `eileen-core` and `eileen-intent`, the addressee feature of 2.8 rule 5, the ledger's `Dwell` row (WP7), and their tests | P3 | No |
| 13 | `wp11/command-scenarios` | scenario | `scenarios/gaze/<id>/` for G2 to G13 | P3 | No |
| 14 | `amend/scroll-law` | amendment | The set-point law and session state machine of section 7 as pure code in `eileen-core`, with unit, property and closed-loop model tests; nothing calls it yet | P4 | No |
| 15 | `amend/scroll-glide` | amendment | In `eileen-desktop` and `eileen-desktop-win` (WP5): the background scroll routes, `Glide` with its clamps and server-side stops | P4 | No |
| 16 | `amend/scroll-session` | amendment | The session end to end behind `gaze.scroll.enabled`: the track request, the dispatcher's `Glide` rule, the scroll band on the HUD, the toggle hotkey, invariant 17 part (b) | P4 | No |
| 17 | `wp11/scroll-scenarios` | scenario | `scenarios/gaze/<id>/` for the scroll mode, run in the desktop lane with the closed-loop injector | P4 | No |
| 18 | `wp11/webcam-signals` | feature | In `eileen-gaze`: the webcam pipeline after the face model (section 18.3): the five signals, the ridge fit with its leave-one-out penalty, the ten-target calibration, the height-only track; tested on Linux over recorded landmark files, with fuzz target `gaze_landmarks` | P4 | No |
| 19 | `wp11/webcam-model` | feature | In `eileen-gaze`: the face landmark model through `ort`, behind P0's licence finding (18.7); the rule that the camera and the model live only while a session or the calibration runs, with its state test | P4 | No |
| 20 | `wp11/source-webcam-win` | feature | `eileen-gaze-win`: camera frames through Media Foundation, as a `GazeSource` of kind `Webcam` | P4 | No |
| 21 | `wp10/gaze-webcam-linux` | feature | `crates/eileen-gaze-linux`, built by WP10: camera frames through Video4Linux2, as a `GazeSource` of kind `Webcam`; its manifest and code-map rows | P4 | No |
| 22 | `wp11/webcam-scenarios` | scenario | The `test-gaze` injector's webcam profile (height only, with a webcam's noise and offset) and the scroll scenarios run with it, on Windows and on Linux | P4 | No |

### Rules that do not bend here

- Gaze never approves, never starts, never changes. No approval, mute change, job start, setting change, first-run choice or consent is ever built from a gaze event or a `GazeOver` line. The scroll mode is the only place gaze moves anything, and it moves only the one pane its session opened on, after a spoken start or a hotkey. Simulator invariant 17 (section 10) enforces this; a pull request that weakens it is closed. Approvals come only from physical or spoken `UserInput` (ARCHITECTURE.md, `eileen-core` invariants).
- No image of the user leaves the gaze sidecar. A hardware tracker's route gives points. The webcam route (section 18) reads camera frames into `eileen-gaze.exe`'s memory only while a scroll session or its calibration runs, turns each into landmarks and drops it. No frame, crop or image of the user ever crosses a process boundary or reaches disk, calibration and incidents included. A new repository check (section 14) greps `eileen-gaze`, `eileen-gaze-win` and `eileen-gaze-linux` for image encoders and fails on one.
- Every gaze point is `Personal<Point>`, so the `pii.` policy applies to it in the flight recorder, incidents and the scenario redactor.
- No new top-level `Input`, `Effect` or `BusEvent` variant. Gaze rides `Input::Sidecar(SidecarId::Gaze, ...)`, `Effect::Hud` and ordinary tool calls. If you believe one is needed, stop and write an amendment instead.
- Only `eileen-contracts` types cross a process, pipe or file. The test injector's samples and the recorded-sample files use the contract type `GazeSample`.
- Text from the screen is `Untrusted`. A section's label, an element's name, a window title: through `desktop::sanitize`, wrapped, fenced in the brain's context, never spoken beyond the bounded template of section 5.
- Build only through `cargo xtask` with the build guard (BUILD.md 6.1). Never `cargo build` directly, never `cargo install` from source, never a long build at the end of a session. Desktop tests take `desktop.lock` and drive only windows the test created.
- The owner's PC is memory-tight (1 to 2 GB free). The gaze sidecar is an optional resident the machine profile fills by value (BUILD.md 2.16). The vendor's tracker service runs outside Eileen but on the same PC, so P0 measures it too. The webcam route opens the camera and loads its model only while a scroll session or its calibration runs (18.8).
- Licences. Nothing under a licence that conflicts with MIT is bundled (CONTRIBUTING.md hard rule 4). How a vendor runtime may be used is open question 8. The webcam route's face model has its terms read in P0 (18.7).
- Writing style. Plain, short sentences, tables where they beat prose, no em or en dashes, no emoji, no marketing words. Relative links must resolve.
- Evidence first. Each pull request leads with the tests, scenarios, numbers or replay that show it is right, in the form `pr-meta` checks.

---

## Part B. The design

## 1. Summary

- What exists. `DesktopState` already answers "what is the user looking at" from the foreground window, the focused element and the selection (BUILD.md 2.6). The desktop server already has `window_from_point` and `element_from_point` (3.4, `Grounding`), physical-input detection through Raw Input, and a visible-intent outline before every click. The HUD already draws a live DWM thumbnail of a window a job works in (2.9). The intent resolver already has typed slots and an `Ambiguous` path (2.4, 3.3.10). Connectors already have a `senses_room` flag with an opt-in rule (2.14, 3.3.12). Gaze makes "what the user is looking at" literal and plugs into each of those.
- What is new. A tracker behind a platform trait; a sidecar that turns its samples into fixations; a ring of recent fixations in the core; a rule that picks the fixation at speech onset; a `Gaze` slot that resolves "this", "that", "here" and "there"; a hit test that turns a point into a window, an element and a section; an enhance view; the grammar of section 5; and an experimental scroll mode that moves a page by how far the eyes sit above or below a set point.
- What it is for. The owner's "enhance" (section 4), hands-free control of what the eyes already point at (section 5), reading long pages with no hands (section 7), better grounding for the brain, and a step toward the accessibility goal that no task requires a mouse (OWNER-DIRECTION.md, "Accessibility as a first-class goal").
- What it costs. One more supervised process, about one percent of a core and tens of MB [to confirm], plus the vendor's service, and a tracker on the owner's desk. The webcam route needs no new hardware; its camera and model cost memory and CPU only while a scroll session is open (18.8).
- When. Proposed for v0.2, as WP11 (section 12), after W6, so it never shares the desktop with WP9. v0.1 needs no change for it. The owner may pull P0 (the measurement spike) forward at any time, since it compiles nothing on the owner's PC.

## 2. Where gaze stands in the plan today

| Already in the design | Where | What gaze adds to it |
|---|---|---|
| "What am I looking at?" answered from DesktopState | E4, 2.6 | The literal answer: the section under the eyes, not only the foreground window |
| `Grounding::window_from_point`, `element_from_point` with deadlines | 3.4 | A point from the eyes instead of the mouse |
| Visible intent: outline and label 400 ms before any click, cancelled by "no, the other one" | 2.6 | The same outline on the gazed target before "click that" runs |
| The live DWM thumbnail on the HUD strip | 2.9, `HudState.watch` | The enhance view: the same mechanism, clipped to a section and scaled to the monitor |
| `desktop_scroll { hwnd, x, y, dx, dy }` at a point | 2.6, 3.3.9 | Background routes that need no focus, and a glide the scroll mode renews ten times a second |
| `recent_references`, `ReferenceKind::{Window, Element}` | 3.3.5 | A gaze target becomes a reference the brain and the slot resolvers see |
| Slot resolvers that return `Ambiguous` instead of guessing | 2.4 | A `Gaze` slot with the same honesty: two candidates under the accuracy disc make a question |
| Room-sensing connectors: opt-in per device, local only, derived events only | 2.14, 3.3.12 | `pc.gaze` is exactly that kind of connector |
| The local addressee score, one point per feature | 2.8 rule 5 | One more point when the user looked at Eileen's own window at onset, which can never pass the score alone |
| "No task requires a mouse" as a tested rule | 2.9 | Gaze plus a word reaches what a click reaches |
| The machine profile fills optional residents by value | 2.16 | The gaze sidecar is one of them |

## 3. Principles

1. Gaze is a slot, never a trigger. A spoken word, a hotkey or a physical click starts every action. Gaze only answers "which one". Looking at something for a long time does nothing, which is why the classic "Midas touch" problem of gaze interfaces does not arise. The scroll mode of section 7 keeps this: a word or a hotkey opens it, and inside it the eyes set only the speed and direction of one pane.
2. Deterministic first. "Enhance", "close this", "scroll here" and "click that" run on the fast path with no model: a fixation, a hit test, a tool call. The scroll mode is a pure control law in the core. The brain gets gaze as context for the requests that need interpretation.
3. Local, opt-in, and visible. Nothing leaves the PC. The connector starts only with a recorded opt-in. A HUD indicator and the tray show whenever the tracker is live, like the recording indicator. "Stop watching my eyes" pauses it at once.
4. The user's hands still win, and tiers do not move. A gaze-resolved click is the same click: the same visible intent, the same per-op tier, the same refusals (password fields, Eileen's own windows, blocked combos, other processes' windows). A gazed Send button still asks. Physical input ends a scroll session (section 7.5 says when).
5. Honest degradation. No tracker, lost eyes, a coarse check result or an ambiguous disc each fall back to one short question or to what Eileen does without gaze, never to a guess. Without gaze, the words of section 5 do what they do today: the grammar runs its entry where it has one, and otherwise the request goes to the brain or a worker.
6. Measured accuracy decides what is offered. Eileen's own nine-point check measures the tracker's accuracy (the mean distance from a target) and precision (the spread of samples while the eyes hold still) on this PC. Sections need about 60 px of accuracy, elements about 25 px, and a heatmap cell about 12 px on the owner's display. With accuracy under 25 px, element commands act directly; from 25 to 60 px they go through the candidate question; above `gaze.max_accuracy_px` (60) Eileen offers no gaze targets and suggests calibrating. Cells are reached through the enhance view, where they are three times larger. The scroll mode needs precision more than accuracy (section 7.4).

## 4. Enhance, in three steps

The worked example, from the owner's screenshots. Edge shows the dashboard. Eileen's check measured the tracker at 30 px accuracy. Times are the budgets of section 9.

| Step | What happens | Under the hood |
|---|---|---|
| 1. Look | The user looks at the "Activity by time" card. If `gaze.show_reticle` is on, the overlay draws a faint ring where Eileen thinks the eyes are | The sidecar's fixation filter emits `Fixation { Started }`, then updates; the core keeps it in the gaze ring (section 6.4) |
| 2. Say | "Eileen, enhance." | At `SpeechStarted` the core snapshots the fixation current at onset as the turn's `GazeRef`. At `EndOfTurn` the fast path matches the built-in entry and returns `CoreCommand::Gaze(Enhance { In })` |
| 3. See | Shortly after the end of speech, the card fills the monitor's work area, letterboxed, live. It keeps updating because it is a composition of the real window, not a screenshot. The reticle, if on, keeps following the eyes inside the view. The page beneath is untouched: no focus change, no input, nothing to undo in Edge | The core issues `desktop_hit_test { point, radius_px: 30, want: Section }` with origin `Core`. The desktop server runs `WindowFromPoint`, then `ElementFromPoint` on Edge's web content (the on-demand UIA request WP5 already makes before typing into a Chromium window), walks up the ancestors and applies the section rule of section 6.6: the nearest named group whose rect is between 3 and 60 percent of the window is "Activity by time". The core emits `Effect::Hud` with `HudPanel::Enhance { hwnd, source, label, level: 1 }`. The HUD registers a DWM thumbnail of that window with the card's rect as its source rectangle and the work area as its destination |
| 4. Ask | "What's this one?" while looking at the Thursday 4 pm cell in the view | The onset fixation lies inside the view, so the core maps it back through `EnhanceMap` to the real cell's pixels. At three times the scale, 30 px of error in the view is 10 px on the page, inside a 12 px cell. It hit-tests again with `want: Element`, and the fast path speaks the bounded template "That's Thursday, 4 pm, 93" from the sanitized name. A question that needs interpretation goes to the brain with a `gaze:` line and `LookTarget::Gaze` |
| 5. Deeper | "Enhance" again | Level 2: the section rule runs on the back-mapped point with the current section as the ceiling, so the heatmap grid alone fills the screen. `gaze.enhance_max_level` (3) caps it |
| 6. Back | "Back", "done", a click on the view's arrow, or any physical click on the view | The panel closes (`HudInput::CloseEnhance`). The close is the inverse the journal records, so "undo that" works too. While a view is open, "back" closes it before it means anything else |

What enhance refuses, in one sentence each: a minimized or cloaked window (DWM renders no thumbnail of it), a private window ("that window is private"), a window whose thumbnail DWM leaves black (protected content, or a window that excludes itself from capture; whether that can be told in advance from its display affinity is a P0 question [to confirm]), and the HUD itself. Esc does not close the view: the view never takes keyboard focus, so Esc would reach the app beneath.

Later rungs. When an app can enhance itself better than a pixel scale can, the route table of 2.5 prefers it: a browser's own fullscreen of the element through the Playwright `Dom` rung when the user installed it, Resolve's viewer zoom through `apps_resolve`. The first version ships the thumbnail route only, because it touches no app and needs no grant.

## 5. The use cases

The `Gaze` slot binds the demonstrative; without a usable gaze, each line falls back as principle 5 says. The tier is the existing tool's tier. G1 and G14 are phase P2, G2 to G13 phase P3, and G15 phase P4 (section 11).

| # | You say | What happens | Under the hood | Tier |
|---|---|---|---|---|
| G1 | "Enhance" / "zoom in on that" / "make that bigger"; "enhance more"; "zoom out"; "back" | Section 4 | `CoreCommand::Gaze(Enhance)`, `desktop_hit_test`, `HudPanel::Enhance` | none (HUD only), journaled with the close as its inverse |
| G2 | "What's this?" / "what am I looking at?" (E4, now literal) / "read this to me" / "summarise this" / "translate this" | A bounded spoken line from the element's or section's accessible name when one exists; otherwise, or for a question, the brain answers from `desktop_look { target: Gaze }` (one downscaled capture of the section's rect) and the fenced tree text | `Gaze(Describe)`; `LookTarget::Gaze` and `LookTarget::Region { rect }`; observed source `Screen` or `Tree` by the existing table | 0 |
| G3 | "Click that" / "press that" / "tick that" / "open that" | The outline and label appear on the gazed element for 400 ms, then `desktop_click_element` on it ("open that" invokes its default action). A send-like name is tier 2 and asks. Eileen's own windows, other processes' windows and password fields are refused by the server's existing rules | `Gaze(Click)`; the hit test returns the element's index and `Selector` (`HitElement`); an `Act` job from the fast path, a new rule of 2.12 (section 6.10) | 1, 2 ByArgs |
| G4 | "Scroll down" / "scroll up here" / "scroll down a lot" | The pane under the eyes scrolls one step, not the focused one, without taking focus | `Gaze(Scroll { dx, dy })` to `desktop_scroll` at the fixation point, through the background routes of section 7.6 | 1 |
| G5 | "Type here" / "put it here" / "dictate here"; or press the dictation key while looking at a field | If the gazed element is editable and not focused, the outline appears, the server focuses it (`desktop_click_element`), then the normal insertion ladder runs. Password fields refused as always | `Gaze(FocusField)`; `InsertTarget::Element` with the hit's index | 1 |
| G6 | "Close this" / "minimise this" / "maximise this" / "snap this to the left" | On the gazed window rather than the foreground one | `Gaze(Window(WindowAction))` to `desktop_window` with the hit test's `WindowRef` | 1, close 2 when unsaved state is unknown |
| G7 | "Move this there" / "put this on that screen" | "This" binds to the onset fixation, "there" to the fixation current at the end of the utterance, so one sentence names a window and a monitor by eye | The `Gaze` slot's two values, `Onset` and `End` (section 6.5); `WindowAction::Snap { monitor }` | 1 |
| G8 | While a question is open: "that one" / "this one" | The clarifying choice, contact-sheet image or job under the eyes is chosen, as if its name had been said. Never an approval, and never a yes or no: "that one" does not pick an option of a Confirm panel or a route proposal, which need their own words or a click | `HudInput::GazeOver { item }` from the HUD (section 6.7) names the item; the core answers the open question from the spoken turn | 0 |
| G9 | Looking at a job row on the HUD: "cancel it" / "how's this one?" / "show me its changes" / "pause this one" | The job under the eyes is the referent, ahead of ordinals and names | `GazeOver { Job(id) }` fills `JobRef` for the existing commands | as today |
| G10 | Speaking while looking at the HUD, with no name | One point in the local addressee score (2.8 rule 5) when the onset gaze was on one of Eileen's windows. It never passes the score alone (the threshold is two points), and looking anywhere else, or away, adds or subtracts nothing | `GazeOnset.on_eileen`, kept for every turn whether or not a `GazeRef` is taken | none |
| G11 | Unnamed requests about what is on screen: "why is Thursday so busy?" | The brain's `[desktop]` block carries a `gaze:` line (section 6.8), so the question is grounded without a screenshot | The gaze target as a `Reference` with a new `ReferenceKind::Gaze` in `DesktopState.recent_references` | 0 |
| G12 | "Where was I?" / "what was I reading before the call?" | Answered from the episodic ledger's dwell rows: window, document identity and seconds looked, never positions or content. Off by default, and refused while the tracker's terms allow live use only | `gaze.ledger`; a `Dwell` row in the ledger; the 14-day retention and off-the-record rules of 2.10 apply | 0 |
| G13 | Every HUD control by "that one" plus a look | The accessibility test of 2.9 extends: every clickable `HudInput` is reachable by its grammar entry or hotkey, and, with gaze, by `GazeOver` plus "that one". The physical-only inputs (Allow, Deny, mute, consent) and the yes or no of a Confirm panel stay out of reach of a look | the test table gains a column | as today |
| G14 | "Calibrate my eyes" / "check my eyes" / "stop watching my eyes" / "watch my eyes again" / "forget my gaze check" | Calibrate opens the vendor's own calibration, then runs Eileen's nine-point check; check runs the check alone (nine targets on the overlay, read aloud) and says what it found ("sections and buttons will work; small cells only in the enhanced view"); stop pauses the tracker and the indicator goes off; forget deletes `gaze\check.toml` and says so, and that the vendor keeps its own calibration | `CoreCommand::GazeControl`; the core-only `gaze_control` tool of `pc.gaze` | settings flow; `gaze.enabled` is a Safety key |
| G15 | "Scroll with my eyes" (or the eye-scroll hotkey); then just read; "stop" | Experimental. A faint line marks the set point on the page under the eyes. Looking below it scrolls down, above it scrolls up, faster the farther from the line, and the page eases to a stop when the text you were looking at reaches the line. Ends at "stop", a touch of the mouse or keyboard, or when the eyes leave the page | Section 7: `CoreCommand::Gaze(ScrollMode(Start))`, the `Track` stream, the set-point law, `desktop_scroll` with `Glide` | 1 |

What gaze never does. It never approves, denies, mutes, unmutes, consents, answers the first run, answers yes or no, starts a job, changes a setting or takes dictation. A look at the Allow button does nothing, and "click that" on it is refused, because the HUD is one of Eileen's own windows. A look never pauses a job, never wakes a lease, never cancels visible intent (only addressed speech and stop words do). It never selects text and never moves the mouse. It scrolls only inside a scroll session the user opened by voice or hotkey, and only the pane that session opened on. A tracker fooled by a photo therefore changes nothing by itself: an attacker would need the user's spoken command too, which is the same threat class as another voice in the room, with the same mitigations (2.8).

## 6. Architecture

### 6.1 Process and connector

A new supervised sidecar, `eileen-gaze.exe`, joins the processes table of 2.1:

| Process | Contains | What a failure costs | Transport to the core |
|---|---|---|---|
| `eileen-gaze.exe` | The tracker route (the Windows gaze input API or the vendor's runtime) or the webcam route (section 18), the fixation filter, the smoothed track, the nine-point check, the rate limiter | Gaze stops; every command of section 5 falls back; an open scroll session ends; the HUD says "eyes off" on the indicator | MCP over stdio, like the desktop and apps sidecars: a `connector_manifest` answer, then `ConnectorEvent::Gaze` events, and its two tools |

It is connector `pc.gaze`, the third PC connector beside `pc.desktop` and `pc.apps`: `RunsIn::Sidecar { binary: "eileen-gaze.exe" }`, `senses_room: true`, `requires_opt_in: true`, events `["gaze.fixation", "gaze.track", "gaze.lost", "gaze.found", "gaze.check", "gaze.health"]`, no checks (nothing of it is a recipe check), and two tools: `gaze_status` (tier 0, in the brain's toolset) and `gaze_control` (core-only). The registry refuses to start it without the recorded opt-in (2.14), which `gaze.enabled` writes through the settings flow as a Safety key. It is one of the optional residents of the machine profile's plan (2.16): kept when it fits, dropped under pressure with the usual hold, and the reason shown on the machine panel. If D79's general `Input::Connector` lands before WP11, gaze moves to it with the other connectors; until then it is a sidecar like desktop and apps.

The core never links it: the 2.1 process table keeps everything that can hang out of `eileen.exe`. The HUD never talks to it: the gaze point and the scroll band come through `HudState` at the HUD's ten snapshots a second, which keeps every data path through the core.

### 6.2 Sources

Two routes to a hardware tracker sit behind one trait, `GazeSource` (section 6.12), beside the webcam route of section 18. The P0 spike measures the tracker routes on the owner's PC with the tracker the owner picks, and the owner picks the first route to build.

| Route | How | Accuracy [to confirm] | Licence and consent questions |
|---|---|---|---|
| Windows gaze input (`Windows.Devices.Input.Preview`: `GazeInputSourcePreview`, `GazeDeviceWatcherPreview`) | The OS-level API behind Windows Eye Control, which supported trackers feed. Vendor-neutral, samples in screen coordinates. Microsoft's reference for [`GetForCurrentView`](https://learn.microsoft.com/en-us/uwp/api/windows.devices.input.preview.gazeinputsourcepreview.getforcurrentview) says it must be called on the UI thread and that the first call shows a one-time privacy prompt; it says nothing about desktop apps. Whether a Win32 process receives samples, and whether it needs a view or a package identity, is the spike's first question | About 0.5 to 1 degree, roughly 30 to 60 px on the owner's display at arm's length | The OS ships it; the user grants consent once in Windows' own prompt. What the tracker vendor's terms say about apps reading its device through Windows is read in P0 |
| The vendor's runtime (for Tobii consumer trackers, its game-integration or stream API) | The vendor's library at run time, from the user's own install, never bundled | As above | Tobii's [developer licence page](https://developer.tobii.com/license-agreement) limits its consumer SDK to interactive applications and sends analytical uses, such as processing gaze data after the fact or across users, to its Pro line. That page describes the older EyeX SDK, so the current runtime's terms are read in P0. Eileen's live use is interaction; the dwell ledger (G12) is analytical, so such a route reports `stores_nothing: true` and the ledger stays off. Whether loading it inside `eileen-gaze.exe` is acceptable under hard rule 4 is open question 8 |
| The webcam (section 18) | Frames from the PC's own camera through a face landmark model, fitted to the user by a ten-target calibration; height only in the first version | 2 to 4 degrees [to confirm]; a prototype missed held-out targets by 18.5 percent of the screen height (18.4) | No vendor. The face model's terms are read in P0 (18.7); the camera follows the operating system's own consent |

The tracker. Tobii's [specifications for the Eye Tracker 5](https://help.tobii.com/hc/en-us/articles/360012483818-Specifications-for-Eye-Tracker-5) list a 133 Hz gaze sampling rate, Windows 10 and 11, USB, and screens from 15 to 27 inches at 16:9. They state no latency or accuracy, which is why P0 measures both here. Other trackers that feed Windows gaze input work through the first route with no Eileen change.

Every route reports `GazeSourceInfo { kind, accuracy_px, precision_px, rate_hz, monitors, stores_nothing, checked }`, with accuracy and precision from Eileen's last nine-point check, not from a datasheet.

Calibration. Hardware trackers calibrate through the vendor's own app (and Windows Eye Control through its own flow), and Eileen does not replace that. "Calibrate my eyes" opens the vendor's calibration where the route allows it [to confirm], then runs Eileen's check. When the check's error has a consistent direction, Eileen keeps a small per-monitor offset correction (two numbers) in `gaze\check.toml` and applies it to every point.

Multi-monitor: a tracker covers the monitor it is mounted on. The first version covers that monitor only (the owner's PC has one). A point off every covered monitor reports `Lost { OffScreen }`.

### 6.3 From samples to fixations, and the smoothed track

The sidecar's neutral crate, `eileen-gaze`, owns the filters and is tested on Linux over recorded sample files.

- Samples arrive at the tracker's rate (about 33 to 133 Hz) as the contract type `GazeSample { at_us, point, confidence_pct }`. Shipped binaries never write them to disk. The `test-gaze` Cargo feature (never enabled in a shipped binary, like `test-physical`) adds a named-pipe injector that feeds synthetic samples of the same type, so the desktop lane can drive every path without a tracker. The recorded-sample files the tests use are JSON lines of the same type, read by a parser with its own fuzz target.
- Fixations come from a dispersion-threshold filter: a run of samples at least `gaze.fixation_ms` long (default 120) whose points stay within `gaze.dispersion` (default `auto`: the larger of three times `precision_px` and one degree at the measured distance) is one fixation; its centroid is the point. A fixation emits `Started`, then `Updated`, then `Ended`. Saccades emit nothing.
- The smoothed track exists for the scroll mode only, and only while the core has asked for it (`gaze_control { Track { on: true } }`). Reading moving text is smooth pursuit, which a dispersion filter turns into a string of short fixations or none, so the scroll mode cannot use fixations. The track is the sample stream through an adaptive low-pass filter of the One Euro kind (strong smoothing when the eyes are nearly still, little lag when they move), sent as `Track { point }` ten times a second, `None` while the eyes are lost.
- The rate. At most ten point events (fixation starts, updates and ends, or track points) reach the core each second. While the track is on, fixation events pause, so the track has the whole budget. State changes (`Lost`, `Found`, `Health`) are not point events and are limited separately, to at most one each a second. Raw samples never reach the core, as PCM never does from the speech process.
- Lost and found. No eyes for 300 ms is `Lost { NoEyes }`; a point off every covered monitor is `Lost { OffScreen }`; a device gone is `Lost { DeviceGone }`; `Found` ends it. Shorter gaps (a blink) show only as `None` track points.
- The reticle is drawn at the fixation centroid, so it sits still on a fixation and jumps between them. That is what the owner's screenshots show, and it is honest about what the core knows.

### 6.4 The core: the gaze ring and the onset rule

The core keeps a ring of the last eight seconds of fixations, timestamped with the `now` passed to `Core::step`, so the simulator replays it exactly.

The onset rule. When the speech process mints a turn (`VoiceEvent::SpeechStarted`), the core records `GazeOnset { on_eileen, on_screen }` from the ring for the addressee feature (G10), and snapshots a `GazeRef` for the target:

1. The fixation current at onset, if its point is not on one of Eileen's own windows.
2. Else the longest fixation that ended within `gaze.onset_window_ms` (default 1,500) before onset and was not on an Eileen window. People look at the assistant when they address it, so the fixation before the glance at the HUD is the target.
3. Else none: every command of section 5 falls back.

A second `GazeRef` is taken at `EndOfTurn` for the `End` slot value (G7). While a scroll session runs, fixations pause (6.3), so a command spoken during a session ends the session and takes its target from the last track point.

Why onset. Gaze leads speech by about half a second to a second: by the time "enhance" is finished the eyes have often moved to the HUD or elsewhere. The rule is a golden-file case in WP7's resolver tests, and the desktop-lane trials of section 10 measure how often it picks what the user meant.

Inside an enhance view. If the fixation's point lies within the open panel's destination rect, the core maps it back through `EnhanceMap` (`source.origin + (point - destination.origin) / scale`) before any hit test, and marks `GazeRef.through`. The round trip is a property test.

The continuation. The core is sans-IO, so a hit test is an effect and its answer an input. `CoreCommand::Gaze(...)` makes the core emit a `ToolCallRequest` for `desktop_hit_test` with origin `Core` (the origin the intent highlight and the media pause use, 3.3.2) and park the command under a timer (deadline 300 ms). On the `ToolResult` it finishes: builds the real tool call (`desktop_window`, `desktop_scroll`, `desktop_click_element`), the HUD panel, the spoken template, the scroll session, or the brain turn. On the timer, or an error, it falls back and says why in one line when the command cannot run at all. A brain turn whose `GazeRef` exists waits for the hit test too (in parallel with the earcon), so the `[desktop]` block can carry the target.

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
- `ScrollMode` comes only from the grammar on an addressed turn or from the hotkey, never from a brain turn.

### 6.6 The hit test and its rules (desktop server)

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

The scroll-container rule (for `want: ScrollContainer`, section 7.6): the nearest ancestor that supports UIA's Scroll pattern and is vertically scrollable, else the window under the point when it takes posted wheel messages. The answer names the route (`Pattern` or `PostedWheel`), the visible share of the content when the pattern reports it, and whether the content is at its top or bottom.

The thresholds are data in `eileen-desktop` (`gaze-sections.toml`), tested over fixture trees: the dashboard page, the reading page, the testpad, Notepad, Explorer, and a Resolve page recorded as a tree. Elements come back as `HitElement { index, selector, info }`, so `desktop_click_element` and `InsertTarget::Element` can use them directly. `candidates` lists every element whose rect meets the disc of `radius_px` around the point, nearest first, so the resolver can ask. A private window returns `private: true` with no element, no section, no scroll target and no label. An Eileen window returns the window only.

Cost: `WindowFromPoint` is microseconds; `ElementFromPoint` is a UIA call on the dedicated MTA thread with the usual deadline and abandon-on-hang rule (2.1). On a Chromium window it needs the on-demand accessibility request WP5 measures in its probe (acceptance 6); the gaze package records its own numbers beside them.

### 6.7 The enhance view, the reticle and the scroll band (HUD)

- The gaze point. `HudState.gaze: Option<HudGaze { point, accuracy_px, lost, draw_reticle }>` carries the current point whenever the tracker is live, because the HUD needs it to compute `GazeOver`. The ring is drawn only when `draw_reticle` is set (from `gaze.show_reticle`), at the HUD's ten updates a second. The reticle is off by default, on during the nine-point check and for the first week after enabling, when Eileen says once "I'll show you where I think you're looking; say 'hide the ring' when you trust it".
- The enhance panel. `HudPanel::Enhance { hwnd, source: Rect, label: Untrusted<String>, level: u8 }`. The HUD draws it on a full-work-area window that is not click-through (so the user's next physical click dismisses it, as a lightbox does), that keeps `WS_EX_NOACTIVATE` (it never takes focus) and `WDA_EXCLUDEFROMCAPTURE` (it never appears in Eileen's screenshots). The content is a DWM thumbnail (`DwmRegisterThumbnail`) with `rcSource` set to the section's rect and `rcDestination` the letterboxed work area, so it costs no capture and shows covered windows, as the strip's thumbnail already does. `HudSurface::set_watch` gains a source clip; a new `set_enhance` sets the panel. The label is drawn as the view's title with a back arrow, and the panel sends `HudInput::CloseEnhance` on the arrow or on any click, physical only. `HudInput::Back` keeps its meaning (return the foreground after a `Show`).
- The scroll band. While a scroll session is open, `HudState.gaze_scroll` (section 7.8) draws, on the click-through overlay and clipped to the pane, a thin line at the set point, a faint tint over the stop band, and a small chip at the pane's edge ("eye scroll: say stop"). The band is what the user aims for, so it is the visible intent for the whole session.
- Gaze over HUD items. The HUD computes its own layout, so it is the one that knows which job row, approval, option or contact-sheet image a point lies on. It reports `HudInput::GazeOver { item: HudItem }` lines, at most twice a second and only when the item changes. The core treats every `GazeOver` as non-physical whatever its line says, and uses it only to name a referent for G8, G9 and G13, never as an answer. The accessibility test's table marks `GazeOver` as not clickable, so it needs no grammar entry.
- The indicator. `Indicators.gaze_live: bool`, drawn whenever the tracker is live, never hidden by a setting, with the same tray mirror the recording indicator has.
- The nine-point check. Nine targets drawn on the overlay in turn while Eileen reads "look at the dot" and counts; the sidecar collects samples per target and reports `GazeCheck { points, accuracy_px, precision_px, offset, ... }`. A result above `gaze.max_accuracy_px` is kept, and Eileen says gaze targets are off until a better calibration.

### 6.8 The brain

- The `[desktop]` block of 2.3 gains one line when the turn has a target: `gaze: Edge "Loud" > section "Activity by time" (2.3 s)` or `gaze: cell "Thursday 4 pm, 93" in section "Activity by time"`, inside the fenced data block with every other screen text. Caps: one line, 120 characters.
- `LookTarget` gains `Gaze` (the turn's section or window) and `Region { rect }`, so `desktop_look` can capture the section alone, downscaled, which is cheaper and more useful than the whole screen. Observed source `Screen`, as today.
- `DesktopState.recent_references` gains the target as `Reference { kind: Gaze, label, window, at }`, so follow-ups inside the conversation window ("and the one next to it?") resolve through `last_turn` as they do for other references.
- `gaze_status` is the only `pc.gaze` tool the brain may call (tier 0), so "is the eye tracker on?" is answered honestly. The brain cannot open a scroll session or change the tracker.
- The brain never receives points, samples or tracks from the gaze path.

### 6.9 Settings

A new `[gaze]` section of `Config` (3.3.11), every key reachable by voice and on the HUD like every other:

```toml
[gaze]
enabled = false            # Safety (NeedsUserApproval): starts the tracker; the recorded opt-in of 2.14
source = "auto"            # auto | windows | vendor | webcam; auto prefers a tracker; Low
show_reticle = false       # Low, Live
fixation_ms = 120          # Low
dispersion = "auto"        # px, or auto from the check's precision; Low
onset_window_ms = 1500     # Low
enhance_max_level = 3      # Low
max_accuracy_px = 60       # above this check result, gaze targets are not offered; Low
addressee_feature = true   # G10; Safety, so the brain cannot propose it
ledger = false             # G12; Safety, and refused while the source reports stores_nothing
# [gaze.scroll], experimental: section 7.8
# [gaze.webcam]: section 18.3
```

The Safety keys are listed one by one in 2.11's Safety list (`gaze.enabled`, `gaze.addressee_feature`, `gaze.ledger`, `gaze.scroll.enabled`), not as a `gaze.*` prefix, because a prefix would make every key under it Safety. "Calibrate my eyes", "check my eyes" and "stop watching my eyes" are grammar entries, not settings; "hide the ring" and "show the ring" are the `show_reticle` phrases.

### 6.10 Privacy, security and tiers

| Rule | How it is kept |
|---|---|
| No image of the user leaves the gaze sidecar | The tracker routes give points only; the webcam route keeps frames in the sidecar's memory and drops each after its landmarks (18.5); no image encoder is linked into `eileen-gaze`, `eileen-gaze-win` or `eileen-gaze-linux`, and a new repository check fails on one; the check stores only error figures and the offset in `%LOCALAPPDATA%\eileen\gaze\check.toml`, and the webcam calibration only its fitted numbers |
| Points are personal data | Every gaze point in the contracts is `Personal<Point>`, so the `pii.` policy applies: an incident bundle holds them only with `logging.include_personal` on, and the scenario redactor drops them from a shared scenario unless its steps need them, in which case they are the synthetic points of its gaze steps |
| At most ten point events a second reach the core | The rate limiter in `eileen-gaze`; a property test |
| Gaze is never an answer or an approval | `GazeEvent` arrives as `Input::Sidecar`. `GazeOver` reaches the core as a HUD line (`UserInput::Hud`), so the core treats it as non-physical whatever the line says and never as an answer, an approval or a yes or no. Simulator invariant 17 (section 10) |
| Gaze alone never addresses a turn | The addressee feature is one point, only for a look at an Eileen window, against a threshold of two; invariant 17 part (c) |
| Tiers do not move | A gaze-resolved call is dispatched like any other, with its own op tier, taint and journal. A new dispatcher rule of 2.12 and 3.3.9 lets origin `FastPath` issue `desktop_click_element` and `desktop_scroll` for a gaze command only, as an `Act` job with the desktop toolset, and accepts `Glide` only from `FastPath` while a scroll session is open. This amends D41 (section 13) |
| The scroll mode moves one pane and nothing else | Opened only by an addressed turn or the hotkey; only `desktop_scroll` glides on the pane it opened on; the desktop server clamps every glide and stops it by itself on physical input and when its lifetime runs out; invariant 17 part (b) |
| Private windows | A fixation on an `is_private` window hit-tests to the window alone, with no element, no section, no scroll target and no label; the enhance view and the scroll mode refuse it; the ledger never records it |
| Off the record | The sidecar keeps running (the user may still say "enhance"), but no fixation, dwell row or `GazeRef` is persisted; the recorder's existing rule |
| The indicator cannot be hidden | `gaze_live` has no setting; the tray mirrors it |
| Spoofing | A photo or a video before the tracker steers nothing by itself (section 5, "What gaze never does"); the spoken command is the trigger, with the speaker-gate and voice-window rules of 2.8 unchanged. At worst a spoofed gaze scrolls the pane of a session the user opened, and the user's own hands stop it |
| Vendor terms | `stores_nothing` from the route's terms keeps the ledger off; nothing analytical runs on such a route |

### 6.11 Learning

- Gaze commands run as fast-path `Act` jobs: journaled, undoable and inside the regret window like every action (2.10), but never sent to the librarian, like every fast-path job. "No, the other one" after a gaze-resolved click counts as a correction for that job, and the resolver's golden file grows with the case.
- A brain turn or a worker job that used a gaze reference records the resolved `WindowRef`, `Selector` and section label in its trajectory, never points. A skill drafted from such a job gets a `Gaze` slot in its template ("close {this:gaze}"), resolved live at replay; a lost or ambiguous gaze refuses the replay and the brain or a worker takes over, by the existing rule.
- A scroll session is journaled once, with its net distance (section 7.6), and drafts nothing.

### 6.12 Platform

A new trait in `eileen-platform`, implemented in `eileen-gaze-win` for the tracker routes and the webcam, and in `eileen-gaze-linux` for the webcam (section 18.6). A Linux tracker route waits for a tracker with a supported Linux runtime; the neutral crate stays green on Linux either way.

```rust
pub trait GazeSource: Send + Sync {        // Windows gaze input, the vendor's runtime, or the webcam (section 18)
    fn info(&self) -> Result<GazeSourceInfo, PlatformError>;
    /// Starts the tracker; samples go to the sink until the subscription drops. Raw samples never leave the sidecar.
    fn start(&self, sink: Box<dyn Fn(GazeSample) + Send + Sync>) -> Result<Subscription, PlatformError>;
    /// Opens the vendor's own calibration where the route can, and says what it did: Started | OpenedVendorApp | Unsupported.
    fn calibrate(&self, deadline: Deadline) -> Result<CalibrationStart, PlatformError>;
}
```

The nine-point check needs no trait method: the sidecar collects samples per target from the same sink. The fake under `cfg(any(test, feature = "fake"))` plays recorded sample files, and the `test-gaze` injector feeds the real sidecar the same type over a named pipe; for the scroll tests it also runs closed loop (section 10). `Automation` gains `ancestors(element, deadline)`; `HudSurface` gains the clip and `set_enhance`.

## 7. Experimental: scrolling by gaze from a set point

### 7.1 The idea

The user fixes a reading line on the page. Looking below the line scrolls down, looking above it scrolls up, and the speed grows with how far the eyes are from the line. When the eyes come back within a small band around the line, scrolling stops.

This works because the reader's eyes stay on the text they want. As the page moves that text toward the line, the eyes follow it, the distance shrinks, and the page slows. It stops when the text sits on the line. The eyes are the error signal of a closed loop, and the stop is the moment the wanted text has entered the reader's focus.

It is experimental because it is the one place gaze drives something continuously. It lives behind `gaze.scroll.enabled` (off, a Safety key), it is measured by the trials of 7.9 before it loses the label, and every rule of 7.7 holds from its first pull request.

### 7.2 How it feels

| Step | What the user does | What Eileen does |
|---|---|---|
| 1. Start | Says "scroll with my eyes" (or presses the eye-scroll hotkey) while looking at a long page | Hit-tests the point for the scroll container (7.6). Sets the anchor at the current gaze height, clamped to the middle half of the pane. Outlines the pane for 400 ms, then draws the anchor line and the stop band. One earcon |
| 2. Look down | Looks at a paragraph near the bottom of the pane | Once the eyes have stayed beyond the start band for 200 ms, the page starts to move up, gently at first, faster for a paragraph far below the line |
| 3. Follow | Keeps looking at the paragraph as it rises | The paragraph's distance to the line shrinks, so the page slows |
| 4. Settle | Nothing | When the eyes, still on the paragraph, enter the stop band, the page stops. The paragraph sits on the reading line |
| 5. Read on | Reads down the paragraph | Reading a line or two below the band does nothing. Reading past the start band for 400 ms eases the text back up to the line, so long reading needs no hands |
| 6. Back up | Looks above the line at a heading that is scrolling out of view | The page moves down until the heading reaches the line |
| 7. End | Says "stop", touches the mouse or keyboard, or looks away from the page for 3 s | The band disappears and one earcon plays. The journal holds one entry, so "undo that" scrolls back to where the session began |

Looking at something outside the pane (the HUD, the taskbar, another window) stops motion at once; it never scrolls at the speed that distance would give.

### 7.3 The control law

Screen y grows downward. Let `a` be the anchor's height, `g` the smoothed gaze height from the track, and `d = g - a` the offset (positive below the anchor). Two bands surround the anchor: the stop band of half-height `s` and the start band of half-height `b`, with `b > s`.

```text
state Holding (no motion):
    if the gaze is inside the pane and |d| > b for start_dwell (200 ms; resume_dwell, 400 ms, after a settle):
        go to Moving
state Moving:
    target speed   v* = clamp(k * d, -v_max, +v_max)      # positive scrolls down: content moves up
    speed-up limit |v| may grow by at most a_max per second; slowing down is immediate
    if |d| <= s:                          v = 0, go to Holding   # settled: the text is on the line
    if d changes sign:                    v = 0, go to Holding   # a look to the other side must earn its own dwell
    if the gaze leaves the pane:          v = 0, go to Holding   # a glance elsewhere is not a command
    if the track point is None (a blink): keep v, never increase it, renew the glide
    if the sidecar reports Lost:          v = 0, go to Holding   # eyes gone for 300 ms or more
```

Speed is proportional to the distance from the set point. Four guards surround it. The start band and the dwell keep a glance or a jitter from starting motion. The gap between the start and stop bands (hysteresis) keeps the boundary from chattering. Going to Holding on a sign change means a look across the line never reverses the page at full speed. The speed-up limit keeps a sudden look at the bottom of the pane from jerking the page. The stop is a step from about `k * s` (around 60 px/s with the defaults below) to zero, which reads as the page settling, not braking.

Defaults, each `auto` from the P0 measurements of this PC:

| Parameter | Default | Why |
|---|---|---|
| Stop band half-height `s` | the larger of 3 x `precision_px` and 48 px | The noise of a still gaze stays inside it; 48 px is about two text lines at the owner's 150 percent scale [to confirm] |
| Start band half-height `b` | 1.6 x `s` | Hysteresis |
| Gain `k` | 0.25 / tau, never above 0.35 / tau | The stability bound of 7.4, with margin; tau is the measured loop delay |
| Maximum speed `v_max` | 1,500 px/s | A ceiling, rarely reached: with `k` = 1.25 per second, a look 600 px below the line gives 750 px/s |
| Speed-up limit `a_max` | 3,000 px/s per second | Reaches 750 px/s in a quarter of a second |
| Anchor | the gaze height at the start command, clamped to the middle half of the pane | People read in a preferred part of the screen that varies by person (7.10), so the user sets it by looking |

If 3 x `precision_px` would make the stop band taller than 15 percent of the pane, the tracker is too noisy for this pane: Eileen says so and offers "scroll down" instead.

### 7.4 Why it stops where the reader wants, and how fast it may go

Let `x` be the wanted text's height relative to the anchor. Scrolling at speed `v` moves it by `dx/dt = -v`. The reader's eyes are on the text, so the gaze reports `x` late by the loop delay `tau`: the tracker's latency, the smoothing, the event period, the dispatch, the app's repaint, and the eye's own lag in following moving text. Outside the stop band the law gives

```text
dx/dt = -k * x(t - tau)
```

the classic first-order loop with a delay. Its behaviour is known. From rest, with `k * tau` at most 1/e (about 0.37) the text approaches the line without overshooting it; between 1/e and pi/2 it overshoots and oscillates while settling; above pi/2 the oscillation grows. So the gain is not a matter of taste: it is capped by the measured delay. The default `k = 0.25 / tau` and the cap of `0.35 / tau` both stay under the no-overshoot bound. Inside that bound the offset shrinks a little faster than `exp(-k t)`, because the law acts on the older, larger offset: with `k` = 1.25 per second and tau = 200 ms, at about 1.8 per second. Because the speed near the band's edge is small, the page travels only about one delay's worth of that speed after the text reaches the band and before the eyes report it there, well inside the band.

Planning figures for tau, all [to confirm] in P0:

| Part of the loop | Planning figure |
|---|---|
| The tracker, from the eye to the API | 20 to 50 ms |
| Smoothing in the sidecar | about 30 ms |
| The event period at ten a second | 50 ms on average |
| The core step, the dispatch and the desktop server | under 10 ms |
| The glide's next frame and the app's repaint | 20 to 40 ms |
| The eye catching up with text that moves | the rest of what P0 measures |
| Total tau | about 150 to 250 ms, so `k` about 1.0 to 1.7 per second |

A worked example, from a step-by-step model of this loop with the ten-a-second events, the 200 ms dwell, the speed-up limit and a pure delay standing in for the eye. Tau is 200 ms, so `k` is 1.25 per second. The wanted paragraph sits 600 px below the line. Motion starts 0.2 s after the eyes land on it. The eyes report the paragraph inside the 48 px band about 1.9 s after they landed, with the paragraph about 36 px below the line and no overshoot. With a 120 ms loop (`k` = 2.1 per second) the same move takes about 1.4 s. With `k * tau` at 1.0, far over the bound, the page carries the paragraph about 150 px past the line, beyond the start band on the other side, and has to come back. That is the cost of latency, and why section 9 budgets it.

Two consequences of the closed loop matter for the design:

- Accuracy matters less than precision, for the loop. The anchor is set by the user's own gaze through the same tracker, so a constant calibration offset moves the anchor and the gaze together, and the loop still settles the text where the eyes were at the start. What remains is precision (the noise), which sets the band, and drift over a session, which the session limits and a re-anchor ("set the line here") cover. The drawn line is a different matter: it sits at the corrected gaze height, so any offset left after the check's correction is the distance between the drawn line and where the text settles. With `anchor = "center"`, or when the clamp moves the anchor, the offset no longer cancels at all.
- A PC whose real delay is longer than measured would hunt around the line. The session watches for it: two overshoot reversals (motion stops, then starts the other way within 600 ms) halve `k` for the rest of the session, and the event is logged with the session's numbers so the next measurement can correct tau.

### 7.5 Starting, pausing and ending

| Event | Effect |
|---|---|
| "Scroll with my eyes", "eye scroll", "follow my eyes" (an addressed turn) | Opens a session on the pane under the onset fixation (`CoreCommand::Gaze(ScrollMode(Start))`) |
| The eye-scroll hotkey (`hotkeys.gaze_scroll`, unset by default), a registered hotkey like Eileen's others | A toggle: the first press opens a session on the pane under the current gaze, the next press closes it. It is among the combos the desktop server refuses to send (2.6), so no worker can press it |
| "Set the line here" | Re-anchors to the current gaze height |
| "Slower" / "faster" | Scales `k` by 0.8 or 1.2 for the session, never above `0.35 / tau` and never below `0.1 / tau`; at the cap, Eileen says it is already at its fastest steady speed |
| The gaze leaves the pane, or the sidecar reports `Lost` | Motion stops at once; the session pauses |
| Paused for `away_end_ms` (3 s) | The session ends |
| No motion for `idle_end_s` (60 s) | The session ends |
| `max_session_s` (600 s) | The session ends, with one line: "say scroll with my eyes to keep going" |
| "Stop", "that's good", "done", or any stop word | The session ends |
| Another addressed command | The session ends first; the command takes its gaze target from the last track point |
| Any physical mouse or keyboard input, after the first 500 ms of the session | The desktop server stops the glide within one event, and the session ends. The first 500 ms are exempt so that releasing the hotkey that opened the session does not close it |
| The pane's window closes, minimizes, or becomes private | The session ends |
| A job asks for the screen | An open session counts as the user being active, so the job asks by voice and on the HUD instead of taking the screen after the idle countdown of 2.6; if the user agrees, the session ends first |
| `gaze.scroll.end_on_settle = true` (a trial arm) | The first settle ends the session: a one-shot "find" instead of read-along |

### 7.6 Picking the pane and moving it smoothly

The pane. At the start command the core asks `desktop_hit_test { point, radius_px, want: ScrollContainer }` at the onset fixation. The answer is the nearest vertically scrollable container with its rect and route. An Eileen window, a private window, a pane with no route, or a pane already at both ends is refused with one line ("I can't scroll that").

The routes. Today `desktop_scroll` turns the wheel through `SendInput`, which needs the foreground and the cursor over the pane (2.6). Gaze commands must take no focus and never move the cursor, so the amendment gives `desktop_scroll` two background routes, used for gaze in this order:

1. UIA's Scroll pattern on the container. No cursor movement, no foreground needed. The server converts pixels to the pattern's percent with the container's height and the visible share it reports (`px per percent = height * (100 / view_pct - 1) / 100`), and reads the position back, so each result carries `moved_px`. Whether apps repaint smoothly at 30 to 60 pattern calls a second is a P0 probe [to confirm].
2. Wheel messages posted to the window under the point (`PostMessage` with `WM_MOUSEWHEEL` and the point in its parameters), in fractional deltas where the app scrolls smoothly on them. No cursor movement, no foreground needed [to confirm with WP5 which apps honour posted wheel messages].

A pane that neither route reaches is refused. The `SendInput` route stays for workers, and gaze never uses it.

The glide. One step of `desktop_scroll` per event would move in jumps ten times a second. Instead the tool gains an optional `glide: Glide { vy_px_s, ms }`: "move this pane at this speed for at most this long". The core renews it at each track event, ten times a second, with `ms` of 250, and the desktop server animates it at the display's rate on its own thread. On a settle or a stop, the core sends a zero glide at once. Four properties follow:

- A dead man's switch. If renewals stop (the core, the sidecar or the tracker fails), the glide ends by itself within 250 ms. Nothing keeps scrolling on a stale command. The 250 ms lifetime leaves room for one late renewal on a loaded PC.
- Stops without a round trip. The desktop server already tracks physical input through Raw Input (2.6), timestamps only; on any physical input after a session's first 500 ms, or when the pane's window stops being visible, it ends the glide itself and says so in the next result.
- Bounded everywhere. The server clamps `vy_px_s` to 2,000 px/s and `ms` to 250, whoever calls. The dispatcher accepts a `glide` only from origin `FastPath` while a scroll session is open, so workers, replays and the brain cannot glide.
- One journal entry. The calls of one session share the session's `Act` job; the dispatcher journals the session once, with the net distance, and "undo that" scrolls back by it (by scroll position where the pattern reports one).

Gain matching. An app may move more or fewer pixels than asked. When results carry `moved_px`, the core keeps the ratio of moved to asked over the session (clamped between 0.5 and 2) and divides it out, so the loop's real gain stays under the bound of 7.4. Without readback the ratio is 1, and the reversal guard of 7.4 is the backstop. At the end of the content a result reports `at_end`, the session holds, and the HUD chip says "end of page".

Visible intent. The 400 ms outline of 2.6 runs once, on the pane, before the first glide; the band then stays on screen for the whole session. A wait on every call would make continuous motion impossible, so later glides in the same session do not wait.

### 7.7 What keeps it safe

- Opened only by the user: an addressed turn or the hotkey. No gaze input, no brain turn, no worker and no skill can open a session.
- One pane, one tool: inside a session, gaze inputs can cause only `desktop_scroll` glides on the pane the session opened on. No click, no key, no focus change, no window action.
- Tier 1, as scrolling is today. Scrolling can have side effects in some apps (a chat app may mark messages as read), exactly as a wheel turn does; the scroll mode adds none.
- Bounded: at most ten glide calls a second, each clamped in speed and lasting at most 250 ms, in sessions of at most 10 minutes.
- The user's hands win: physical input ends the session within one event, in the desktop server itself.
- Visible: the indicator, the band and the chip are on screen whenever a session is open.
- Simulator invariant 17 part (b) checks all of it (section 10).

### 7.8 Settings, contracts and HUD state for the mode

```toml
[gaze.scroll]                 # experimental; Low unless noted
enabled = false               # Safety; the mode exists only when on
trigger = "voice"             # voice | hotkey | both; the hotkey is hotkeys.gaze_scroll
anchor = "gaze"               # gaze (clamped to the middle half of the pane) | center
end_on_settle = false         # true: the first settle ends the session (trial arm)
stop_band_px = "auto"         # half-height; auto = max(3 x precision_px, 48)
start_band_px = "auto"        # half-height; auto = 1.6 x stop band
gain_per_s = "auto"           # auto = 0.25 / measured tau; any value is capped at 0.35 / tau
max_speed_px_s = 1500         # at most 2000, the server's clamp
accel_px_s2 = 3000            # speed-up only; at most 6000
start_dwell_ms = 200
resume_dwell_ms = 400
away_end_ms = 3000
idle_end_s = 60
max_session_s = 600           # at most 1800
show_band = true              # the tint over the stop band; the anchor line always shows
```

The contract additions are in section 8: `GazeEvent::Track`, `GazeAction::Track`, `GazeCommand::ScrollMode`, `Glide` on `DesktopScroll` with `moved_px` and `at_end` in its output, `HitWant::ScrollContainer` with `ScrollTarget`, `HudState.gaze_scroll`, and the scenario's `GazeFollow` step with a simulated page.

### 7.9 How to tell whether it works: the trials

On the reading fixture of section 10 and two real apps (a long article in Edge and a long file in VS Code), with tasks of the form "bring paragraph N to your reading line" from random starting positions, twenty per arm, the owner runs:

| Arm | What it is |
|---|---|
| A | The law of 7.3, started by voice, read-along on |
| B | The same, started and ended with the hotkey |
| C | A steeper curve: speed grows with the 1.5th power of the offset, with its steepest point held to the same bound |
| D | One-shot: `end_on_settle = true` |
| E | Baselines: the mouse wheel, and "scroll down" by voice (G4) |
| F | The webcam route (section 18) with arms A and D, on Windows and on Linux |

Measured: time until the target sits in the band; overshoot reversals; false starts, counted over two minutes of plain reading with a session open; ends by stop word against ends by touch; and a one-to-five comfort rating. Proposed bar to drop "experimental" [to confirm with the owner]: no reversal in 95 percent of trials, fewer than one false start per two minutes of reading, a median under 3 s for a target one pane away, and the owner's yes. The results go in `docs/wp/WP11-gaze.md` and B11.

### 7.10 Prior art, and what is new here

- Aaron's "Gaze Zone Scroller" (shown to the owner and to WP10 on 2026-10-26) scrolls a page by gaze from an ordinary webcam. The user locks a band of the screen where reading feels comfortable, and nothing moves while the eyes stay inside it, because a single line "works badly": the eyes are almost never exactly on it. When the eyes leave the band, the page brings the text being read back into it, faster the farther they look. Two small margins keep it calm: the gaze has to go a few percent of the screen height past an edge and stay out for a fifth of a second. Its calibration fits five signals from a face landmark model by ridge regression (section 18.3). The band, the dwell and the proportional speed of this section are the same idea, and section 18's webcam route is built on the prototype's design and numbers.
- Kumar, Winograd and Paepcke at Stanford proposed [gaze-enhanced scrolling techniques](https://hci.stanford.edu/publications/paper.php?id=75) in 2007, manual and automatic, with gaze as the main or an added input. As a later study describes them, they used fixed screen regions that scroll when looked at, turned automatic scrolling on and off with a dedicated key, and some assumed people read in the central third of the view.
- A [2015 study presented at the PETMEI workshop](https://microsoft.com/en-us/research/wp-content/uploads/2016/07/PETMEI2015scrolling_petmei_15_final.pdf) watched 18 people scroll by hand. People read in a preferred region that varied by person (about half of scroll events came while they looked at the middle third), and they scrolled in chunks of a few lines (a median of about 3.5 lines while reading) rather than smoothly. The study also summarises an auto-scroll by Sharmin and colleagues that starts when the gaze crosses the lower edge of the user's usual reading range and speeds up beyond it.
- What that means here: the anchor is set by the user's own gaze, not fixed at the centre; the stop band is a few lines tall; and the trials include a one-shot arm and a steeper arm, so the chunked habit is tested rather than assumed away.
- What this plan adds to the prototype: the gain capped by the loop delay measured on this PC; the stop as the closed loop's own settling point, with hysteresis; and the safety envelope of 7.7 (a spoken or hotkey start, one pane, a server-side dead man's switch, and hands that always win).

## 8. Contracts

Sketches in the style of 3.3; the amendment writes them in full. New module `gaze`, `pinned_by = "WP11"`. The other modules named below are provisional today and will mostly be pinned when WP11 runs, which is why pull request 4 is an amendment. Every string from the screen is `Untrusted`; every other plain string is listed in `text-allowlist.toml` with its reason.

```rust
// eileen_contracts::gaze (new, pinned by WP11). Every gaze point is Personal<Point>.
pub struct GazeSample { pub at_us: u64, pub point: Option<Personal<Point>>, pub confidence_pct: u8 }
    // the injector's pipe and the recorded-sample files; never written to disk by a shipped binary
pub struct GazeSourceInfo { pub kind: GazeSourceKind, pub axes: GazeAxes, pub accuracy_px: u16, pub precision_px: u16,
    pub rate_hz: u16, pub monitors: Vec<u8>, pub stores_nothing: bool, pub checked: Option<WallTime> }
    // kind: WindowsGazeInput | Vendor { name } | Webcam; axes: Both | Vertical (the webcam route at first:
    // the x of its points means nothing and is never hit-tested, section 18.2)
pub enum GazeEvent {
    Ready(GazeSourceInfo),
    Fixation(Fixation),                   // Started, then Updated, then Ended; point events, at most ten a second with Track
    Track(Track),                         // only while the core asked for it (a scroll session); fixations pause meanwhile
    Lost { reason: LostReason },          // NoEyes | OffScreen | DeviceGone | Paused
    Found,
    Check(Option<GazeCheck>),             // after "check my eyes"; None after "forget my gaze check"
    Health { rate_hz: u16, latency_ms: u16, cpu_pct: u8 },
}
pub struct Fixation { pub id: u64, pub phase: FixationPhase, pub point: Personal<Point>, pub monitor: Option<u8>,
    pub age_ms: u32, pub dispersion_px: u16, pub confidence_pct: u8 }   // physical virtual-screen pixels, like every Point
pub struct Track { pub point: Option<Personal<Point>>, pub age_ms: u16, pub confidence_pct: u8 }   // smoothed; None while lost
pub struct GazeCheck { pub points: u8, pub accuracy_px: u16, pub precision_px: u16, pub offset: Option<(i16, i16)>,
    pub monitor_device_path: String, pub at: WallTime }
/// Kept for every addressed turn at speech onset, for the addressee feature (section 6.4).
pub struct GazeOnset { pub on_eileen: bool, pub on_screen: bool }
/// The target an addressed turn captured at speech onset and at its end (section 6.4).
pub struct GazeRef { pub point: Personal<Point>, pub fixation_ms: u32, pub accuracy_px: u16, pub age_ms: u32,
    pub through: Option<EnhanceMap> }
pub struct EnhanceMap { pub hwnd: i64, pub source: Rect, pub destination: Rect }
/// desktop_hit_test's output.
pub struct GazeHit { pub window: Option<WindowRef>, pub element: Option<HitElement>, pub section: Option<Section>,
    pub scroll: Option<ScrollTarget>, pub candidates: Vec<HitElement>, pub private: bool }   // candidates: nearest first
pub struct HitElement { pub index: u32, pub selector: Selector, pub info: ElementInfo }
pub struct Section { pub rect: Rect, pub label: Untrusted<String>, pub control_type: String, pub selector: Selector, pub depth: u8 }
pub struct ScrollTarget { pub rect: Rect, pub element: Option<u32>, pub route: ScrollRoute, pub view_pct: Option<u16>,
    pub at_top: bool, pub at_bottom: bool }                           // route: Pattern | PostedWheel

// connectors: SidecarId gains Gaze; ConnectorEvent gains Gaze(GazeEvent)
// desktop: ReferenceKind gains Gaze
// tools/desktop:
pub struct DesktopHitTest { pub point: Point, pub radius_px: u16, pub want: HitWant }   // Window | Element | Section | ScrollContainer. Tier 0
pub struct DesktopScroll { pub hwnd: i64, pub x: i32, pub y: i32, pub dx: i32, pub dy: i32, pub glide: Option<Glide> }   // tier 1
pub struct Glide { pub vy_px_s: i32, pub ms: u16 }
    // the server clamps vy_px_s to 2,000 and ms to 250; dx and dy must be 0; accepted only from FastPath in a scroll session;
    // the output gains moved_px: Option<i32> and at_end: bool
// tools/brain: LookTarget gains Gaze and Region { rect: Rect }
// pc.gaze's own tools:
pub struct GazeStatus {}                            // tier 0, in the brain's toolset; returns GazeSourceInfo and whether it is live
pub struct GazeControl { pub action: GazeAction }   // core-only:
    // Pause | Resume | Calibrate | Check { monitor: u8 } | Forget | Track { on: bool }
// intent:
pub struct ResolveCtx { /* ... */ pub gaze: Option<GazeCtx> }
pub struct GazeCtx { pub usable: bool, pub accuracy_px: u16, pub on_eileen: bool, pub enhanced: bool }
pub enum CoreCommand { /* ... */ Gaze(GazeCommand), GazeControl(GazeAction) }
pub enum GazeCommand {
    Enhance { step: EnhanceStep },        // In | Out | Off
    Describe { how: DescribeHow },        // WhatIsThis | Read | Summarise | Translate { to: String }
    Click,                                // click that, press that, tick that, open that (the default action)
    Scroll { dx: i32, dy: i32 },          // one step at the eyes (G4)
    FocusField,                           // type here, put it here, the dictation key with a gazed field
    Window(WindowAction),                 // close this, snap this left, move this there (Snap { monitor } from the End value)
    Which,                                // "that one" while a question is open; never a yes or no
    Where,                                // "where was I?" (G12)
    ScrollMode(ScrollModeAction),         // Start | Stop | Reanchor | Faster | Slower (section 7); never from the brain
}
// The Gaze slot's value, substituted by the core: {"gaze": "onset"} | {"gaze": "end"}
// hud:
pub struct HudState { /* ... */ pub gaze: Option<HudGaze>, pub gaze_scroll: Option<HudGazeScroll> }
pub struct HudGaze { pub point: Personal<Point>, pub accuracy_px: u16, pub lost: bool, pub draw_reticle: bool }
pub struct HudGazeScroll { pub pane: Rect, pub anchor_y: i32, pub stop_band_px: u16, pub start_band_px: u16,
    pub state: GazeScrollState }          // Holding | Moving { down: bool } | Paused | AtEnd
pub enum HudPanel { /* ... */ Enhance { hwnd: i64, source: Rect, label: Untrusted<String>, level: u8 } }
pub struct Indicators { /* ... */ pub gaze_live: bool }
pub enum HudInput { /* ... */ GazeOver { item: HudItem }, CloseEnhance }
    // GazeOver is treated as non-physical whatever its line says; it names a referent, never an answer or approval
pub enum HudItem { Job(JobId), Approval(ApprovalId), Option { id: u64, option: u8 }, Strip, Nothing }
// config: Config gains gaze: GazeConfig with scroll: GazeScrollConfig (sections 6.9 and 7.8);
//     hotkeys gains gaze_scroll: Option<String>
// scenario: Step gains Gaze { point: Point, hold_ms: u32 }, GazeFollow { target_y: i32, delay_ms: u16, noise_px: u16 }
//     (a model eye that follows a line of the simulated page) and Lost {}; SimWorld gains
//     gaze: Option<SimGaze { accuracy_px: u16, precision_px: u16 }> and page: Option<SimPage { pane: Rect, content_px: u32 }>
// learn: the ledger gains a Dwell row { window, document, seconds }, behind gaze.ledger
```

Wire: `GazeEvent` travels as `Input::Sidecar(SidecarId::Gaze, SidecarEvent::Event(ConnectorEvent::Gaze(event)))`; nothing new at the top level. Gaze events are not bus events: they are not "what happened" for listeners, and they would flood the plugin and the `events` table. So `BusEvent` gains nothing, the HUD learns the point and the band from `HudState`, and the ledger learns dwell from `LearnCmd`.

## 9. Budgets

| Stage | Budget | Where measured |
|---|---|---|
| Sample to `Fixation { Started }` | `fixation_ms` plus at most 50 ms | unit test on recorded samples; the owner's PC |
| Point events into the core | at most 10 a second, fixations and track together; state changes at most one each a second | property test |
| Sidecar memory and idle cost | under 40 MB and under 1 percent of one core [to confirm]; the vendor's service measured beside it | `cargo xtask measure-idle`; the P0 report; rows for BUILD.md 4.4 and 4.5 |
| `desktop_hit_test` | p95 under 60 ms from the cache for the window, under 150 ms with `ElementFromPoint`; deadline 300 ms | the desktop lane |
| "Enhance", end of speech to the view visible | the fast path's 0.5 to 0.6 s quiet and 0.8 to 1.2 s typical (4.3), plus the hit test and the thumbnail setup: under 0.8 s quiet and 1.4 s typical [to confirm] | timed scenario |
| A brain turn with a gaze target | plus one hit test, in parallel with the earcon: first word still within the 1.5 s target on a quiet PC | B10's cases with gaze |
| Reticle and scroll band | 10 updates a second through `HudState`; no extra HUD cost beyond the snapshot it already draws | `measure-idle` |
| Accuracy needed | section 60 px, element 25 px, cell 12 px (about 1, 0.4 and 0.2 degrees on the owner's display); the check's `accuracy_px` decides which are offered | the nine-point check |
| Scroll loop delay tau, gaze sample to the page moving | under 250 ms, aim 150 ms; it sets the gain (7.4) | trace spans, plus the P0 step test |
| Scroll stop, eyes enter the band to the page still | under 200 ms (up to one event period, then the zero glide and a repaint) | desktop lane, closed-loop injector |
| Scroll stop on physical input | within one input event, in the desktop server | desktop lane with `test-physical` |
| A glide with no renewal | ends within its `ms`, at most 250 ms | unit test on the server's glide timer |
| Scroll settling, target one pane away | under 3 s median (the trial bar of 7.9) | the owner's trials |
| Webcam route while a session runs | the budgets of 18.8; nothing while no session runs | P0 item 11; `measure-idle` |

## 10. Testing

- Unit and property. The dispersion filter on synthetic streams (every fixation at least `fixation_ms`, centroid inside the dispersion window, saccades emit nothing); the track filter (lag and jitter on recorded pursuit); the onset rule over scripted rings; the back-mapping through `EnhanceMap` round-trips; the rate limiter never passes more than ten point events a second, with fixations paused while the track runs; the section and scroll-container rules over fixture trees.
- The scroll law. Table tests: zero inside the stop band, the sign of the speed follows the sign of the offset, the speed never falls as the offset grows within one side, the cap holds, no start before the dwell, a sign change goes to Holding, a short blink keeps the speed without raising it, no chatter on a noisy stream at the band's edge. A closed-loop model test, pure and seeded (randomness passed in): a simulated page and a model eye that follows a target line with delay tau and noise, with ten-a-second events and the dwell. For `k * tau` at most 0.35, every run over a range of starting offsets `d0` ends with the target inside the stop band, with no reversal, within `start_dwell + 2 event periods + tau + ln(d0 / s) / k + v*/a_max`. A deliberately unsafe `k * tau` of 1.0 shows reversals and the reversal guard halving the gain, which is why the cap exists.
- Fuzz. `gaze_samples`: the recorded-sample parser and both filters; clean for ten minutes in `long.yml`, with a row in 4.7's fuzz table. Every new parser of untrusted input gets a target, and a vendor stream parser, if one is written, gets its own.
- Snapshots. The `gaze` contracts module and the desktop toolset's input schemas; HUD draw lists for the reticle, the enhance panel at each level and corner, the scroll band in each state, and the indicator.
- Simulator invariant 17, in three parts. (a) Over any input stream whose only inputs besides timers are gaze events and `GazeOver` lines, the core emits no `ApprovalResolved`, no `JobAccepted`, no `ToolForward`, no `Persist(SettingsVersion)` and no `Speech(Speak)`; only `Hud` snapshots, `Learn` dwell rows when `gaze.ledger` is on, and timers. No `GazeOver` line, physical flag or not, ever resolves an approval, a Confirm panel's yes or no, or a route proposal. (b) A scroll session opens only from an addressed turn or the hotkey. While one is open, the only tool calls gaze inputs cause are `desktop_scroll` glides on the session's pane, at most ten a second, each with `ms` at most 250 and speed within the clamp. After a stop word, physical input past the first 500 ms, `away_end_ms`, `idle_end_s` or `max_session_s`, no further glide is issued except the zero glide. (c) No utterance passes the addressee score on the gaze feature alone. Checked on the PR seeds and the regressions.
- Resolver golden file. The `Gaze` slot cases of 6.5, including "close Chrome" ignoring gaze, "move this there" binding onset and end, every fallback, and `ScrollMode` never resolving from a brain turn.
- Scenarios in `scenarios/gaze/<id>/`, replayed in simulation with scripted gaze steps and run in the desktop lane with the `test-gaze` injector. For P2: `enhance-section`, `enhance-cell-through-view`, `enhance-minimized-refused`, `enhance-private-refused`, `stop-watching-pauses`, `check-my-eyes`. For P3: `this-window-close`, `scroll-under-gaze`, `click-that-send-asks` (a Send-named control stays tier 2), `click-eileen-window-refused`, `gaze-lost-falls-back`, `that-one-contact-sheet`, `that-one-never-says-yes`, `move-this-there` (`monitors>=2`, the fleet), `addressee-looking-at-hud`, `never-approves` (gaze on Allow plus silence: nothing; gaze on Allow plus "click that": refused). For P4: `scroll-mode-settles-on-target`, `scroll-mode-look-up-scrolls-up`, `scroll-mode-stop-word-ends`, `scroll-mode-physical-input-ends`, `scroll-mode-eyes-away-pauses-then-ends`, `scroll-mode-blink-keeps-moving`, `scroll-mode-end-of-page-holds`, `scroll-mode-eileen-window-refused`, `scroll-mode-private-refused`, `scroll-mode-never-starts-from-gaze` (looking at the bottom of a page for a minute with no command scrolls nothing), `scroll-mode-job-asks-for-screen`.
- The fixture pages, templates under `scenarios/gaze/_fixtures/`, loaded in the isolated Edge profile WP5 already uses. `dashboard/`: three cards laid out as the owner's screenshots (a line chart, the "Activity by time" heatmap with `role="grid"` and each cell named like "Thursday 4 pm, 93", recent transactions), with `section` landmarks and `aria-label`s so the section rule and the cell names are deterministic. `reading/`: a long article of numbered, labelled paragraphs in one scroll container, so a test can read each paragraph's rect.
- The closed-loop injector. For the scroll scenarios in the desktop lane, the `test-gaze` injector acts as a model eye: every 30 ms it reads the target paragraph's rect over UIA and injects its centre, delayed and with noise. The scenario then checks that the paragraph ends inside the band with no reversal, and that no glide follows the session's end.
- The owner's measurements, recorded in `docs/wp/WP11-gaze.md`: the check's accuracy and precision per route; thirty "enhance" trials on the fixture and on three real apps with the share that picked the intended section; thirty "click that" trials with the share that outlined the intended element on the first try; the scroll trials of 7.9; idle cost of the sidecar and of the vendor's service; the hit test's p95 on Edge, Notepad and Resolve.
- The webcam route. The five signals and the ridge fit over recorded landmark files (JSON lines of a contract type, never images), with fuzz target `gaze_landmarks`; a property test that the leave-one-out penalty is the one the error curve picks; a recorded calibration that replays to the same numbers; a state test that the camera is closed and the model unloaded whenever no session or calibration runs; and the `test-gaze` injector's webcam profile, height only, with a webcam's noise and offset, so the scroll scenarios run on the hosted lanes of both platforms.
- A benchmark, B11, beside B1 to B10: the enhance, click and scroll trials as timed A/B scenarios, on the `devhub` results page.

## 11. Phases and acceptance

1. P0, the spike (pull requests 1 and 2; compiles nothing on the owner's PC; section 17). Acceptance: `docs/research/2026-11-gaze.md` records, for each route, whether it delivers samples to a Win32 process, the check's accuracy and precision on the owner's display, the loop delay tau, idle cost (Eileen's and the vendor's), the scroll-route probe, and the licence findings with quotes and dates; `docs/wp/WP11-gaze.md` exists; the owner picks the route.
2. P1, the foundation (pull requests 3 to 7). Acceptance: the `gaze` module snapshot; `eileen-gaze` passes its filter, limiter, parser and fuzz tests on Linux; invariant 17 parts (a) and (c) hold on the PR seeds; the resolver golden file has every case of 6.5; the four Safety keys need a `UserInput` approval; the registry refuses to start `pc.gaze` without the opt-in; the machine plan lists the sidecar with its reason; the tray shows the indicator.
3. P2, enhance and the controls (pull requests 8 to 11; G1 and G14). Acceptance: on the owner's PC with the chosen route, `enhance-section` passes thirty times out of thirty on the dashboard fixture within the budget of section 9; `enhance-cell-through-view` names the Thursday 4 pm cell; a minimized, cloaked, private or Eileen window is refused with one sentence; "back", the arrow and a physical click close the view, and the source window never gained focus or input (checked by the fixture's own state); the view never appears in `desktop_screenshot`; "stop watching my eyes" turns the indicator off within one HUD update; "check my eyes" writes `check.toml` and "forget my gaze check" removes it; the reticle, panel and indicator draw-list snapshots exist; the `test-gaze` injector drives the same scenarios on the hosted lane with no tracker.
4. P3, the commands (pull requests 12 and 13; G2 to G13). Acceptance: the P3 scenarios pass; a gazed Send button still asks; "click that" on the HUD is refused; "that one" never answers a Confirm panel or a route proposal; the addressee feature's scenario shows at least nine of ten unnamed requests to the screen passing with a look at the HUD, and room talk with a look at the screen still greyed; the accessibility test's gaze column is complete; the ledger's dwell rows exist only with `gaze.ledger` on and never for a route that `stores_nothing`.
5. P4, scrolling by gaze, experimental (pull requests 14 to 17; G15; after P2's hit test, beside P3). Acceptance: the law's table and closed-loop tests pass; invariant 17 part (b) holds on the PR seeds; every `scroll-mode-` scenario passes in simulation and, with the closed-loop injector, in the desktop lane; physical input ends a session within one event, and a glide with no renewal ends within 250 ms; workers, replays and the brain are refused a `glide`; the owner has run the trials of 7.9 and recorded them. Dropping "experimental" is a separate owner decision on those results. The webcam route (pull requests 18 to 22) runs beside P4 and is accepted by 18.9.

## 12. Proposed work package: WP11, Gaze

For BUILD.md as section 6.13, in the form of 6.2 to 6.12.

- Branch `wp11/<topic>` for WP11's own files and `amend/<topic>` for amendments.
- Depends on WP0 phase A (the traits), WP1 (the core and the simulator), WP2 (the dispatcher and the sidecar client), WP5 (`desktop_hit_test`, the background scroll routes and `Glide` land in its crate), WP7 (the `Gaze` slot in the resolver, the `[gaze]` settings), WP8 (the panel, the reticle, the band, `GazeOver`). P0 depends on nothing.
- Desktop yes: the owner's PC with the chosen tracker for P2 and later; the hosted lane through the `test-gaze` injector; a Linux desktop with a webcam for `eileen-gaze-linux`.
- Wave after W6, as v0.2's first package, so it never shares the desktop with another package; P0 at any time, since it compiles nothing. One slot; its builds are small.
- Owns `crates/eileen-gaze`, `crates/eileen-gaze-win`, the `gaze` contracts module, `scenarios/gaze/` (its `_fixtures/` included), the `fuzz` target `gaze_samples` with its corpus, `docs/GAZE.md`, `docs/wp/WP11-gaze.md`, `docs/research/2026-11-gaze.md`. WP10 builds `crates/eileen-gaze-linux` (section 18.6) under this plan, as it builds every `-linux` crate.
- Does not own the platform traits, the other contract modules, `eileen-desktop` (WP5), `eileen-hud` (WP8), `eileen-intent`, `eileen-config` and `eileen-learn` (WP7), `eileen-core` (WP1), or the runtime and the tray (WP2). Every change to them is an amendment that package reviews.
- Builds sections 6, 7, 8 and 18 of `docs/GAZE.md`: the sidecar and connector, the routes behind `GazeSource`, the fixation and track filters and the rate limiter, the nine-point check, the core's gaze ring and onset rule, the `Gaze` slot, `desktop_hit_test` and its rules, the enhance view and the reticle, the grammar entries of section 5, the `[desktop]` gaze line and `LookTarget::Gaze`, the `[gaze]` settings, invariant 17, the experimental scroll mode with its law, routes, glide and band, the scenarios, the fixture pages, B11.
- Acceptance the phase lists of section 11.

## 13. Proposed decision record

For `docs/DECISIONS.md`, at the next free number. That was D81 on 2026-10-26, but `docs/wp/WP10-linux.md` queues thirteen unnumbered records that may be numbered first.

- Date: 2026-10-26
- Status: proposed (not yet confirmed by the owner)
- Amends: D41 (its rule that clicks and typing stay with workers: the fast path may click and focus a gazed element, as an `Act` job with that tool's own tier)
- Decision: Eye tracking enters Eileen through a hardware tracker or, for the scroll mode, a webcam, as a slot, never a trigger. A room-sensing connector, `pc.gaze`, runs in a supervised sidecar behind the `GazeSource` platform trait. It turns the tracker's samples into fixations and sends only those to the core, at most ten a second. No image of the user leaves the sidecar, and every gaze point is personal data. The core snapshots the fixation current at speech onset for each addressed turn, and resolves "this", "that", "here" and "there" through a `Gaze` slot and one hit test in the desktop server, which D21's server gains. A gaze-resolved action is dispatched like any other, with its tool's tier, taint and journal. The "enhance" view is a clipped DWM thumbnail the HUD draws, touching no app. No approval, answer, mute change, job, setting, consent or first-run choice is ever built from gaze, and gaze alone never addresses a turn; simulator invariant 17 checks both. An experimental scroll mode, off by default, is the one continuous use. Only an addressed turn or a hotkey opens it, and it scrolls only the pane it opened on. Its speed is proportional to the gaze's distance from a set point, with the gain capped by the loop delay measured on the PC. It stops when the gaze returns to a band around the point, and ends at a stop word, physical input or when the eyes leave the pane. The connector is opt-in through a Safety key, local only, and visible on the HUD and the tray whenever the tracker is live. A route whose terms allow live use only keeps every stored use off.
- Why: The owner's "enhance": look at a part of a dashboard, say the word, and it fills the screen, with the eyes still tracked inside the view. The same slot removes the mouse from the commands that only needed a target, and the scroll mode removes it from reading, which serves the accessibility goal of OWNER-DIRECTION.md. Deterministic first keeps it on the fast path. "A slot, never a trigger" keeps the approval invariant intact and avoids the Midas touch problem. A hardware tracker gives the accuracy the targeting use cases need. The webcam route needs no new hardware and is the only route on Linux; it serves the scroll mode because that loop needs precision more than accuracy (7.4).
- Consequences: WP11 (BUILD.md 6.13) builds it after W6, with the P0 spike first. The contracts gain a `gaze` module and additions to nine other modules, none at the top level. `eileen-desktop` gains `desktop_hit_test`, background scroll routes and `Glide`. A new dispatcher rule lets the fast path issue `desktop_click_element` and `desktop_scroll` for gaze commands. The machine profile treats the sidecar as an optional resident. A new repository check forbids image encoders in the gaze crates. ROADMAP's v0.2 table gains a row. The webcam route serves the scroll mode on Windows and Linux, with its camera and model alive only while a session runs, and WP10 builds its Linux crate.
- Source: `docs/GAZE.md`; `docs/research/2026-11-gaze.md` once P0 has run; BUILD.md 2.6, 2.8, 2.9, 2.12, 2.14, 3.3.9, 3.3.10, 3.4; OWNER-DIRECTION.md, "Accessibility as a first-class goal".

## 14. Amendments this needs

Pull request 3 makes the document changes; pull request 4 makes the contract changes it describes; the crate rows of `ARCHITECTURE.md`'s code map arrive with the pull requests that create the crates (5 and 10), so check 3 holds at every commit.

| File | Section | Change |
|---|---|---|
| BUILD.md | 1 | E20, "Enhance": the table of section 4 in E-table form, with one line on the scroll mode |
| BUILD.md | 2.1 | The `eileen-gaze.exe` row of 6.1 in the processes table and the diagram |
| BUILD.md | 2.3 | The `[desktop]` block's `gaze:` line and its caps |
| BUILD.md | 2.4 | The grammar entries of section 5 under built-in grammar; the `Gaze` slot resolver beside the others |
| BUILD.md | 2.6 | `desktop_hit_test` as the nineteenth tool; the section and scroll-container rules; the background scroll routes; `Glide` with its clamps and server-side stops; visible intent once per scroll session; an open session counts as the user being active for the lease rule; the eye-scroll hotkey among the refused combos |
| BUILD.md | 2.8 | Rule 5 gains the one-point `gaze_on_eileen` feature (G10), which never passes alone |
| BUILD.md | 2.9 | The reticle, the enhance panel and `CloseEnhance`, the scroll band, `GazeOver` and its handling, the indicator, the accessibility table's gaze column |
| BUILD.md | 2.10 | Gaze commands as fast-path jobs that are never distilled; a scroll session journaled once; the ledger's `Dwell` row |
| BUILD.md | 2.11 | The four Safety keys, by name; `hotkeys.gaze_scroll` |
| BUILD.md | 2.12 | A row in the observed-read table for `desktop_hit_test` (ByTarget) and `desktop_look` with `Gaze` or `Region` (Screen); "what gaze never does" under approvals; the new dispatcher rule for origin `FastPath` and for `Glide` |
| BUILD.md | 2.14 | `pc.gaze` as a room-sensing connector; `stores_nothing` |
| BUILD.md | 2.15 | `gaze\check.toml` in the data directory |
| BUILD.md | 2.16 | The gaze sidecar as an optional resident |
| BUILD.md | 3.1 | `eileen-gaze`, `eileen-gaze-win` and `eileen-gaze-linux` in the crate list; `scenarios/gaze/` in the directory table |
| BUILD.md | 3.3 | A new 3.3.18 with section 8's types; the additions to 3.3.5, 3.3.6, 3.3.9, 3.3.10, 3.3.11, 3.3.12, 3.3.13, 3.3.14 |
| BUILD.md | 3.4 | `GazeSource`; `Automation::ancestors`; `HudSurface::set_enhance` and the clip |
| BUILD.md | 4.1 | The `windows` crate's feature for `Windows.Devices.Input.Preview`, if P0 picks that route; its Media Foundation features for the webcam route; the face landmark model's pin with its terms (18.7) |
| BUILD.md | 4.3, 4.4, 4.5 | The rows of section 9: latency, the sidecar's memory, its idle cost |
| BUILD.md | 4.7 | Invariant 17 in three parts; the gaze scenario steps and the simulated page; the `gaze_samples` row of the fuzz table; B11 |
| BUILD.md | 4b.3 | A new check: no image encoder in `eileen-gaze`, `eileen-gaze-win` or `eileen-gaze-linux`. The check count becomes thirty-one (AGENTS.md and CONTRIBUTING.md say "thirty"; their text follows as a guidance change) |
| BUILD.md | 6.13 | Section 12 |
| BUILD.md | 7 | The risks of section 15 under open risks |
| BUILD.md | 8 | A revision-log line |
| DECISIONS.md | end | The record of section 13, at the next free number |
| PLAYBOOK.md | as needed | What the record changes, or the reason it changes nothing, under the pull request's "Playbook" heading |
| OWNER-DIRECTION.md | Proposed 2026-10-26 | The proposal in one entry, with the owner's answers to section 16 |
| ROADMAP.md | v0.2 | "Gaze: a hardware eye tracker as a slot for 'this', 'that' and 'enhance', and an experimental scroll by gaze, also from a webcam on Windows and Linux; WP11" |
| ARCHITECTURE.md | Processes, Connectors | The sidecar row; a sentence under "Connectors" |
| `Cargo.toml` | the `windows` feature list | The gaze feature, if P0 picks the Windows route |

## 15. Risks

- No tracker on the owner's PC today. The spike needs one; the owner picks it (section 16, question 1). A tracker mounts on a monitor of 15 to 27 inches, and a laptop screen needs a mount [to confirm for the owner's laptop].
- Windows gaze input from a Win32 process is unverified. If the API needs a view or a package identity, the sidecar hosts a hidden core window or gets a sparse package, or the vendor route is first. P0, question 1.
- Vendor terms and hard rule 4. Interaction against analytical use, redistribution and scope are read from the current licence text, quoted in the report, and settle `stores_nothing` and the install route. The licence page found while writing this describes the older EyeX SDK. Loading a proprietary vendor library inside an Eileen process may sit badly with hard rule 4 (open question 8).
- Chromium's accessibility cost. `ElementFromPoint` on web content needs the on-demand request WP5 measures; if it is too costly on this PC, the hit test falls back to `want: Window` for browsers and the section is the quarter around the point, which still enhances. The scroll mode then uses the posted-wheel route on the window.
- DWM thumbnail limits. Minimized, cloaked and DRM-protected windows render nothing; some DirectComposition apps may render black [to confirm]. Enhance should say so in one sentence rather than show a black view; how to tell in advance is question 4 of section 17.
- The onset rule picks the wrong fixation sometimes. People glance at the HUD, at the keyboard or at the next thing before they finish speaking. The trials of section 10 measure it, and `onset_window_ms` and "the longest fixation before the glance" are the levers; the fallback is always one short question.
- The scroll loop hunts. If the real loop delay is longer than measured, the page overshoots and reverses. The gain is capped from the measured delay, the reversal guard halves it within a session, and the trials count reversals.
- Read-along surprises. Reading down a paragraph may start motion the reader did not want. The start band, the resume dwell and the one-shot arm of the trials address it, and "stop" always ends it.
- Panes the mode cannot reach. Apps with no Scroll pattern that ignore posted wheel messages are refused rather than driven by moving the cursor.
- Glide jitter. Renewals every 100 ms on a loaded PC may arrive late; the 250 ms lifetime absorbs one late renewal, and P0 measures how smooth the motion is.
- Motion discomfort. Continuous scrolling can tire the eyes. The speed cap, the speed-up limit and the session limit bound it, and the comfort rating in the trials watches it.
- Privacy perception. A device watching the user's eyes worries people more than a microphone. The indicator that cannot be hidden, "stop watching my eyes", the no-image rule with its repository check, and the opt-in Safety key are the answer; the reticle shows the user what Eileen knows and nothing more.
- Multi-monitor and mixed DPI are untested on the owner's single monitor, as for the desktop server; the fleet covers them.
- The webcam route's accuracy depends on the light, glasses, the camera's height and the user's eyes; the prototype's author noted it may work less well for narrower eyes. The calibration's held-out error says how it does on each PC, and above the 15 percent rule of 7.3 Eileen offers "scroll down" instead.
- The webcam route's CPU cost on the owner's PC. The model runs on every frame of a session. If P0 measures too much, the route runs at 15 frames a second, or the machine profile keeps it off on that PC.
- A camera that another app holds (a call) may be unavailable. The session then refuses with one line.
- A camera worries people more than a tracker does. The rules of 18.5 are the answer: frames never leave the sidecar, the camera is open only during a session or the calibration, and the indicator shows whenever it is.

## 16. Open questions for the owner

1. Which tracker for the spike: a Tobii Eye Tracker 5, or another tracker that feeds Windows gaze input?
2. Placement: v0.2 after W6 as proposed, when most of the touched contract modules are pinned and the contract changes are an amendment, or P0 and P1 earlier, while those modules are still provisional?
3. The dwell ledger (G12): keep it in the design as an opt-in, or leave it out entirely?
4. The reticle: on for the first week after enabling, as proposed, or off until asked?
5. The word: "enhance" as the canonical entry, with "zoom in on that" and "make that bigger" as phrasings, or another word?
6. The scroll mode's start: by voice ("scroll with my eyes"), by the hotkey, or both?
7. After the text settles: keep following as the user reads on (read-along), or end the session (one-shot)?
8. The vendor route loads the vendor's library inside `eileen-gaze.exe`. Hard rule 4 says to talk to tools under conflicting licences over IPC or to spawn them. Is a separate sidecar process enough, or should WP11 use the Windows gaze input route only?
9. The webcam route (section 18): build it, so the scroll mode works without a tracker and on Linux, or leave it out?
10. If it is built: on Windows and Linux, or on Linux only?

Per the standing rule of BUILD.md section 7, the recommendation applies unless the owner says otherwise: a Tobii Eye Tracker 5; v0.2 after W6; keep the ledger as an opt-in; the reticle on for a week; "enhance"; the scroll mode by voice, with the hotkey as an option; read-along, with the one-shot as a trial arm; the Windows gaze input route first, asking again only if P0 shows it cannot reach a Win32 process; and the webcam route on both platforms, for the scroll mode only.

## 17. The P0 spike: what to confirm

A research report, `docs/research/2026-11-gaze.md`, in the form of the October reports, with dates and quotes. Scripts and fixtures under `docs/research/fixtures/gaze/`, never in a crate.

1. Windows gaze input. Whether `Windows.Devices.Input.Preview` delivers samples to a Win32 process on the owner's Windows 11 build with the chosen tracker, in which coordinates and at what rate; whether it needs a view, a package identity or only a watcher; what the consent prompt looks like.
2. The vendor runtime. The current licence text of the consumer runtime: interaction against analytical use, redistribution, scope; whether its library can be loaded at run time from the user's own install, or reached over its own service; whether its calibration can be opened from another app.
3. Accuracy, precision and drift. The nine-point check on the owner's display at the measured viewing distance, in daylight and lamp light, with and without glasses, on each route; repeated after ten minutes to see drift.
4. DWM thumbnails with a source clip on a layered, no-activate overlay: that `rcSource` clips as expected at 150 percent DPI, that covered windows render, and what minimized, cloaked, DRM and DirectComposition windows render, with screenshots in the report (of fixtures only).
5. The hit test's cost on Edge, Notepad, Explorer and Resolve: `WindowFromPoint`, `ElementFromPoint`, ancestors, with and without the Chromium accessibility request, in the numbers WP5's probe uses.
6. The onset rule, by hand: twenty "enhance" trials on the fixture page, logging the fixation at onset and the intended card, to set `onset_window_ms` before P2.
7. Degrees to pixels on the owner's display at the measured viewing distance, so the defaults of 6.3, 7.3 and 9 are this PC's numbers.
8. The scroll loop delay tau. The software part exactly, from trace spans (sample received, track sent, glide issued, glide applied); the tracker's latency from its own timestamps where the route gives them, else from a step test on a jumping target; the eye's lag following text that moves at 300, 600 and 900 px/s. The sum sets the default gain.
9. The scroll routes. Which panes in Edge (web content and a PDF), Notepad, Explorer and VS Code expose a vertically scrollable Scroll pattern; how smoothly each repaints at 30 and 60 pattern calls a second; which take posted wheel messages at a point without the cursor there; how each treats fractional wheel deltas; and how smooth a glide renewed every 100 ms looks under load.
10. Idle cost, Eileen's sidecar and the vendor's service separately, against the machine profile's usual free memory.
11. The webcam route, on the owner's PC and on a Linux laptop: the face landmark model's terms and an ONNX export of it; its CPU and memory through `ort` at 15 and 30 frames a second; the camera's frame time; the calibration's held-out error and the jitter before smoothing, in daylight and lamp light, with and without glasses, against the prototype's 18.5 and 6.2 percent of the screen height; and arm F of the scroll trials.

## 18. The webcam route

### 18.1 Why

Most PCs have a webcam and no eye tracker. On Linux no consumer tracker has a supported runtime for interactive use: Tobii lists its consumer trackers (the 4C, 5 and 5L) as never supported by its [Pro SDK](https://developer.tobiipro.com/tobiiprosdk/eyetrackercompatibility.html), the one Tobii SDK that names Linux, and that SDK is meant for analysis. A community driver for the Eye Tracker 5, [tobiifree](https://github.com/Aetherall/tobiifree), works on Linux and Wayland, but it calls itself experimental. A webcam is therefore the only gaze source Eileen can count on for Linux, and the only one on a Windows PC without a tracker.

The route follows a working prototype. Aaron's "Gaze Zone Scroller" (7.10) scrolls a page by gaze from an ordinary 640p webcam: a face landmark model, a ten-target calibration fitted by ridge regression, and a band the user sets, inside which nothing moves. This section builds the route on the prototype's design and numbers.

### 18.2 What it serves, and what it does not

A webcam estimate is coarse. The prototype's calibration missed its held-out targets by 18.5 percent of the screen height (about 168 px), against the 60 px a section needs (principle 6). So the route offers no gaze targets: "enhance", "click that", "close this" and the other commands of section 5 fall back exactly as with no tracker (principle 5). It serves the scroll mode (G15) and the calibration and pause commands of G14, because the scroll loop needs precision more than accuracy (7.4): the anchor is set by the same estimate, so a constant error moves the anchor and the gaze together, and the text still settles where the eyes were.

The first version estimates height only, as the prototype does. Its `GazeSourceInfo.axes` is `Vertical`; the x of its points means nothing and is never hit-tested. A session therefore opens on the scroll container under the mouse pointer when the pointer is over the foreground window, and otherwise at the centre of the foreground window's client area (`desktop_hit_test { want: ScrollContainer }` at that point), never by gaze. If a later check measures a webcam route under `gaze.max_accuracy_px` on both axes, principle 6 offers its targets as it does any tracker's.

### 18.3 How it works

| Stage | What happens |
|---|---|
| Camera | Frames from the PC's camera at 30 a second, 640 by 480 or the nearest mode the camera offers, uncompressed where it can (YUYV or NV12), so nothing is decoded or encoded |
| Face | A face landmark model gives the eye and iris landmarks and each eye's look-up and look-down scores. Faces are found by YuNet, which Eileen already ships (BUILD.md 2.7), or by the landmark model's own detector [to confirm in P0] |
| Signals | Five per frame, from the prototype: the iris height between the eyelids, the eyelid opening, the look-down and look-up scores, the head's pitch, and the face's height in the frame |
| Estimate | The user's own fit of the five signals to the screen height (18.4) |
| Track | The estimate through the track filter of 6.3, sent as `Track` ten times a second while a session runs |

```toml
[gaze.webcam]                 # Low unless noted
camera = "auto"               # auto (the system's default camera) or a device name
fps = 30                      # 15 to 30
```

### 18.4 Calibration

With the webcam route, "calibrate my eyes" runs Eileen's own calibration, since there is no vendor app. A dot visits ten places at five heights, about twelve seconds in all. At each, the route waits about half a second for the eyes to settle, then records the signals for about two thirds of a second. A ridge regression maps the signals to the screen height; the penalty matters because several of the signals rise and fall together, and plain least squares would balance large positive and negative weights between them. The penalty is chosen by leaving one target out at a time and fitting the other nine, and the error on the left-out targets is the accuracy the check reports, so it says how well the fit predicts places it has not seen.

The prototype reported 18.5 percent of the screen height on held-out targets and 6.2 percent frame-to-frame jitter before smoothing, which the track filter reduces. The fit is stored as numbers only (the weights, the penalty and the held-out error) in `gaze\webcam.toml`; "forget my gaze check" deletes it.

The fit holds while the head stays roughly where it was. A lean or a slouch shifts the estimate, mostly by an offset. Inside a session, "set the line here" re-anchors (7.5), which removes the offset, as the prototype's re-align key does.

### 18.5 Privacy

- Frames live only in `eileen-gaze.exe`'s memory. Each becomes landmarks and is dropped. No frame or crop is sent to another process, written to disk or encoded, calibration and incidents included, and the image-encoder check of section 14 covers the webcam code.
- The camera is open only while a scroll session or the calibration runs, and closes when it ends. Outside those, the route holds no camera and no model. A state test on the sidecar checks it.
- The camera's own light and the `gaze_live` indicator show whenever the camera is open. The operating system's camera privacy setting and consent apply: Windows' camera access setting, and on Linux the camera portal's permission when Eileen runs sandboxed.
- Landmarks are personal data, like points, so the `pii.` policy covers any that a recorded test file holds; shipped binaries never write them.

### 18.6 Platforms

| Platform | Camera | Crate |
|---|---|---|
| Windows | Media Foundation's source reader through the `windows` crate [to confirm the features] | `eileen-gaze-win` |
| Linux | Video4Linux2 through `rustix`, already pinned for Linux, on a desktop session, where the logged-in user may open the camera device. When Eileen runs sandboxed (L3), the camera portal instead: `org.freedesktop.portal.Camera`'s `AccessCamera`, then `OpenPipeWireRemote` and a PipeWire stream, which needs a PipeWire client and its pin [to confirm] | `eileen-gaze-linux`, built by WP10 as every `-linux` crate is |

The landmark model, the signals, the fit and the filter are platform-neutral, in `eileen-gaze`, tested on Linux over recorded landmark files. On Linux the scroll mode's other half, moving the pane, is WP10's desktop backend: AT-SPI2's scroll actions, or wheel events through the RemoteDesktop portal's input that LINUX.md section 4 plans [to confirm in P0]. LINUX.md gains a gaze row through WP10's own docs pull request.

### 18.7 The model

The prototype uses MediaPipe's Face Landmarker: 478 landmarks with the irises, and 52 blendshape scores that include each eye's look-down and look-up. MediaPipe's code is Apache-2.0; the model files' own terms are read from their model cards in P0 [to confirm]. The route runs the model through `ort` on the ONNX Runtime Eileen already ships (1.28.2, BUILD.md 2.8), as YuNet already runs in `eileen-apps.exe` (2.7), from an ONNX export whose making P0 records [to confirm, and how the Linux build ships ONNX Runtime]. If the terms or the export fail, another landmark model with permissive terms takes its place, and the five signals stay.

This route trains nothing on other people's gaze. The only fit is the user's own calibration, so the worry about gaze models trained on datasets whose terms allow research only does not apply to it.

### 18.8 Budgets

| Stage | Budget [to confirm in P0] |
|---|---|
| Memory while a session or the calibration runs | under 80 MB for the camera buffers, the model and its runtime |
| Memory and CPU while no session runs | none: no camera, no model |
| CPU while a session runs | the landmark model once per frame, on the CPU. P0 measures it at 15 and 30 frames a second on the owner's PC and on a Linux laptop; the prototype ran at 57 frames a second on a browser's GPU path |
| Frame to `Track` | one frame period (33 ms at 30 a second) plus the model's time. It adds to tau, so the gain cap of 7.4 follows it with no change |
| Calibration | about 12 s |

### 18.9 Acceptance

- Arm F of the trials of 7.9 meets the same bar as the tracker arms, or the route stays experimental with the mode.
- On Windows and on Linux, the scroll scenarios of P4 pass with the `test-gaze` injector's webcam profile, and the state test shows the camera closed whenever no session or calibration runs.
- The image-encoder check covers `eileen-gaze-linux` and the webcam code in `eileen-gaze-win`.
