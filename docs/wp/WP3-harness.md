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

- `wp3/worker-tools` (#126, merged as d2e1281): pins the provisional `tools/worker` module of
  `eileen-contracts` (section 3.3, `modules.toml`), the worker toolset row of section 3.3.9.
  `Approve` now documents route 2 of section 2.5 and gains `matches_ask` (the same tool, the
  same `tool_use_id` and an equal input as the pending ask). `PendingAsk`, built only from the
  guard's `PreToolUse` request, holds the ask until the user decides, and `ApproveReply::allow`
  consumes it, so an allow carries the guard's stored input and never the call's.
  `ApproveReply` holds the two shapes the CLI accepted,
  `{"behavior":"allow","updatedInput":<input>}` and `{"behavior":"deny","message":...}`, with
  `text()` and `content()` for the one text block. The same pull request makes
  `HookRequest::PermissionRequest.tool_use_id` optional, as section 6.5 directs, because the
  CLI's `PermissionRequest` body carries none. Tests in `eileen-contracts::it::wire` check the
  call and both replies against copies of `mcp-approve.json` and `hooks.json` from
  `docs/research/fixtures/wp3-approval/` (under `tests/it/fixtures/wp3-approval/`, so a change
  to them runs the Rust lanes), a property test shows `allow` carries any stored value
  unchanged, and compile-fail doc tests show the call's input cannot become an allow. None of
  the acceptance items above closes with it; it is the contract the dispatcher (WP2) and
  `wp3/worker-brain` build on.
- For WP2's dispatcher: hold each ask as the `PendingAsk` the guard's request made, keyed by
  session and `tool_use_id`, and test an `approve` call whose input differs from the stored one
  (refused and logged as a security event, never answered `allow`). The weekly contract test's
  case of an `updatedInput` that differs from the stored input (section 2.5) is the CLI's side
  of the same rule.
- `wp3/approval-amendment` (#131, draft, `type: amendment`, for the owner's review): brings BUILD.md
  in line with #126 and the contract report. Section 3.3.3 writes
  `HookRequest::PermissionRequest.tool_use_id` as `Option<String>`, as the crate already has
  it, and section 6.5's "when WP3 next changes the contracts" clause is marked done. Section
  3.3.16's `StubScriptLine::PermissionRequest`, and its doc comment in `record.rs`, say the stub
  plays it the way the CLI behaves under `dontAsk`: a tool use posted to the `PreToolUse` guard,
  with an `ask` routed to the permission-prompt tool, never the `PermissionRequest` hook. A
  `PermissionRequest` with no id is keyed in scenario replay by its ordinal among the job's
  permission requests. Tests in `eileen-contracts::it::wire` parse the recorded
  `PermissionRequest` body (no id) and round-trip one with an id. No crate produces
  `HookRequest` yet, so it never blocks slice S1.
- For `eileen-stub-harness`: a `PermissionRequest` script line carries a tool and its input and
  no id, so the stub mints the `tool_use_id`, posts the `PreToolUse` body to the guard's URL from
  the session's settings file, and on `ask` calls `approve` through the session's MCP config
  with the same id and input, then runs or refuses as the reply says. It never posts to the
  `PermissionRequest` hook, which the CLI never calls on the approval route.

## Surprises

- `it::modules::exactly_the_slice_modules_are_pinned` failed for any module a package pinned,
  though section 3.3 has each package's first pull request pin its own. `wp3/worker-tools`
  replaces it with `wp0_pinned_exactly_the_slice_modules`: the modules with no `pinned_by` are
  exactly slice S1's, and a module a package pinned keeps its `pinned_by`.

## Decisions

- An allow is built only from a `PendingAsk`, and a `PendingAsk` only from the guard's
  `PreToolUse` request (`PendingAsk::from_guard_ask`), which section 2.5's echo rule needs
  (finding 7 of the contract report: the CLI runs `updatedInput` without calling the guard
  again). The first design took any `Untrusted<Value>`, which did not hold the rule: the
  shim's `CallLine.args`, a `PermissionRequest`'s input and `Untrusted::new(call.input)` all
  have that type. Now `PendingAsk`'s fields are private, the `Allow` variant's field is an
  `AskedInput` whose field is private, `ApproveReply` is not deserializable, and `PendingAsk`
  is not `Clone`, so `allow` consuming it allows each ask once. Compile-fail doc tests show
  that `allow(Untrusted::new(call.input))`, the variant written out with that input, an
  `AskedInput` made from it and a parsed reply do not compile. The one way around it is to
  write a `HookRequest::PreToolUse` out by hand from the call, which review catches; the
  dispatcher's own tests still check the input comes from the stored ask.
- `Approve::matches_ask` also compares the tool's name, which section 2.5 does not ask for: the
  CLI sends it and the stored ask has it, and a mismatch can only be an error or an attack.
- `ApproveReply::text()` cannot fail; if `serde_json` ever failed to write a reply, the text
  would be a denial, never an allow.
- An approve result is never an error, a denial included: the CLI reads an errored `approve`
  call as a failed tool, not a denial (the contract report's tool-error rows).
- `HookRequest::PermissionRequest.tool_use_id` stays a plain `Option<String>` with no
  `#[serde(default)]`: serde's derive already reads a missing `Option` field as `None`, no other
  `Option` field in `eileen-contracts` carries the attribute, and schemars would add a default
  to the pinned `harness` schema for no change in behaviour.
  `a_permission_request_without_a_tool_use_id_parses` holds the behaviour instead, so a later
  `with =` or `deserialize_with` on the field that made it required would fail there.

## Outcome

Open.
