# WP3: Claude harness and the brain

The living plan for work package 3 of [BUILD.md](../BUILD.md) section 6.5: the Claude harness,
the brain backend and the stub harness. The agent doing the work keeps it current.

## Goal

First, on the owner's PC, the approval contract test against the real CLI (section 2.5), the
`--restricted` test per session kind, and the containment measurements for decision D40's v0.2
sandbox (Windows Sandbox, AppContainer, a restricted token at low integrity). The first pass ran
on CLI 2.1.304 in `wp3/approval-spike` and settled the route: `--permission-prompt-tool
mcp__eileen__approve` under `--permission-mode dontAsk`
([2026-10-wp3-approval-contract.md](../research/2026-10-wp3-approval-contract.md)).

Then the typed stream-json parser, `ClaudeWorker` with `SessionSpec` and `JobBinding`, the warm
pool's session lifecycle, `BrainBackend` with its Primary, Spare and Fallback slots, the brain
benchmark, model aliases resolved per CLI version, worktrees and in-place snapshots,
`continue_session` jobs, `eileen claude` and `eileen doctor claude`, the other routes as types
that return `RouteUnavailable`, `eileen-stub-harness` with scripts recorded from real sessions,
and `cargo xtask contract-claude`.

`wp3/worker-brain`, one worker job and one brain turn end to end, is the pull request slice S1
waits for. The D77 memory work (the memory gate and its probes) follows it and never blocks the
slice.

## Branch, dependencies and files

- Branch: `wp3/<topic>`; the slice's pull request is `wp3/worker-brain`.
- Depends on: WP0 phase A. Wave: W1. Desktop: no.
- Owns: `crates/eileen-harness`, `crates/eileen-stub-harness`, `tests/harness-scripts/`,
  `scenarios/claude/`, the fuzz targets `stream_json_line` and `control_message` with their
  corpora and regressions, and this file.

## Done when

The acceptance list of BUILD.md section 6.5, items 1 to 8, each recorded under Progress with its
evidence when it holds. The lines below are reminders; the full text in BUILD.md is the bar.

1. The approval contract test passes on the owner's PC with the route recorded; the subagent case
   and the containment measurements are recorded.
2. Against `eileen-stub-harness` in CI: a pooled worker job completes, and narration, permission
   requests, `Skill` calls, crashes and cancel behave as specified.
3. On the owner's PC (`real_claude::`): brain and worker init, warm-turn and cold-start timings,
   pool fit, interrupt and learning-loop row 7; the admission record is a W2 item.
4. The worker and brain environments follow their lists, no code opens the user's `.claude`
   folder, and real coding workers run `cargo test` and `npm test` green.
5. `eileen doctor claude` reports a signed-out CLI and suggests `claude auth login` without
   running it, naming the npm shim and the real executable behind it.
6. Model aliases resolve to full ids per CLI version, `eileen bench brain` gives the B10 numbers,
   and every other route returns `RouteUnavailable`.
7. The fuzz target `stream_json_line` runs clean for 10 minutes, and its corpus replays on Windows.
8. Notes (D77): every session spawns with vendor memory off until the gate passes, and the memory
   gate and its probes are recorded here.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

Not started.

## Surprises

None yet.

## Decisions

None yet.

## Outcome

Open.
