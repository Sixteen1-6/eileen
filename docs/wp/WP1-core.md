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

- `wp1/minimal-core` (open): the minimal core slice S1 wires. `eileen-core` gains `turn` (a
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
  (`wp1/turn-exhaustive`) builds on `turn::Turns` next. Every CI lane is green on #130.
- Next for the first pull request's scope: #127 (merged while #130 was open) adds the speech
  process's life (start at boot, hang dump and restart on a missed heartbeat or a late `Ready`,
  replies on the HUD while speech is down, turns heard late addressed only by name, a named
  command during a restart acted on once from the replay). It waits for `SpeechStarted.replayed`
  in the contracts and is the follow-up `wp1/speech-life`. #130 already ends an utterance that
  gets `SpeakDone` with no clause, as the audio component answers while speech is down.

## Surprises

- A brain reply streams in, so it reaches the speech process as several `SpeechCmd::Speak`
  commands naming one `SpeakId`, clauses numbered on across them, and a `Drained` can come while
  the reply is still open (synthesis behind playback). The contract does not say this yet; the
  pull request proposes the wording for WP4's speech process and audio driver.
- Dumping and re-parsing the flight-recorder ring after every step costs time in proportion to
  everything recorded so far, which the real core's spans made too slow for the pull request
  seeds. The simulator now collects span records as they happen and checks invariant 13 only
  after a step that produced one.

## Decisions

- The core strips the assistant's name (at the start or the end of the turn) before it calls
  the resolver, and passes the lower-cased words that remain.
- A worker session's `SessionSpec` leaves the hook and shim endpoints empty for the runtime to
  fill at spawn (it mints the tokens), and names its working folder `run/<session>/cwd`
  relative to the data directory.
- Effects carry the core's own `seq` counter, kept in the snapshot as `effect_seq`.
- The local time zone reaches the core as `CoreConfig.local_offset_min`, read by the runtime at
  start.

## Outcome

Open.
