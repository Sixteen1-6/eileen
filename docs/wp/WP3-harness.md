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

The approval contract test's first pass ran on the owner's PC on 2026-10-21 and settled the
permission route, which section 6.5's amendment of that date records (see Goal).

- `wp3/worker-tools` (open, draft): pins the provisional `tools/worker` module of
  `eileen-contracts` (section 3.3, `modules.toml`), the worker toolset row of section 3.3.9.
  `Approve` now documents route 2 of section 2.5 and gains `matches_ask` (the same
  `tool_use_id` and an equal input); `ApproveReply` holds the two shapes the CLI accepted,
  `{"behavior":"allow","updatedInput":<input>}` and `{"behavior":"deny","message":...}`, with
  `text()` and `content()` for the one text block. Tests in `eileen-contracts::it::wire` check
  the call and both replies against `docs/research/fixtures/wp3-approval/mcp-approve.json` and
  the guard's ask in `hooks.json`, and a property test shows `allow` carries any stored value
  unchanged. None of the acceptance items above closes with it; it is the contract the
  dispatcher (WP2) and `wp3/worker-brain` build on.

## Surprises

- `it::modules::exactly_the_slice_modules_are_pinned` failed for any module a package pinned,
  though section 3.3 has each package's first pull request pin its own. `wp3/worker-tools`
  replaces it with `wp0_pinned_exactly_the_slice_modules`: the modules with no `pinned_by` are
  exactly slice S1's, and a module a package pinned keeps its `pinned_by`.

## Decisions

- `ApproveReply::allow` takes an `Untrusted<Value>`, the type of the guard's stored
  `HookRequest::PreToolUse.tool_input`, while `Approve.input` stays a plain `Value`. So the
  call's own input does not type-check as an allow, which section 2.5's echo rule needs
  (finding 7 of the contract report: the CLI runs `updatedInput` without calling the guard
  again). A compile-fail doc test on `allow` keeps it so.
- `ApproveReply::text()` cannot fail; if `serde_json` ever failed to write a reply, the text
  would be a denial, never an allow.
- An approve result is never an error, a denial included: the CLI reads an errored `approve`
  call as a failed tool, not a denial (the contract report's tool-error rows).

## Outcome

Open.
