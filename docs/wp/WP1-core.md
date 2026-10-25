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
11. `eileen-core` depends only on `eileen-contracts`, `eileen-stdx` and `eileen-trace` (check 24).
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

Not started.

## Surprises

None yet.

## Decisions

None yet.

## Outcome

Open.
