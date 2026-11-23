# WP1: Core logic, replay and simulation

The living plan for work package 1 of [BUILD.md](../BUILD.md) section 6.3: the core, the recording
format, the replayer and the simulator. The agent doing the work keeps it current.

## Goal

The core as a pure state machine, `Core::step` of section 3.3.3, with the recording format, the
replayer and the deterministic simulator that test it.

The first pull request is a minimal core that WP2 can wire for slice S1: turn-taking, barge-in
with pause-then-decide, the addressee gate with the name and the follow-up window, fast-path
dispatch through the `IntentResolver` port (a scripted fake), one brain turn, one job, mute and
completion announcements, with simulation tests that include "stop" while Eileen speaks.

The first follow-up, `wp1/speech-life`, adds the speech process's life (start, a missed heartbeat
or a late `Ready`, an exit, the backoff, and replies on the HUD while speech is down or the
microphone is muted) and merges before WP2's `wp2/speech-transport` (D83). The rule for turns
heard late follows in `wp1/speech-late` once WP4's first part adds `SpeechStarted.replayed`.

Then the full core (the addressee gate of section 2.8, intent dispatch, the fact path, `Act` jobs,
the brain turn, jobs, sessions and the warm pool, memory admission, the permission gate, approvals,
announcements, input leases, the journal, the settings flow, the regret window, forget, route
proposals and the first-run state machine), `eileen-record`, `eileen-replay` with strict and
scenario modes, and `eileen-sim` with the world of section 4.7, its eight profiles and all 16
invariants. The machine profile's core part and the speaker gate wait for WP0's
`wp0/machine-contracts`; the D77 parts wait for WP7's amendment of section 3.3.13.

## Branch, dependencies and files

- Branch: `wp1/<topic>`.
- Depends on: WP0 phase A. Wave: W1. Desktop: no.
- Owns: `crates/eileen-core`, `crates/eileen-record`, `crates/eileen-replay`, `crates/eileen-sim`,
  `scenarios/feel/`, `scenarios/jobs/`, `scenarios/first-run/`, `scenarios/machine/`, the fuzz
  targets `recording`, `expected_trace`, `sim` and `shell_command` with their corpora and
  regressions, and this file.

## Done when

The acceptance list of BUILD.md section 6.3, items 1 to 17, each recorded under Progress with its
evidence when it holds. The lines below are reminders; the full text in BUILD.md is the bar.

1. All 16 invariants hold on the PR seed set and on 30 minutes of random seeds per profile, each
   proved by a hand-written scenario that breaks a fake.
2. `turn::exhaustive` passes at depth 6 within the PR budget, and at depth 8 nightly.
3. A proptest-state-machine reference model of the job lifecycle agrees with the core
   (`tests/it/job_model.rs`).
4. Barge-in in simulation: onset to pause within 96 ms of virtual time plus one callback, the
   right heard prefix, and resume after a backchannel.
5. Approval safety: text from a tool result, a worker, `eileen say` or a video never approves; a
   tier-2 call needs the action word in the window, physical input or a matching pending ask.
6. Turn authority and the permission table: the `claude_tool_tier`, git, `url_vouched` and
   `observed_source` table tests, and `shell_command` fuzzing clean for 10 minutes.
7. Two speeds: a deep answer that arrives while the user talks goes to the HUD only; one that
   arrives in silence is spoken once.
8. Memory admission: a queued agent's spoken reason, the paging rule, "start it anyway", the 1 GB
   commit floor, the drop order and the per-job memory limit.
9. A recorded run replays in strict mode with no divergence, a router change reports the first
   divergent event, and scenario mode survives an extra minted ID.
10. `scenarios/feel/`, `scenarios/jobs/` and `scenarios/first-run/` replay green.
11. `eileen-core` has no build dependencies, and in its normal dependencies depends, of
    Eileen's crates, only on `eileen-contracts`, `eileen-stdx` and `eileen-trace`, and of
    third-party crates only on `tracing` and `serde_json` (check 24, D84).
12. The learning decisions in the core, in simulation: facts and their retirement, aliases,
    exact repeats, `rung_skipped`, forget, the `[match]` block, follow-ups, speech levels and calls.
13. Scenario mode: timers and playback synthesized, brain turns keyed by transcript digest and
    occurrence, hooks replayed in sequence, an unanswered effect reported rather than hung on.
14. The reserved job kind: a `jobs_dispatch` of kind `ml_project` is refused with
    `JobKindUnavailable` before routing and admission.
15. The speaker gate never refuses for voice without a passing `SpeakerBar`, stays in shadow until
    300 clean turns, and turns itself off after three false rejects.
16. The machine plan, table-tested over WP0's three fixture machines and in simulation, with
    `scenarios/machine/` green.
17. Notes and receipts (D77): the `Notes` rows of `claude_tool_tier`, `url_vouched`, the `[notes]`
    block, forget recycling, induced-template `[match]` blocks and receipts.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

- `wp1/minimal-core` (#130, merged): the minimal core slice S1 wires. `eileen-core` gains `turn` (a
  pure, cloneable turn-taking machine), barge-in with pause-then-decide, the addressee gate's
  rules 1, 3 and 4 in minimal form, fast-path dispatch through `IntentResolver::resolve` with
  mute, unmute, stop, cancel, "go on" and "what time is it" answered by the core, one brain turn
  spoken clause by clause, one job with a cold worker session and fail-closed hook answers,
  mute, the attributed completion announcement, its spans, and `Core::snapshot` from the live
  state. `eileen-sim` gains the parts that answer the core (the speech process, the playback
  device in 10 ms callbacks, a scripted brain and scripted workers), `SimResolver`, timers drawn
  from the world's pending timers, an incremental span collector for invariant 13, and the
  slice tests `eileen-sim::it::slice::*`. Acceptance 4 (barge-in in simulation) is shown by
  `stop_while_eileen_speaks_ducks_pauses_and_stops_within_the_decision` and
  `a_backchannel_resumes_from_the_paused_sample`; the exhaustive enumeration
  (`wp1/turn-exhaustive`) builds on `turn::Turns` next. "What time is it" speaks the offset of
  `CoreConfig.local_time` in force at the turn's wall time (section 3.3.3 as #134 amended it),
  shown across the United States' daylight-saving changes of 2026 by
  `eileen-core::clock::tests::the_hour_moves_at_a_daylight_saving_change`. The crate keeps
  `tracing` and `serde_json` as its only third-party dependencies and has no build dependencies
  (check 24 as #134 amended it, D84). It follows #134 and builds on it.
- `wp1/speech-life` (#141, merged): the core's half of the speech process's life (section 6.3 as #134
  and #146 amended it, D81, D83), which merges before WP2's `wp2/speech-transport`. `eileen-core` gains
  `life`: `Sidecar(Speech, Start)` on the first step; `HangDumpThenKill` then `Restart` when no
  `Ready` comes within 120 s of a `Start` or `Restart`, or no `Heartbeat` for 5 s from that
  `Ready` or the latest `Heartbeat`; `Restart` alone after an `Exited`; after a kill, `Restart`
  waits for the step and for the killed process's `Exited`, but for that `Exited` at most 10 s
  from the kill (`EXIT_WAIT_NS`); each `Restart` after a
  backoff step of 1 s doubling to 60 s, back to 1 s once the process stayed up a minute after
  its `Ready`; and `SidecarRestarted` on the bus with the reason. It gains `hud`: the HUD state
  with `MicState::LoadingSpeech`, `SpeechRestarting`, `Muted` or `Listening`, sent when it
  changes, and the line that carries every reply Eileen cannot say aloud (speech down, the
  microphone muted, a `SpeakDone` with no clause, the part of an utterance not synthesized when
  speech went down, what the user had not heard of an utterance muting stopped, and the rest of
  a brain reply after any of these), at most 4 KiB, added to only by the message it belongs to.
  A turn open when speech goes down is dropped and its late `EndOfTurn` never routed, and
  announcements wait while Eileen cannot speak. `eileen-sim`'s speech
  part answers `Start` and `Restart` with `Ready` after a 2 s load and pings with a `Heartbeat`
  every second, and takes injected hangs, crashes and loads that never finish; invariant 8,
  `children_restart_with_bounded_backoff`, is checked after every step. Tests:
  `eileen-core::life::tests::*` (the backoff sequence), `eileen-core::tests::life::*` (the two
  deadlines, an exit, the backoff's reset, replies on the HUD), `eileen-core::turn::tests::*`
  (`speech_lost`), `eileen-sim::invariants::tests::invariant_8_*` and five
  `eileen-sim::it::slice` tests plus a load that never finishes and a crash mid-answer.
- `wp1/turn-exhaustive` (#149, merged): acceptance 2 and 4. `eileen-sim::exhaustive`
  enumerates every sequence of section 4.7's eleven turn-taking inputs (speech start, speech end, end of turn,
  clause ready, playback tick, mute, unmute, brain text, backchannel, other audio, physical
  input) through the real `eileen_core::turn::Turns`, to depth 6 on pull requests, from five
  starts: idle, speaking the first clause of a streaming reply, paused by a barge-in, and two
  that run the clock to just before the 600 ms decision deadline (a pause, and a duck that
  never paused), so the timer's resume and unduck are enumerated. A bench plays the core's
  part around `Turns` for these inputs and the parts that answer it (a device that takes 32 ms
  of samples a tick and reports its position, a speech process that readies a clause at a
  time, the user's speech as one barge-in window a tick while captured, the decision timer at
  its own instant inside a tick), and checks invariants 2, 3, 9 and 10 after every input.
  States are pruned by a 64-bit FNV-1a hash of everything that can change what follows, with
  times taken as ages, which is why `Turns` now derives `Hash`. The search returns the facts
  it reached (phases, duck, pause, mute, capture off, a mute mid-utterance, a reply on the HUD,
  a callback across clauses, an utterance spoken to the end, a heard prefix told, a
  backchannel resume, and both deadlines), and `it::turn::exhaustive` asserts every one is
  reached and pins each start's state and input counts at depth 6. A broken machine (no duck,
  no pause, the whole reply told to the brain, capture left on when muted, a mute ignored, an
  utterance over another) is caught with a shortest counterexample
  (`eileen-sim::exhaustive::tests::*`). Invariants 2 (`barge_in_pauses_in_time`), 3
  (`heard_prefix_matches_playback`) and 9 (`nothing_sounds_while_muted`, its mute and
  microphone parts) are named functions in `eileen-sim::invariants`, each a watch the
  enumeration carries in its states, with hand-built violating traces
  (`invariants::tests::audio::*`); invariant 10 runs on the same watch as before. Invariant
  2 measures onset to silence from the user's `SpeechStarted` and judges a pause's 600 ms at
  every record, its resume included; invariant 9 learns a mute from the user's request as well
  as the core's announcement and wants capture off within one 10 ms frame. `Turns::set_mute`
  now announces an unmute before its earcon, so no earcon sounds inside a mute. A world keeps
  an audio trace (inputs, effects, and each callback's samples while the device holds an
  utterance and the first callback after) only when a test asks (`World::record_audio`), and `eileen-sim::it::barge_in`
  shows acceptance 4 through the real core: onset to silence within 96 ms plus one callback,
  the heard prefix the brain is told equal to the scripted sentence cut at the samples handed
  to the device, and a backchannel that resumes from the exact paused sample, with invariants
  2, 3 and 9 holding over each trace. It also asserts two gaps as they are today, with
  `FIXME`: a stop with no pause first tells a prefix a character short (#151), and Eileen keeps
  speaking while another app uses the microphone (#152). The nightly hook: `nightly.yml` is not
  on `main` yet, so WP0 phase B's nightly job runs
  `EILEEN_EXHAUSTIVE_DEPTH=8 cargo nextest run -p eileen-sim --profile nightly -E 'test(/turn::exhaustive/)'`;
  the test reads the variable (`eileen_sim::exhaustive::DEPTH_ENV`) and runs depth 6 without
  it.
- `wp1/record` (#155, merged): `eileen-record`'s formats, store, reader, writer and trim
  (section 3.3.16). `line::encode` and `line::decode` write any contract value as one line of JSON with each
  string over 1 KiB in the blob store, through `record::stub_blobs` and `resolve_blobs`;
  `writer::Writer` and `reader::Reader` write and read a recording's `RecordLine`s, each line
  bounded at 1 MiB, a cut-off last line reported as `Truncated`, CRLF endings and blank lines
  read. `blob::BlobStore` keeps `blobs/<2 hex>/<hash>.zst`, named by BLAKE3 (pure) and
  zstd-compressed, with a `<hash>.refs` count beside it: one reference per stub a writer wrote,
  and `reader::release_blobs` releases a deleted recording's references, deleting each blob
  with its last, reading the whole recording before it releases anything. The store takes an
  exclusive lock on `blobs/.lock`, syncs each file before renaming it into place, rewrites a
  blob that does not hash to its name on the next put of its bytes, and reads a damaged count
  as `blob::UNKNOWN_COUNT`. A line that fails to write, is nested past `line::MAX_DEPTH` or is
  refused by the output part way keeps no reference the file does not hold. `trim::trim` keeps
  the inputs of the chosen turns and jobs (`trim::Selection`), the latest setup input of each
  kind before them (`trim::setup_kind`), the unclaimed setup inputs among them and each kept
  hook input's answer. Tests: `eileen-record::it::round_trip::*` (property tests through the disk store, the
  crate doc's promised inverse), `eileen-record::it::trim::*`, `eileen-record::it::blobs::*`
  (the reference count), `eileen-record::blob::tests::*`, `eileen-record::line::tests::*`, and
  the `recording` fuzz target's `fuzz_entry` with its seed corpus, replayed by
  `eileen-record::it::fuzz_corpus::*`. `eileen record` itself stays a stub until the runtime
  wires the flight recorder.
- `wp1/sim-world` (#160, merged): the world and profiles of section 4.7, with no change to the core.
  `eileen-sim` gains `pc`, the simulated PC: available memory, commit headroom and paging (pages
  in proportion to how far available memory is under the profile's floor), read into
  `Input::Memory` every 5 s as section 2.5 samples it, with Eileen's own share (the core, each
  running brain process, the speech process while it runs and each live worker session, at
  section 4.4's figures), and `MachineEvent::Load`'s fields as section 3.3.17 pins them: the
  CPU's busy percent, `PowerState` (`has_battery`, `on_battery`, `battery_saver`, so a laptop on
  mains differs from a desktop) and `build_slot_held` (a window in the profile); and a heavy app
  that opens, grows to its peak, holds it and closes. It gains `faults`, the world's fault
  points through `eileen_stdx::buggify`, switched per run from the seed (a runtime switch): the
  brain stalls (nothing for the profile's `stall_ms`, then its whole answer, with no interrupt
  heard until then) or fails, the speech process crashes or hangs at a heartbeat or loads 10 s
  slower, a worker session stops part way or runs 5 s late, and the slow bus subscriber skips a
  drain. Every fire is a `FaultRecord` with its step's `seq`; a replay fires exactly those
  records and refuses a site the world cannot fire, and a test can fire a site every time it is
  reached (`Faults::every`). The brain's pace (a first-text delay with jitter, the delta pace,
  how often an unscripted reply dispatches a job) comes from the profile, drawn from a fourth
  random stream. The fake brain keeps one process per slot with its own instance number, stamped
  on its `Ready`, deltas and `TurnDone`: `Start`, `Recycle` and `Promote` start a process with
  the next number, `Stop` ends one with `Exited`, each holds the profile's brain memory while it
  runs, and a stall in one slot holds up only that slot, so a fallback can answer while the
  stalled primary is silent. The core sends only to the primary today, so the per-instance
  fallback of invariant 15 is shown at the part (`parts::tests`) and waits for the core's
  fallback to be shown through it. The generator draws activity (other audio starting and
  stopping, physical input, another app taking the microphone) and takes the profile's barge-in
  chance. Profiles: `default`, `barge-in-heavy`, `many-jobs`, `flaky-sidecars`, `media-playing`
  and `low-memory`, with `profile.schema.json` regenerated. `cargo xtask sim` with no arguments,
  and CI's simulator step, run the pull request seeds of every profile, as `it::pr_seeds` does,
  whose tests and coverage list come from one seed list. Tests: `eileen-sim::pc::tests::*`,
  `eileen-sim::faults::tests::*`, `eileen-sim::parts::tests::*` (the brain's pace, its stall and
  its failure, a stall in one slot not holding up another, promotion, a slow load),
  `eileen-sim::world::tests::the_fault_points_written_here_are_exactly_the_known_sites`,
  `eileen-sim::cli::tests::*`, `eileen-sim::profile::tests::*`, `eileen-sim::inputs::tests::*`,
  `eileen-sim::run::tests::a_run_with_faults_replays_with_exactly_its_fault_records`,
  `eileen-sim::it::world::*` (the readings, the heavy app paging and closing, the CPU and power,
  media at the start, a stalled brain's late answer never spoken, and each fault breaking its
  part through the real core), `eileen-sim::it::regressions::*` (a run with faults written as a
  `SimRegression` file and replayed from it), and `eileen-sim::it::pr_seeds::<profile>::*`,
  every pull request seed of every profile, doublechecked.
- `wp1/jobs` (#163, merged): section 6.3's jobs, first half; acceptance 14, the registry half of
  acceptance 3, and invariants 4, 5 and 12 in the core. `eileen-core` gains `lifecycle`, a
  pure, cloneable `Registry` the core keeps: the eight states and one graph of steps (`Step`,
  `lifecycle::legal`), so every job ends in exactly one terminal state; names of two to four
  words given at accept; slugs collision-checked against seven days of jobs, kept or dropped
  to make room; fan-out children named "agent one" to "agent eight" with ordinals in accept
  order, counted on the parent so a dropped child never frees its number; the approval
  deadline and the jobs past it; cancel targets by id, slug, name or ordinal, the newest
  unfinished job first; a restart that interrupts every unfinished job; and the checkout of a
  coding job whose repository is an absolute path, in place behind `refs/eileen/<job id>` when
  it works alone and in a worktree `worktrees/<repo hash>/<slug>` on `eileen/<slug>`
  otherwise. The core does not send that checkout yet (see the decisions below). A
  `jobs_dispatch` of `ml_project` is refused before routing and admission (`user_error`): the
  `UserError` `JobKindUnavailable` goes on the bus, and its sentence is the call's error
  result. The core's first step, and only that one, emits `JobInterrupted` and the row of each
  job the snapshot held unfinished, and every ID the core mints carries a timestamp after the
  last one's, kept in the snapshot as `minted_ms`, and no earlier than the input's wall-clock
  millisecond, so no ID repeats across a restart, even from an older snapshot. Tests:
  `eileen-core::it::job_model::the_registry_agrees_with_the_reference_model` (the registry
  half of acceptance 3, proptest-state-machine), `eileen-core::it::job_model::*` (slugs over
  seven days and after a job is dropped, ordinals after a child is dropped),
  `eileen-core::lifecycle::tests::*`, and `eileen-core::tests::jobs::*` (acceptance 14, the
  restart, speech going down and up with a job running, slugs, the folder a coding job works
  in, cancel by name and ordinal). Still open: acceptance 3's other half, a reference model
  driving `Core::step` itself (dispatch, the worker's `Init`, `TurnDone` and `Exited`, cancel,
  a restart from `snapshot()`) and checking its `JobUpsert` rows and `Job*` events, which comes
  with fan-out, when the core runs more than one job; and the simulator's checks of invariants
  4, 5 and 12 (a simulated core restart in the world, every persisted row against the graph).
- `wp1/job-steering` (#165, merged): section 6.3's jobs, second half. The core runs many jobs
  at once (at most 16 live worker sessions, `job::MAX_LIVE`). `jobs_dispatch` takes `fanout` 1
  to 8: a fan-out is a parent job with no session, which takes the focus, and one child per
  worker ("agent one" to "agent eight"), each with its own cold session, the whole task in
  `compete` mode and its own sub-task in `split` mode (one per worker, or refused); the parent
  runs with its first child and ends once after its last, completed when any child completed
  and announced once ("Flaky login tests is done: 2 of 3 agents finished."), cancelled with
  its children when the user cancels it, and cancelled by the source that cancelled its last
  child when the user cancelled each child. The router (`Core::route_for`) takes an override the
  brain passed on (model, then depth, then route), recorded on the job with the user's words
  (`override_words`), then the kind's policy, then the depth's model, and says why in
  `JobAccepted.reason`. `lifecycle::Registry::pick` names a job by section 2.5's order: an
  explicit id, slug, ordinal or name; a kind word ("the render"); a route word ("Claude's
  job"); then for a pronoun the job just announced, the focus job and the most recent; several
  fitting jobs with none in focus mean one question. The focus moves to a new job, a fan-out's
  parent, a job the user follows, steers or reroutes, and off a finished one to the newest
  unfinished job outside a fan-out, with the rows whose focus changed. `jobs_control` (from
  the brain's turn in flight, as `jobs_dispatch`) steers (an interrupt, then the words as a new
  user turn to the bound session; before the bind the words go with the task; a worker that
  already owes eight turns is told nothing, and the brain hears that it is still catching up),
  reroutes (a new job with the same task, name, ordinal and working folder on the model asked
  for, then the old one cancelled), focuses and cancels; a spoken "cancel" with words names
  its job the same way and says "I can't find that job." when none fits. App tasks: the apps
  sidecar's `RenderStarted` opens a running job named "the render" with no session,
  `RenderProgress` becomes `JobProgressed` with the percent on the row, and `RenderDone` ends
  it with the usual announcement. `eileen-sim`'s workers answer `Prompt` and `Interrupt`, a
  test can lengthen their work (`World::set_worker_ms`), the scripted brain can call
  `jobs_control` (`BrainReply::controlling`), and `SimResolver` reads "cancel" followed by a
  job's words.
  Tests: `eileen-core::lifecycle::tests::*` (pronoun and ordinal resolution, focus moves,
  successors, progress), `eileen-core::tests::steering::*` (fan-out children and split mode, a
  fan-out's one end, its cancel and the source of its cancel, focus moves, steer reaching the
  bound session and refused past eight owed turns, reroute, the router, spoken words naming
  jobs and the question, the render as an app task and its cancel),
  `eileen-core::tests::jobs::*` (a second job running beside the first), and
  `eileen-sim::it::jobs::*`: three parallel jobs side by side with a steer and a cancel, and a
  fan-out of three whose agent two moves to a stronger model and which ends once, the bases
  of `scenarios/jobs/`'s three parallel jobs and reroute. Still open from section 6.3's jobs:
  acceptance 3's other half (a reference model driving `Core::step` with many jobs), the
  simulator's checks of invariants 4, 5 and 12, narration for the focus job, the one spoken
  sentence on how a compete run's results differ (it needs the brain's completion turn; the
  parent says Eileen's own count until then), worktrees for a coding fan-out's children
  (`FIXME(#5)`), and the scenarios themselves once `eileen-replay`'s scenario mode runs them.
- `wp1/job-steering` again (#175): the review of #165. The router counts a depth as the user's
  override only when the user said it (the brain turn's L1 override, kept on the live turn, or
  a reroute's); a depth the brain chose, as it does for every deep answer, picks the depth's
  model after the kind's policy, with no `override_words`. A deep answer at deep depth takes
  `routing.deep`'s route and model. A policy naming a route v0.1 does not run falls back to
  Claude with the depth's model and says so, and a route asked alone is the reason given. A
  fan-out's parent settles from a tally kept beside the registry, so finished children dropped
  to make room still count, and a cancelled fan-out ends its children first and itself last.
  `job::MAX_LIVE` is kept in `start_worker` alone, and a reroute ends the old session before it
  starts the new one. A reroute carries what the user told the old worker (`Live.told`) and the
  old job's taint; the bind cuts the task, never the words. A steer to a fan-out names the
  agents still catching up. An ordinal picks the fan-out in focus as a name does; route words
  and the focus pass over an app's own render; a render cancelled through `jobs_control` says
  the app goes on; and renders end lost when the apps sidecar exits. `job.rs`, `lifecycle.rs`
  and `tests.rs` were split under check 16's 800 lines (`router.rs`, `worker.rs`,
  `lifecycle/pick.rs`, `lifecycle/tests.rs`, `tests/barge_in.rs`). Tests:
  `eileen-core::tests::router::*`, `eileen-core::tests::fan_out::*`,
  `eileen-core::tests::apps::*`, `eileen-core::tests::refusals::*`, and the new cases in
  `eileen-core::tests::steering` and `eileen-core::lifecycle::tests`.
- `amend/wp1-speech-reload` again (#188), from review of #146's merged text.
  Section 6.4 has `wp2/speech-transport` answer `HangDumpThenKill` with the kill alone, and
  spawn the process again only on the core's `Restart`, as section 6.3 and D81 have it; #178's
  wording read as a kill and a restart, which would have loaded the models twice for every
  hang. Sections 2.13, 3.3.16 and 6.3 treat the minimal core's snapshot as a resume snapshot
  (Surprises below) and name the contracts amendment that lifts the start-only rule for strict
  replay (item 4 of Next). The clause that a backoff step longer than 10 s is kept in full when
  the kill's `Exited` never comes gains its tests:
  `eileen-core::tests::life::a_kill_whose_exit_never_comes_keeps_a_16_s_step_in_full` (five
  kills in a row: no restart at 10 s, a restart at 16 s) and
  `eileen-core::tests::life::a_kill_whose_exit_never_comes_restarts_at_10_s_after_a_4_s_step`
  (three kills in a row: no restart at the 4 s step, a restart at 10 s).
- `amend/wp1-job-pronouns` (#190), which #165 asked for. Section 2.5 now gives the order
  `Registry::pick` implements: an explicit name, slug or ordinal; a kind word, never a fan-out's
  child; a route word, never a child, an app task or an `Act` job; then for a pronoun or no job
  words the job just announced, the focus job and the newest job outside a fan-out; two or more
  remaining candidates mean one question. It listed the announced job last, after the focus job,
  which would always have won. Section 3.3.4 lists `CancelSource` and plans `Typed`: a cancel
  asked for in a turn takes the variant named like the turn's `TurnSource`, else `Voice`
  (item 4 of Next).
- `amend/wp1-job-pronouns` again, from review of #190's merged text. Section 2.5's order holds
  over unfinished jobs only for a cancel, focus, steer, reroute or interrupt; a request about
  what a job did (a status question naming a job, `ShowChanges`, `Keep`, `Undo`, "why did you
  do that?") follows it over jobs that include finished ones, so "undo that" right after a
  coding job's completion is announced undoes that job, not the focus job. The announced job's
  window runs to when the request was heard, not to when the brain's call arrives. Section
  3.3.4 maps a turn's source to `CancelSource` in an exhaustive match (`Scenario` and
  `FirstRun` as `Voice`, no variant for `ClaudeSession`, `JobEvent` and `Connector`), says a
  cancel from a CLI turn is recorded as `Voice` today as well as a typed one, and keeps a
  provisional D79 line. The code and its tests are in items 2 and 4 of Next.
- Next, in this order. WP3's follow-up to #161, which binds a worker on `WorkerEvent::Ready`
  for slice S1, edits `job.rs` and builds on this pull request rather than beside it.
  1. `wp1/speech-late`: the rule for turns heard late (section 2.8), with the simulation test
     of a named command spoken while the speech process restarts and acted on once from the
     replay; WP4's first part added `SpeechStarted.replayed` (#136).
  2. `wp1/job-model`: acceptance 3's other half, `tests/it/job_model.rs` driving `Core::step`
     with many jobs (dispatch and fan-out, `Init` or `Ready`, `TurnDone` and `Exited`, steer,
     reroute, cancel, a restart from `snapshot()`) against the reference model's rows and
     events, and the simulator's checks of invariants 4, 5 and 12 with a core restart in the
     world. This pull request, or whichever WP1 pull request touches `control.rs` or
     `lifecycle` first, also takes two fixes from review of #190. The core takes the announced
     job once, when it routes the turn: `send_brain_turn` keeps `self.announced(cx.now)` on the
     brain's `Live`, and `control` passes that to `Registry::pick` instead of reading
     `announced` when the call arrives (section 2.5), tested in `tests::steering` with job A
     announced ending at t, the focus on B, "cancel it" heard at t + 3 s and sent to the brain,
     and `jobs_control { job: "it", action: cancel }` arriving at t + 12 s, giving
     `JobCancelled` for A, not B. And a test in `lifecycle::tests` of `choose`'s parent arm: in
     the two coding fan-outs of
     `an_ordinal_or_an_agents_name_picks_the_agent_of_the_fan_out_in_focus`, "the coding job"
     gives `Pick::Ask(vec![second, first])` with no focus and `Pick::One(first)` with the focus
     on `first_one`.
  3. Invariants 2, 3 and 9 in the world's check after every step, with the generator pacing
     barge-in windows at 32 ms (acceptance 1), once #151 is fixed. Invariant 9's microphone
     part waits for the core to act on `MicInUseElsewhere` (#152), and the enumeration gains a
     step for it then.
  4. The rest of the full `Core::step` in section 6.3's order: the addressee gate of section
     2.8, intent dispatch, the fact path, `Act` jobs, the brain turn (with narration for the
     focus job), sessions and the warm pool, memory admission (with the worker cap in place of
     `job::MAX_LIVE`), the permission gate, approvals, announcements as data, the input lock,
     the journal, the settings flow, the regret window, forget, route proposals and the
     first-run state machine; `CancelSource::Typed` in the pinned `bus` module, with a cancel
     asked for in a turn, on the fast path or through `jobs_control`, recorded by the turn's
     source (section 3.3.4): the turn's `TurnSource` threaded to both cancel sites (on `Routed`
     for `cancel_spoken`, on the brain's `Live` for `control` and `reroute`) and mapped in an
     exhaustive match with no catch-all arm, tested so that `UserInput::Cli` "cancel the
     render" gives `JobCancelled { by: Cli }`, `UserInput::Typed` gives `Typed`, a scenario's
     turn gives `Voice`, and a `jobs_control` cancel or reroute from a typed turn gives `Typed`;
     `jobs_control`'s `show_changes`, `keep` and `undo` once `FIXME(#5)`'s worktree and
     snapshot wiring lands, naming jobs that include finished ones (section 2.5), with a test
     that "undo that" right after a coding job's completion is announced, while another job
     holds the focus, restores the coding job; the contracts amendment that adds to
     `CoreSnapshotV1` the speech process's life, the HUD's line and the rest of the live state,
     each with a default, with a restore for replay that skips what only a resume does,
     lifting section 3.3.16's rule that strict replay of the minimal core starts only from the
     core's start, and tested by a strict replay from a mid-session checkpoint, taken while
     speech is up and a job is running, that matches the recording; `eileen-replay`'s strict and scenario modes (strict
     mode refusing any checkpoint of the minimal core but the start's, with a plain message, if
     it lands before that amendment); and the scenarios of `scenarios/feel/`,
     `scenarios/jobs/` and `scenarios/first-run/` (acceptance 10).
  5. `wp1/machine-fit` and the speaker gate once WP0's `wp0/machine-contracts` merges, with the
     `heavy-app` and `roomy` profiles and `scenarios/machine/`, and with the first reload the
     core orders that stops recognition, the speech process's reload rule (the 5 s count
     suspended until the reload's first `Heartbeat`, a kill when none comes within 120 s) and
     its simulation test (section 6.3 as #146 amended it); the D77 parts once WP7's amendment
     of section 3.3.13 merges (acceptance 17).

## Surprises

- The minimal core orders no reload that stops recognition, so the suspension of the 5 s
  heartbeat count from such a reload to its first `Heartbeat` has nothing to start it yet. #146
  amended section 6.3 to move it, with its simulation test of a reload whose first `Heartbeat`
  comes within 120 s, to the machine profile's core part, which orders the first such reload
  (item 5 of Next above).
- The minimal core's `CoreSnapshotV1` is a resume snapshot, not a replay checkpoint. It lacks
  the core's live state (the speech process's life, the HUD's line, the turn phase, the
  utterance holding the floor and what Eileen last said, live timers, the brain turn, the
  announcement queue and the speech turns already handled), and a core restored from it
  interrupts the unfinished jobs and starts speech again on its first step. So strict replay of
  the minimal core starts only from the checkpoint taken at the core's start, and an incident
  whose start checkpoint has left the flight recorder's ring replays in scenario mode (sections
  2.13 and 3.3.16 as #188 amended them), until the contracts amendment with the full
  `Core::step` carries every field and a restore for replay skips what only a resume does (item
  4 of Next above).

- A brain reply streams in, so it reaches the speech process as several `SpeechCmd::Speak`
  commands naming one `SpeakId`, clauses numbered on across them, and a `Drained` can come while
  the reply is still open (synthesis behind playback). The contract does not say this yet; the
  pull request proposes the wording for WP4's speech process and audio driver.
- Dumping and re-parsing the flight-recorder ring after every step costs time in proportion to
  everything recorded so far, which the real core's spans made too slow for the pull request
  seeds. The simulator now collects span records as they happen and checks invariant 13 only
  after a step that produced one.

- The core ends an utterance it stops at once, so after a stop with no pause first (muting, a
  request after one or two windows) the heard prefix is the position of the last `Played`
  report, up to 50 ms of audio behind the device, under one character at speaking pace (#151).
  A barge-in pauses first, and the `Paused` report is exact, so the brain is told exactly what
  was heard there. The exhaustive bench reports after every tick, so invariant 3 holds in it
  and the enumeration cannot show the gap; in the world, whose device reports every 50 ms,
  `it::barge_in` asserts the shortfall for a mute and for a request after one window, with
  `FIXME(#151)`, which is why invariant 3 is not yet in the world's per-step check. Taking the
  `Stopped` report's position for a heard prefix not yet handed to the brain would close it.

## Decisions

- The `heavy-app` and `roomy` profiles wait for `wp1/machine-fit`: section 6.3 runs them on
  WP0's fixture machines (the 32 GB fixture for the heavy app), which come with the machine's
  facts. `low-memory` already carries a heavy app on the owner's shape, so the world's app that
  grows and closes is exercised on every pull request.
- The CPU's busy percent and the power state reach the core in section 2.16's
  `MachineEvent::Load`, which arrives with `wp0/machine-contracts`; until then the world keeps
  them (`World::load`) and tells the core nothing, so no input is invented outside the
  contracts.
- A world fault site is reached only inside a step (while the step's input or effects are
  applied), so a record's `seq` is the same in the run and in the replay. A site the core names
  later is recorded after its step and refused by a replay, which cannot fire inside the core.
- A stalled brain process hears no interrupt until its whole late answer has been sent, judged
  by time rather than by turn, and answers the next turn sent to its slot only after it, as a
  stuck CLI would; the other slots answer at once. The core's per-turn filter (it accepts text
  only for the turn in flight) already drops the late answer, which `it::world` shows; the
  per-instance filter of invariant 15 is shown through the core once the core starts a fallback.
- A turn sent to a slot with no process starts one first, as the driver would; the primary
  runs from the start as instance 1, as the runtime starts it before the core's first step.
- A memory reading is due every 5 s of virtual time and goes before a timer or a part's answer
  only when strictly earlier, so the readings never reorder what the slice tests deliver.

- The core strips the assistant's name (at the start or the end of the turn) before it calls
  the resolver, and passes the lower-cased words that remain.
- A worker session's `SessionSpec` leaves the hook and shim endpoints empty for the runtime to
  fill at spawn (it mints the tokens), and names its working folder `run/<session>/cwd`
  relative to the data directory.
- Effects carry the core's own `seq` counter, kept in the snapshot as `effect_seq`.
- The local time reaches the core as `CoreConfig.local_time`, an initial offset and the
  changes the runtime read from the time zone's rules at start for the current year and the
  next; the core takes the change with the latest `from` at or before the step's wall time,
  whatever the order of the list.
- `jobs_dispatch` takes its turn and authority from the core's record of the brain turn in
  flight, not from the call's `CallOrigin`, and only from the brain's shim: the shim protocol
  carries neither, so the claim is filled outside the core and is not trusted.
- An `eileen say` turn may mute, stop and cancel, but an unmute from it is
  routed with the reason `unmute_by_hand` and capture stays off; nothing is spoken, since
  Eileen is muted. Cancel stays, because it only stops work the user started.
- A brain turn that replaces one still unanswered holds both utterances with the weaker
  authority, so an `eileen say` request neither rides on a spoken turn's authority nor drops
  the spoken request.
- The safety and control commands the minimal core does not run (section 2.4, precedence 1:
  "that was for you", undo, forget, off the record and the rest) are answered "I can't do that
  yet." and never reach the brain. Other core commands, such as "how's it going", still go to
  the brain, which can answer them from its read-only tools.
- A worker gets its kind's built-in tools narrowed to `Read`, `Grep`, `Edit` and `Write`, the
  file tools the minimal hook can check. `Glob` is left out: its pattern can reach outside the
  job's folder while only its `path` carries a path fact. `--allowedTools` repeats them with
  `mcp__eileen__*`.

- The backoff resets when the process goes down after a minute up, measured from its `Ready`
  to its last `Heartbeat` for a hang and to the `Exited` for an exit, rather than on a
  heartbeat a minute in: the reset needs no input of its own, and a hang is not credited with
  the 5 s it took to notice.
- The HUD's line keeps its start and is cut at 4 KiB with " ..." rather than keeping the
  newest text: a full line takes no more, so a long reply stops resending the HUD state, and
  the start of a reply is what the user reads first.
- Muting while Eileen speaks a reply puts what the user had not heard on the HUD and the rest of
  the reply after it, since section 6.3 shows every reply on the HUD while muted; the unheard
  rest is still kept for "go on".
- While speech is down announcements wait in their bounded queue, as they do while muted,
  rather than going to the HUD; a reply goes to the HUD because the user is waiting for it.
- Simulator invariant 8 bounds each restart to 1 to 60 s after the child went down (its kill or
  its exit, whichever came first), at most six restarts of one child in any 60 s, and no child
  left down past 60 s. Five restarts fit in the first minute of a child that crashes at once
  (1, 2, 4, 8 and 16 s steps), so six leaves one of margin.

- `turn::exhaustive`'s playback tick is one barge-in window, 32 ms, with one device callback
  of 768 samples, rather than the device's 10 ms: every tick is then the same, so states that
  differ only in time are one state, and onset to silence is bounded by three windows and one
  tick (128 ms, within invariant 2's 150 ms). The 10 ms device and acceptance 4's 96 ms plus
  one callback are shown in the world (`it::barge_in`).
- Invariant 2's onset is the user's `SpeechStarted` (or the moment Eileen began, if they were
  already speaking), and the run's first window less one window when no `SpeechStarted` came
  first, since a window covers the 32 ms before it. The 150 ms bound holds whatever the pace of
  the windows, as section 4.7 states it.
- Invariant 9 judges earcons against the mute the core announced, and judges speech, phrases
  and samples against that or the user's request: the mute earcon confirms the request before
  `MuteChanged { muted: true }`, and the unmute earcon follows `MuteChanged { muted: false }`.

- A setup input is one that leaves the core in a standing state a fresh core started from the
  scenario's configuration and fixtures lacks: `SetMute` and `ToggleMute`, `Memory`, the
  `Activity` states (other audio, the microphone taken, the session locked, a presentation),
  `Settings` `FileChanged` and `Voice` `Calibration`. Section 3.3.16 names setup inputs without
  listing them; the list is `trim::setup_kind`, an exhaustive match. Before the first kept input
  trim keeps only the latest of each kind (for mute, the latest `SetMute` and every `ToggleMute`
  after it), since a recording kept for days holds thousands of memory readings and scenario
  mode would schedule every one. Between the first and the last kept input it keeps every setup
  input no other turn or job claims, since those change the state while the turn and its jobs
  run.
- Trim selects by trace context, the turns and jobs a manifest lists (section 3.11: "that turn
  and its jobs"), not by a range of sequence numbers, so another turn's inputs never reach a
  shared scenario and a job's inputs after its turn's last one are kept. This needs the
  recorder to stamp each input with the turn or job it belongs to; turn IDs are minted by the
  speech process at the speech onset, so a turn's first input carries its own.
- A blob's reference count is a small file beside it, written beside its name, synced and
  renamed into place, rather than a database row: the store stays std and zstd only. Every
  failure leaves a count too high (a blob kept too long), never too low: a crash between the
  blob and its count, a line that fails to write part way, a damaged count file (read as
  `blob::UNKNOWN_COUNT`) and a stub `release_blobs` cannot find in a damaged line. The blobs so
  kept wait for a sweep that recomputes every count from every referrer (recordings, incidents,
  trajectories and the learning store's blobs), which eileen-learn's forget owns (section 2.10
  item 12); until it exists they are kept, never deleted early.
- Deleting a recording renames it to a tombstone, releases its blobs once with
  `release_blobs`, and deletes the tombstone, because releasing is not idempotent. The runtime's
  flight recorder and `eileen record` do this when they are wired.
- `float_roundtrip` on serde_json makes a float in a recorded line read back exactly; the
  default parser can be a unit in the last place off, which would break the exact inverse the
  crate doc promises. Features unify across a build, so the root manifest's `serde_json` entry
  carries it (#157), and `eileen-record` takes `serde_json` from the workspace like every other
  member, as section 3.1 requires (#194, an amendment from WP3's lane that closed #185).

- The job graph: `queued` goes to `running`, `failed`, `cancelled` or `interrupted`;
  `running` to `blocked`, `awaiting_approval`, `completed`, `failed`, `cancelled` or
  `interrupted`; `blocked` and `awaiting_approval` back to `running` (`Step::Resume`, which
  also carries the decline at the approval deadline) or to `failed`, `cancelled` or
  `interrupted`. Section 2.5 names the states without the edges. Only a running job
  completes, so a worker that ends before it was given its job fails with that reason.
- A restart interrupts queued jobs as well as running ones (invariant 12 names running
  ones): the admission queue is not in the snapshot and no worker outlives the core, so a
  queued job would otherwise wait for an admission no one will make.
- Ordinals belong to a fan-out's children, in accept order, as section 2.5 says; a job
  outside a fan-out has none, so "agent one" never names it. The contracts' "ordinal among
  live jobs" is read as among the live children of one fan-out. The registry counts each
  parent's children and never lowers the count while the parent is kept, and a child is
  accepted only while its parent is kept and unfinished (`Refused::NoParent`): a fan-out's
  children are accepted with it, and a finished fan-out takes no new ones.
- A child's slug is its parent's slug and its ordinal (`flaky-login-2`), so its branch and
  worktree sit beside its siblings'. A slug drops a leading article and keeps at most 40
  bytes, cut at a word, since it names a folder and a branch.
- The in-place snapshot is kept under `refs/eileen/<job id>` rather than the slug, since a
  slug may come back after seven days and the ref outlives that. The core does not send a
  checkout yet: every session works in `run/<session>/cwd`, and no row names a snapshot,
  worktree or branch (`FIXME(#5)` in `job::session_spec`). The harness reads only
  `SessionSpec.cwd` today, records no snapshot and creates no worktree, and the contract
  comments `snapshot_ref` as "a snapshot to restore before it starts". The in-place wiring
  lands with WP3's harness side and a contracts amendment that documents the field as the
  ref to record, so no row claims a snapshot that was never taken. A repository path in
  `cwd` would also be personal data in a field that is not `Personal`; that type change is
  raised with WP3 then.
- Only an absolute path with no `.` or `..` part (a drive letter and a separator, a UNC path,
  or a leading `/`) can become a checkout. The brain's `repo` is untrusted and may be a bare
  name; the harness puts a relative `cwd` under the data folder, so a name such as
  `learning` would make Eileen's own folder the job's, where the minimal hook lets file tools
  write. A name stays in the scratch folder until section 2.5's resolution (the utterance,
  the focused editor, a `project` fact, or asking once) is built.
- The core cannot see the user's uncommitted files, so the overlap rule of section 2.5
  (a worktree when they overlap the files the task names) waits for the runtime to report
  them; `Registry::checkout` takes the fact as an argument and the core passes false.
- Slugs are checked against the jobs the core keeps (at most 32) and against the slugs it
  has given in the last seven days (at most 1024, `lifecycle::SLUGS_KEPT`), so a job dropped
  to make room still holds its slug, branch and worktree folder. The snapshot keeps only its
  jobs, so after a core restart only the kept jobs' slugs count until the snapshot or the
  store hands the core the week's slugs (a contracts change); a worktree path that already
  exists is then the harness's to refuse.
- A repository's key ignores case and the kind of separator only in a Windows path (a drive
  letter or a backslash); a Linux path keeps its case.
- A cancel whose target names no unfinished job cancels nothing; with pronoun resolution
  (`wp1/job-steering`) Eileen says "I can't find that job." when words named one, and says
  nothing for a bare "cancel" with no job open.
- A step a job cannot take from where it is leaves it live, so a later event of its session
  still ends it; only a finished job stops being the live one.
- An `ml_project` refusal puts the `UserError` on the bus (`BusEvent::UserError`, for the
  HUD's card, section 2.13) and returns its sentence as the call's error result with no
  structured output, since a tool result's `output` must match the op's output schema,
  `JobsDispatchOutput`. The brain says the sentence.
- The restart's interruption runs on the core's first step, once, by a flag of its own, not
  on the first step with the speech process off: a later stop and start of speech never
  interrupts a job this core accepted.

- The focus job is the row's `focus` flag, held by at most one unfinished job. A new job
  takes it (a fan-out's parent for the fan-out), as does a job the user follows, steers or
  reroutes to; when it ends, the focus moves to the newest unfinished job outside a fan-out
  that is not an app task. A render the app started takes it only when no other job is open,
  since the user did not ask for it. Each change persists the rows whose flag changed.
- Section 2.5 listed "it right after an announcement" last among the pronoun rules; the core
  tries it first for a pronoun, since the focus job would otherwise always win, and only for
  an unfinished job announced within `conversation.follow_up_s` with no reply since. Section
  2.5 gives that order since #190. `control` reads that window when the brain's `jobs_control`
  call arrives, so a slow brain can carry a request heard inside it past its end; section 2.5
  runs it to when the request was heard since the follow-up to #190, and item 2 of Next keeps
  the announced job on the brain turn. `pick` passes over finished jobs because its callers
  cancel, focus, steer and reroute; a request about what a job did names finished jobs too
  (section 2.5), which `show_changes`, `keep` and `undo` need when they are built. Names
  match after the articles and the words "job", "task", "please" and "now" are dropped. Kind
  and route words name only jobs outside a fan-out, whose children are named by ordinal or
  name, and route words never name an app task or an `Act` job, which no route runs (their
  rows say Claude only because a row needs a route). An ordinal, a name, a kind word or a route
  word that fits several jobs picks the one in focus among them (the focus job, a child or the
  parent of it, or a sibling of a focused child), or asks one question naming the newest
  three; the answer is a new request that names the job, since the core keeps no pending
  question yet.
- A fan-out's children announce only their failures (section 2.5's table interrupts for
  `JobFailed`); their completions are on the bus and the HUD, and the parent's end is the one
  announcement, with Eileen's own count rather than a worker's words. A parent whose children
  were all cancelled is cancelled by the source that cancelled the last of them (the HUD's
  cancel of the last agent cancels the fan-out from the HUD), and one whose children all
  failed fails. The parent settles from a tally the core keeps beside the registry (children
  accepted, completed, cancelled and failed, and the last cancel's source), not from the
  children still kept, since a running fan-out's finished children are among the oldest
  finished jobs the registry drops to make room. A cancelled fan-out cancels its children first,
  by their parent, and ends itself last, by the user's source, so it ends once, after its last
  child.
- Steer is `WorkerCmd::Interrupt` then `WorkerCmd::Prompt`. The core counts the user turns a
  worker owes (`Live.owed`): the bound task's and one per steer, at most 8, and the job ends
  with the last `TurnDone`, so the interrupted turn's own `TurnDone` (errored or not) never
  ends it. A steer to a worker that owes 8 is refused with "the test agent is still catching
  up; tell it again in a moment.", so the brain never says it told a worker that heard
  nothing. This assumes each user turn ends in one `result`, an interrupted one included,
  which WP3's mid-turn interrupt case (its acceptance 3) is to confirm. A steer before the
  session is ready waits and goes with the task in the bind, which cuts the task to make room,
  never the words (at most `job::STEER_CHARS`, the newest kept); a steer to a fan-out's parent
  goes to each running child, and when some owe eight turns the brain hears which ("Told flaky
  login tests; agent two is still catching up.").
- Reroute ends the old session first, so no more than `job::MAX_LIVE` workers run even for a
  moment (`start_worker` keeps that bound, the one place it is kept), then accepts the new job,
  then cancels the old job, so a fan-out's parent never sees all its children end in between.
  The new job keeps the name and ordinal the user knows (`Registry::successor`) with a slug of
  its own, works in the old job's folder, starts with the old job's taint (that folder holds
  what the old job read) and no grants, and is given what the user told the old worker
  (`Live.told`, sent or still waiting) with the task. The old transcript is not attached
  (`FIXME(#3)`): sessions run with no session persistence and the harness keeps no
  transcript. A cancel through `jobs_control` is recorded as `Voice`:
  `CancelSource` has no source for a typed brain turn, and a "cancel" from a typed or CLI turn
  on the fast path is recorded as `Voice` too, though `Cli` exists, since neither cancel site
  sees the turn's `TurnSource` (`Routed` and the brain's `Live` carry only its authority).
  Section 3.3.4 plans `Typed` since #190 and maps every source since its follow-up (item 4 of
  Next). The new session's `cwd` is the old
  session's `run/<session>/cwd`, as section 2.5 asks ("the same working directory"); section
  2.12's path classes name a session's own `run\<session>\cwd` its `JobFolder` and "another
  session's folder" `EileenData`, so the runtime's classifier must take the job folder from
  the session's `SessionSpec.cwd`, not from its id, or a rerouted worker's file tools are
  refused. The pull request suggests that wording for section 2.12.
- The router treats a route or a model the brain passes in `jobs_dispatch` or a reroute as the
  user's spoken override, and a depth only when the user said it: the brain turn's L1 override
  ("quick answer", "think hard about this"), which the core keeps on the live turn from
  `Resolution::Brain.depth`, or a reroute's. Section 2.3 has the brain pick the depth of every
  deep answer itself, so a depth it chose is no override: it picks the depth's model only after
  the kind's policy, and no words are recorded. An override is recorded with the brain turn's
  words; an asked model beats a said depth, which beats the policy's model. A deep answer at
  deep depth takes `routing.deep`'s route and model (section 2.3 configures the deep workers
  apart from the brain), after the kind's policy; other kinds at deep depth keep the strongest
  model, and a considered deep answer the mid one. An asked route v0.1 does not build is
  refused; a policy or `routing.deep` naming one falls back to Claude with the depth's model,
  never with a model meant for that route, and the reason says so. A route asked alone is the
  reason given.
- An app task the user started in the app has no turn: its `authorized_by` is the nil turn
  id and its bus events carry no turn. Its row needs a route, so it says Claude with an empty
  model, and its completion is announced with no attribution. Cancelling it, by voice or
  through `jobs_control`, stops the tracking, and Eileen says the app is still running it,
  since the adapter has no op to stop a render. When the apps sidecar exits, each render it
  reported ends as failed on its row and is announced as lost ("I lost track of the render
  when the app's bridge stopped."), and none stays remembered, so a render still running when
  the sidecar is back opens a job of its own. A bridge to Resolve lost mid-render while the
  sidecar runs on is WP6's to report as `RenderDone { ok: false }`. At most `RENDERS_KEPT`
  renders are remembered, the oldest whose job has ended forgotten first; while every one
  still runs, a new render is not tracked. A render started through Eileen's own
  `render_start` call opens its job once the core forwards tool calls to connectors
  (`FIXME(#3)`).
- `jobs_control`'s `show_changes`, `keep` and `undo` are answered "I can't do that to a job
  yet." until the worktree and snapshot wiring of `FIXME(#5)` lands. `interrupt` is answered
  the same way until the pause of section 2.5 is built (with the hands-off pause and the
  input lock): an interrupted worker waits with no turn in flight, which the job's states
  cannot show yet, and its errored `TurnDone` would end the job as failed.

## Outcome

Open.
