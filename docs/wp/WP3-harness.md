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
- `wp3/approval-amendment` (#131, draft, `type: amendment`, for the owner's review): brings
  BUILD.md in line with #126 and the contract report. Section 3.3.3 writes
  `HookRequest::PermissionRequest.tool_use_id` as `Option<String>`, as the crate already has
  it, and section 6.5's "when WP3 next changes the contracts" clause is marked done. Section
  3.3.16's `StubScriptLine::PermissionRequest`, and its doc comment in `record.rs`, say the stub
  plays it the way the CLI behaves under `dontAsk`: a tool use posted to the `PreToolUse` guard,
  with an `ask` routed to the permission-prompt tool, never the `PermissionRequest` hook.
  Section 4.7 says how the stub reaches both: the guard over loopback HTTP, and `approve` by
  spawning the shim from the MCP config and writing the JSON-RPC itself, without rmcp. A
  `PermissionRequest` with no id is keyed in scenario replay by its ordinal among the job's
  permission requests. Tests in `eileen-contracts::it::wire` check that the recorded
  `PermissionRequest` body has no `tool_use_id` and that the core's request built from it,
  written without the field, parses as `None` and writes back as null, and they round-trip one
  with an id. No crate produces `HookRequest` yet, so it never blocks slice S1.
- For `eileen-stub-harness`: a `PermissionRequest` script line carries a tool and its input and
  no id, so the stub mints the `tool_use_id` and posts the `PreToolUse` body to the guard's URL
  from the session's settings file, with the bearer and session headers the settings file
  fills in from the environment through `allowedEnvVars`. On no decision it plays the tool use
  as allowed, and on `deny` it plays a refusal. On `ask` it calls `approve` through the
  session's MCP config with the same id and input. It plays the tool use as allowed on `allow`,
  a refusal listed in `permission_denials` on `deny` or a reply in neither shape, and a tool
  error left out of `permission_denials` on a call that errors, outlives `MCP_TOOL_TIMEOUT` or
  loses its server, as the CLI did in section 2.2 of the contract report (the harness counts
  that tool error as a denial itself). With no approve server in the MCP config it ends the
  session with exit code 1 at the first ask, as the CLI did. Like the CLI under
  `onFailure: "block"`, it treats a dead endpoint, a timeout, an HTTP error, a body that is not
  JSON or an invalid decision from the guard as a block. A tool use played as allowed is the
  script's next lines, which hold the recorded result: the stub never executes a script's
  input, and it never posts to the `PermissionRequest` hook, which the CLI never calls on the
  approval route.

- `wp3/stream-json` (#135, draft): the typed stream-json parser of section 6.5, in a new
  `eileen_harness::stream` module. `parse` caps the line before parsing (`MAX_LINE_BYTES`,
  16 MB, since a tool result can carry a screenshot), gives `Unknown` for an unknown `type` or
  `system` subtype, keeps what `system/init` reports for feature detection (session id, model,
  tools, MCP servers with status, plugins, capabilities, `permissionMode`, `memory_paths.auto`
  as `Personal`, the CLI version), splits assistant text into text and `> ` narration, redacts a
  tool input with the `Redactor` and caps it at 2 KB as JSON text (a cut input is a JSON string
  whose quotes and escapes count), turns a `Skill` call into `SkillInvoked` with its name
  redacted and capped at 256 bytes,
  and types tool results, partial text deltas, rate-limit events (as `PlanUsage`), hook events,
  `system/permission_denied` and `result` (cost, context tokens, `permission_denials`, a failed
  sign-in). `Line::denials` reads the three denial sources of section 2.5 and counts a failed
  `approve` call as a denial. `worker_events` and `brain_events` map a line into the contracts'
  events. `SUBAGENT_TOOLS` names `Task` and `Agent`. Unit tests in `stream::tests` run against
  copies of the contract test's runs (`crates/eileen-harness/tests/fixtures/wp3-approval/`);
  `fuzz_entry::stream_json_line` checks the caps, the narration split and that a literal
  registered with the `Redactor` never survives in a tool input or skill call, and its 51 seeds
  (from those runs, plus hand-written lines for the shapes they leave out) replay in
  `eileen-harness::it::fuzz_corpus` on Windows and Linux, which is acceptance 7's Windows
  replay. Its 10-minute `long.yml` run waits for WP0 phase B's `long.yml`.
- The `control_message` half (the stdio control protocol: `interrupt` and `can_use_tool`
  requests, `control_response`, `fuzz_entry::control_message` and its corpus from
  `stdio-control.json`) is built and goes in its own pull request, `wp3/control-message`, on top
  of `wp3/stream-json`: with it the logic ran to about 800 lines. Until it lands, control lines
  parse as `Unknown` and the `control_message` entry is the stub.

## Surprises

- `it::modules::exactly_the_slice_modules_are_pinned` failed for any module a package pinned,
  though section 3.3 has each package's first pull request pin its own. `wp3/worker-tools`
  replaces it with `wp0_pinned_exactly_the_slice_modules`: the modules with no `pinned_by` are
  exactly slice S1's, and a module a package pinned keeps its `pinned_by`.

- The CLI writes `system/init` again at the start of every user turn in a stream-json
  session (the steer run's second turn), so a driver that treats init as "started" must take
  the first and check the rest. A `system` subtype the plan never named, `thinking_tokens`,
  arrives while the model thinks; the parser gives it `Unknown`.
- The contract test's fixtures leave the rate-limit event out (it describes the account's plan
  usage), so its shape comes from the Agent SDK's parser until a recorded script confirms it.

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
  `#[serde(default)]`: serde's derive already reads a missing `Option` field as `None`, and
  schemars would add a default to the pinned `harness` schema for no change in behaviour. The
  one `Option` field in the crate that carries the attribute, `SimRegression::seed`, needs it
  only because `with = "seed_text"` turns off serde's missing-field rule for `Option`. That rule
  is also why `a_permission_request_without_a_tool_use_id_parses` holds the behaviour instead:
  a later `with =` or `deserialize_with` on the field that made it required would fail there.

- Denials are read from a tool result's text only when it is an error, by the prefixes CLI
  2.1.304 wrote: `PreToolUse:<tool> hook error:` is the guard, `<tool_use_error>Error calling
  tool (` a failed `approve` call, "The permission prompt tool returned an invalid permission
  result" a malformed reply, and "Permission to use ... has been denied" the mode. An `approve`
  deny's text is Eileen's own sentence, so it is counted from `result.permission_denials`
  instead. Whether an Eileen MCP tool's own error uses the same wrapper is not recorded; if it
  does, it counts as a denial, the safe direction, and `contract-claude` should record one.
- A tool input is redacted whole, then cut: cutting first could leave half a key or a private
  key block without its end marker, which the `Redactor` would no longer match. An input whose
  redacted text is over 2 KB becomes the first 2 KB of that text as a JSON string.
- Worker text and narration come from the whole assistant message, because narration needs
  whole lines; the brain speaks from partial deltas, so its text comes only from them and the
  whole message gives it nothing, and nothing is spoken twice.
- The context after a turn is the last request's input, cache and output tokens from
  `usage.iterations`, not the turn's totals, which count the cached prefix once per request
  (5,192 against 10,318 in the pt-allow run). `Cost.tokens_in` counts cache writes with plain
  input, and `tokens_cache_read` the reads.
- `WorkerEvent::Init.mcp_servers` holds `name:status` strings, since the pinned contract has
  `Vec<String>` for "the MCP servers and their status".
- A `Skill` call without a skill name stays a `ToolUse`, so it is never a `SkillInvoked` with an
  empty name.
- A failed sign-in is an errored `result` with HTTP status 401 or one of the texts the
  subscription-harnesses report recorded ("Not logged in", "Failed to authenticate",
  "Please run /login", and the API's `authentication_error`); it gives `AuthFailed` and then
  `TurnDone`, so the turn still ends.
- `serde` is not a dependency: the parser walks `serde_json::Value`, which tolerates the fields
  a CLI release adds, and an unused `serde` would fail cargo-machete.

## Outcome

Open.
