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
  amended it, D81, D83), which merges before WP2's `wp2/speech-transport`. `eileen-core` gains
  `life`: `Sidecar(Speech, Start)` on the first step; `HangDumpThenKill` then `Restart` when no
  `Ready` comes within 120 s of a `Start` or `Restart`, or no `Heartbeat` for 5 s from that
  `Ready` or the latest `Heartbeat`; `Restart` alone after an `Exited`; after a kill, `Restart`
  waits for the killed process's `Exited` as well as the step, for at most 10 s; each `Restart` after a
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
- `wp1/turn-exhaustive` (open): acceptance 2 and 4. `eileen-sim::exhaustive` enumerates every
  sequence of section 4.7's eleven turn-taking inputs (speech start, speech end, end of turn,
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
- `wp1/record` (open): `eileen-record`'s formats, store, reader, writer and trim (section 3.3.16).
  `line::encode` and `line::decode` write any contract value as one line of JSON with each
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
- `wp1/sim-world` (open): the world and profiles of section 4.7, with no change to the core.
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
- `wp1/jobs` (open, #163): section 6.3's jobs, first half; acceptance 14, the registry half of
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
- Next: `wp1/speech-late`, the rule for turns heard late, once WP4's first part has added
  `SpeechStarted.replayed` (it has, #136); then invariants 2, 3 and 9 in the world's check
  after every step, with the generator pacing barge-in windows at 32 ms (acceptance 1), once
  #151 is fixed. Invariant 9's microphone part is not in the core yet (#152): the core ignores
  `MicInUseElsewhere`, and the enumeration gains a step for it when the core acts on it.

## Surprises

- The minimal core orders no reload that stops recognition, so the suspension of the 5 s
  heartbeat count from such a reload to its first `Heartbeat` has nothing to start it yet. #146
  amends section 6.3 to move it, with its simulation test of a reload whose first `Heartbeat`
  comes within 120 s, to the machine profile's core part, which orders the first such reload.
- The speech process's life and the HUD's line are not in `CoreSnapshotV1`, so strict replay of
  the minimal core starts from the checkpoint taken at the core's start (section 3.3.16 as #146
  amends it) until a contracts amendment adds them with the full `Core::step`.

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
  crate doc promises. Features unify across a build, so it belongs in the root manifest's
  `serde_json` entry, which is an amendment; `eileen-record` turns it on in its own manifest
  only until that amendment merges.

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
- A cancel whose target names no unfinished job does nothing and says nothing yet; the
  spoken "I can't find that job" comes with pronoun resolution in the second half.
- A step a job cannot take from where it is leaves it live, so a later event of its session
  still ends it; only a finished job stops being the live one.
- An `ml_project` refusal puts the `UserError` on the bus (`BusEvent::UserError`, for the
  HUD's card, section 2.13) and returns its sentence as the call's error result with no
  structured output, since a tool result's `output` must match the op's output schema,
  `JobsDispatchOutput`. The brain says the sentence.
- The restart's interruption runs on the core's first step, once, by a flag of its own, not
  on the first step with the speech process off: a later stop and start of speech never
  interrupts a job this core accepted.

## Outcome

Open.
