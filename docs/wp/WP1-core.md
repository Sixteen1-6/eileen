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
- `wp1/speech-life` (open): the core's half of the speech process's life (section 6.3 as #134
  amended it, D81, D83), which merges before WP2's `wp2/speech-transport`. `eileen-core` gains
  `life`: `Sidecar(Speech, Start)` on the first step; `HangDumpThenKill` then `Restart` when no
  `Ready` comes within 120 s of a `Start` or `Restart`, or no `Heartbeat` for 5 s from that
  `Ready` or the latest `Heartbeat`; `Restart` alone after an `Exited`; each `Restart` after a
  backoff step of 1 s doubling to 60 s, back to 1 s once the process stayed up a minute after
  its `Ready`; and `SidecarRestarted` on the bus with the reason. It gains `hud`: the HUD state
  with `MicState::LoadingSpeech`, `SpeechRestarting`, `Muted` or `Listening`, sent when it
  changes, and the line that carries every reply Eileen cannot say aloud (speech down, the
  microphone muted, a `SpeakDone` with no clause, the part of an utterance not synthesized when
  speech went down, and the rest of a brain reply after any of these). A turn open when speech
  goes down is dropped, and announcements wait while Eileen cannot speak. `eileen-sim`'s speech
  part answers `Start` and `Restart` with `Ready` after a 2 s load and pings with a `Heartbeat`
  every second, and takes injected hangs, crashes and loads that never finish; invariant 8,
  `children_restart_with_bounded_backoff`, is checked after every step. Tests:
  `eileen-core::life::tests::*` (the backoff sequence), `eileen-core::tests::life::*` (the two
  deadlines, an exit, the backoff's reset, replies on the HUD), `eileen-core::turn::tests::*`
  (`speech_lost`), `eileen-sim::invariants::tests::invariant_8_*` and five
  `eileen-sim::it::slice` tests plus a load that never finishes and a crash mid-answer.
- Next: `wp1/speech-late`, the rule for turns heard late, once WP4's first part has added
  `SpeechStarted.replayed` (it has, #136); then `wp1/turn-exhaustive`.

## Surprises

- The minimal core orders no reload that stops recognition, so the suspension of the 5 s
  heartbeat count from such a reload to its first `Heartbeat` (section 6.3) has nothing to
  start it yet. It arrives with the first reload the core orders, with its simulation test of a
  reload whose first `Heartbeat` comes within 120 s.

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
- While speech is down announcements wait in their bounded queue, as they do while muted,
  rather than going to the HUD; a reply goes to the HUD because the user is waiting for it.
- Simulator invariant 8 bounds each restart to 1 to 60 s after the child went down (its kill or
  its exit, whichever came first), at most six restarts of one child in any 60 s, and no child
  left down past 60 s. Five restarts fit in the first minute of a child that crashes at once
  (1, 2, 4, 8 and 16 s steps), so six leaves one of margin.

## Outcome

Open.
