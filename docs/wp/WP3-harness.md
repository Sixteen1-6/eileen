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
   pool fit, interrupt and learning-loop row 7; the admission record is a W2 item. The interrupt
   check also confirms which `system/init` capability gates the interrupt (Eileen assumes
   `interrupt_receipt_v1`) and that the CLI answers it with a `control_response`, before
   `wp3/worker-brain` relies on either.
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

- `wp3/stream-json` (#135, merged): the typed stream-json parser of section 6.5, in a new
  `eileen_harness::stream` module. `parse` caps the line before parsing (`MAX_LINE_BYTES`,
  16 MB, since a tool result can carry a screenshot), gives `Unknown` for an unknown `type` or
  `system` subtype, keeps what `system/init` reports for feature detection (session id, model,
  tools, MCP servers with status, plugins, capabilities, `permissionMode`, `memory_paths.auto`
  as `Personal`, the CLI version), splits assistant text into text and `> ` narration, redacts a
  tool input with the `Redactor` and caps it at 2 KB as JSON text (a cut input is a JSON string
  whose quotes and escapes count), turns a `Skill` call into `SkillInvoked` with its name
  redacted and capped at 256 bytes,
  and types tool results, partial text deltas, rate-limit events, hook events,
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
- `wp3/control-message`: the stdio control half of stream-json, in `stream::control`. The
  stream parser gives `Line::Control` for `control_request`, `control_response` and
  `control_cancel_request`, so the stdio run's `can_use_tool` line is typed (tool, input
  redacted and capped as a tool input is, `tool_use_id`, the guard's reason) and no longer
  `Unknown`; it is parsed, not answered, since that route is the recorded fallback (section 2.5
  item 10 of the contract report). `control::Requests` writes the interrupt as
  `eileen_<n>`, only when `system/init` lists `interrupt_receipt_v1`, keeps at most four open,
  and matches a `control_response` (`success` or `error`) to an open request by `request_id`;
  a response with no id or an id over 128 bytes is malformed, one to an id that is not open
  settles nothing (`SettleError::NotOpen`), and `abandon` closes a request at the driver's
  deadline. Only `Requests` writes an interrupt; the line builder is private to the crate. The
  `control_message` fuzz entry checks the id cap, the tool input invariants, that a registered
  secret never survives in a reason or an error, that the stream parser agrees, that an
  interrupt round-trips with the same id and that, with `eileen_1` open, only a response to
  `eileen_1` settles it; its 14 seeds (the stdio run's requests and replies from
  `stdio-control.json`, plus hand-written lines, among them a reason and an error holding the
  registered secret and a reply to `eileen_1`) replay in `eileen-harness::it::fuzz_corpus`. The interrupt against the
  real CLI mid-turn is acceptance 3's owner-run check, still open; whether the CLI answers
  every interrupt with a `control_response` is part of it.
- `wp3/stream-fixes`: review fixes to the parser. A tool input and a `Skill` call's object
  arguments are redacted string by string, keys and numbers included, before they are
  serialized and cut (`stream::tests::a_token_at_the_start_of_a_line_in_an_input_is_redacted`),
  each until the redactor finds nothing more in it
  (`stream::tests::a_token_right_after_another_match_is_redacted`).
  A rate-limit line is a `RateLimit` for the `five_hour` or the `seven_day` window, any other
  window is `Unknown`, and `stream::PlanWindows` merges them into the contract's `PlanUsage`.
  `Line::denials` takes which calls the guard asked about and counts `ApproveFailed` only for
  those. The fuzz entry now also fails when a tool input that kept its shape holds anything the
  redactor would still replace, or a cut input or skill call holds a GitHub, API or Slack token.

- `wp3/session-files`: what each Claude session spawns with, in a new `eileen_harness::session`
  module. `session_files` turns a `SessionSpec` and the `HarnessConfig` (which gains
  `data_dir`, `hook_hold_s`, `shim_exe` and `mcp`) into the command line of section 2.5, the
  settings file and the MCP config under `<data_dir>/run/<session>/`, and the environment of
  section 2.12. The settings file holds `disableSkillShellExecution: true`,
  `fileCheckpointingEnabled: false`, the `PreToolUse` guard with `onFailure: "block"` and a
  10 s timeout, the `PermissionRequest` tripwire, and the seven bus hooks at 5 s each, every
  one sending `Bearer $EILEEN_HOOK_TOKEN` and `$EILEEN_SESSION_ID` through `allowedEnvVars`.
  The MCP config starts `eileen-shim` with the session's pipe and role and names its token as
  `${EILEEN_SHIM_TOKEN}`; approved third-party servers follow with Eileen's tokens blanked,
  and each one's rung comes back beside the files from `mcp.servers.<app>.rung`. A server
  whose app has no entry there, or whose entry does not list the session's job kind, is
  refused, and so is any third-party server on the brain. Every session gets
  `ENABLE_CLAUDEAI_MCP_SERVERS=false`,
  `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, `CLAUDE_CODE_DISABLE_FILE_CHECKPOINTING=1`, its id,
  both tokens, `TRACEPARENT` and `MCP_TOOL_TIMEOUT=(hook_hold_s + 30) * 1000`.
  `CLAUDE_FLAGS` and `CLAUDE_UNLISTED_FLAGS` are the constants the nightly drift job reads.
  Insta snapshots in `session::tests` hold the three files for a coding worker, a
  computer-use worker and the brain, and the other tests check the denylist and the
  pass-through list, the brain's allowlist, the timeout, that no token reaches a file or the
  command line, memory and checkpointing off everywhere, `Task,Agent` in `--disallowedTools`,
  and that no path lies under a `.claude` folder. This is the environment half of acceptance 4
  and the CI half of acceptance 8 ("every session spawns with vendor memory off").

- `wp3/claude-locate`: the CLI's location and `eileen doctor claude`. `eileen_harness::locate`
  walks the worker's `PATH` in order, then npm's folder under `%APPDATA%` and the native
  installer's `~/.local/bin`. It accepts only the two layouts section 2.5 names: npm's package
  binary, reached through a shim (`claude.cmd`, `claude.ps1`, or npm's link on Unix) read for
  the package's `bin\claude.exe`, and the native installer's `~/.local/bin` executable or one
  of its releases. Anything else is passed over and named: a shim that runs `cli.js` through
  node, the Claude desktop app's bundled copy (its `Claude-Data`, `AnthropicClaude` and MSIX
  folders, or any `claude-code\<version>` folder), a folder under `.claude`, and any other
  `claude` on `PATH` ("not an npm or native-installer copy"). Links and junctions are followed
  by reading each one, with every target refused by name (desktop app, `.claude`) before
  anything there is looked at. The version comes from the layout where it is exact: the
  package's `package.json`, or the release a Unix link leads to. The native installer's
  `claude.exe` on Windows is a copy of some release, not always the newest in its versions
  folder, so its version comes from `claude --version`.
  `cli::doctor_claude` builds a coding worker's environment with the same code a worker's
  session uses (`session::worker_env`) and looks for the CLI and the toolchains on its `PATH`.
  Until WP2's runtime reads the registry's `PATH` and the toolchain folders, that `PATH` is the
  shell's, and the report says so. Every probe is a short tool run through `eileen_supervisor`
  with a deadline. `claude auth status` (20 s) is read by its exit code only: 0 is signed in,
  and any other code is signed out, because only 0 is recorded (D4, the subscription harness
  report); the report names the code. Signed out, it prints the `claude auth login` suggestion
  and starts nothing else; signed in, it asks `claude --version` when the layout gave no
  version and looks for `cargo` and `node` on the worker's `PATH` and the MSVC tools through
  vswhere. It reports the version against the floor (`MIN_CLI`, 2.1.284) and the tested
  release (`TESTED_CLI`, 2.1.304), names the npm shim or the native installer and the
  executable, and compares the running sessions' versions with the installed one once WP2's
  status endpoint can report them (until then it says it did not check). The tests run against
  fake npm and native layouts in a temporary folder, a scripted probe runner, and the real
  supervised runner against this test binary run again as a chatty child. This is acceptance 5
  and the doctor half of the D77 floor. The owner's PC has the native installer (2.1.316 when
  this was written), so its run should report the native installer, the `.localin`
  executable, a version newer than the tested one, and signed in. Still to do there: that run,
  and recording here the exit code `claude auth status` gives when signed out.

- `wp3/stub-play`: `eileen-stub-harness` plays `StubScriptLine` scripts (section 6.5's item
  for it). The script is named by `--stub-script <path>` or, when no argument names one, by
  `EILEEN_STUB_SCRIPT`; every other argument is ignored, so `ClaudeWorker` can start the stub
  with the claude CLI's own argv. The whole script loads before anything plays, and a line
  that does not parse, a `crash` code outside 0 to 255, or a line that would write a line
  break inside its one stdout line, is a typed error naming the line. `event` lines are
  written as the script's own `json` text, byte for byte, found by a small scan of the line
  (a re-serialised value would sort its keys, and serde_json's `raw_value` feature would
  change how every crate built beside the stub parses an object holding its private marker
  key), `malformed` bytes as one line, a `narration` as an
  assistant text message whose lines start with `> `, `delay` waits, `crash` exits with its
  code, and `await_user` holds until the next stream-json user line; after the last line the
  stub waits for stdin to close, and a user line then (a turn the script lacks) is an error.
  A named thread reads stdin into a bounded queue, lines capped at 16 MB as the harness caps
  the CLI's, and a stdin line that is not the protocol ends the run with a typed
  `InputError`. An interrupt `control_request` gets a success `control_response` with its id
  whenever it arrives, during a delay included, in a shape `Requests::settle` accepts; any
  other request gets an error reply. That reply copies the `control_response` envelope Eileen
  sends the CLI (`stdio-control.json`, `sent_to_cli`) without its inner `response` object: no
  reply the CLI wrote to an interrupt is recorded yet, and acceptance 3's owner-run check
  records one to replace it. The stub answers and plays on: what the real CLI
  does to the turn after an interrupt is acceptance 3's owner-run check, and a script can
  then carry it. A `permission_request` line ends the run with `RunError::NotPlayed` and
  plays nothing after it, never a silent allow, until `wp3/approval-contract` plays the
  guard and `approve` (above). The first five scripts in `tests/harness-scripts/` (a worker
  job that writes `hello.txt` and narrates, a `Skill` call, a crash mid-turn, a malformed line
  and a two-turn brain conversation with text deltas) are derived from the approval contract
  test's recorded runs, and the folder's README names each one's source. Their `system/init`
  lines drop the fixtures' `$omitted` annotation and put back `cwd`, `slash_commands`,
  `skills`, `plugins` and `output_style` with placeholder values; the README lists the fields
  still left out. The brain script's
  `stream_event` lines are not recorded, since those runs did not ask for partial messages;
  the owner-run recordings replace them. Integration tests start the binary through
  `eileen_supervisor` and read its stdout back through `eileen_harness::stream`: each script's
  worker or brain events, the crash's exit code, the hold at `await_user` with interrupts
  answered before and between turns and during a ten-minute delay, the script from the
  environment under claude's argv, the typed errors, and a crash and a `permission_request`
  that stop the stub while its stdin is still open and still exit with their codes; a corpus
  test checks every script loads, passes the `Redactor` unchanged, holds no `$` annotation key
  and is named in the README. The `stub_script` fuzz target (section 3.3.16 asks one of every
  parser of its formats) runs scripts and stdin lines through `load` and the stdin reader;
  its seeds are the harness scripts cut to 4 KB and hand-written lines, replayed by
  `it::fuzz_corpus`. Part of acceptance 2; the pooled job and cancel come
  with `ClaudeWorker` running against the stub.

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
- Redacting a tool input's serialized text missed a token at the start of a line: serialized,
  the newline before it is the `n` of `\n`, and the `Redactor`'s token rules find no word start
  after a letter (only its card rule treats an escape's letter as a word break). The trace's log
  writer was never affected, because it redacts each string raw. The same rule also misses a
  token after a literal backslash and letter in raw text, such as `printf 'a\nghp_...'`;
  teaching `word_start` the card rule's escape case is an `eileen-trace` change, suggested in
  the pull request.
- One pass of the `Redactor` is not always enough: a token glued to another match, such as a
  registered secret followed by an AWS key and a GitHub token, has no word start until the
  match before it becomes a `]`-ended mark. The parser redacts each string until it settles,
  at most 8 times, and a string still changing then becomes `[redacted:unsettled]`. The
  trace's log writer redacts once, so a log can keep such a token.

- npm's shim names the real binary relative to its own folder (`"%dp0%\node_modules\..."`
  in the `.cmd`, `"$basedir/node_modules/..."` in the `.ps1`), so reading the shim finds the
  binary without running node. The desktop app keeps each bundled CLI in a
  `claude-code\<version>` folder, which no npm or native layout uses.

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
  instead. The wrapper names the called tool, so it is the CLI's text for any tool that
  failed, not one for `approve`: `Line::denials` takes a predicate for the calls the guard
  answered `ask` for (the driver knows them from `HookRequest::PreToolUse.tool_use_id`) and
  counts `ApproveFailed` only for those. A call that was asked about and then failed on its
  own, after an allow, still reads as a denial; `contract-claude` should record what the CLI
  writes for an Eileen MCP tool's own error, with and without an ask, to tell the two apart.
- A tool input is redacted whole, then cut: cutting first could leave half a key or a private
  key block without its end marker, which the `Redactor` would no longer match. Each string,
  key and number is redacted raw, as the trace's log writer does, and only then is the value
  serialized. An input whose text is over 2 KB becomes the first 2 KB of that text as a JSON
  string.
- A `rate_limit_event` reports one window, and `PlanUsage.resets_at` is the nearer window's
  reset, so the mappings give no `RateLimit` event; the driver (`wp3/worker-brain`) keeps one
  `PlanWindows` for the account, passes each `RateLimit` line and the current time to
  `PlanWindows::update`, and sends the merged `PlanUsage`. A window past its reset is left out
  until an event reports it again. The per-model windows (`seven_day_opus`,
  `seven_day_sonnet`) and any other window are `Unknown`, since `PlanUsage` has no field for
  them; a field would be an amendment to the pinned contract.
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

- The session files leave out what waits on WP3's owner-run probes or on later pull requests:
  `--restricted`, `--add-dir` with `CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD`, the notes
  hub's `--append-system-prompt-file`, `--fallback-model` (it needs the resolved alias of the
  next model down), the brain settings file's `alwaysThinkingEnabled` (WP3 measures which
  way of turning thinking off works first), the D77 receipt hooks (`PostToolUse` on the memory
  folders and `InstructionsLoaded`, moot while `MemorySpec` is off) and the `dev` toolset's
  shim. Each is one more flag or entry beside the ones built.
- The brain is the session whose toolsets hold `brain`: `SessionSpec` has no brain kind. It
  gets `--tools ""` and `--allowedTools mcp__eileen__*` whatever the spec lists, and its
  prompt through `--system-prompt`, which replaces the CLI's default prompt (section 2.3); a
  worker's goes through `--append-system-prompt`. A brain or librarian spec with the worker
  denylist is refused, so neither sees the user's environment. Section 2.12 gives the scribe
  the allowlist too, but `JobKind` has no scribe kind yet; it joins `needs_allowlist` when it
  gets one.
- The brain's command line keeps four things section 2.3's brain block does not list, on
  purpose: `--permission-mode dontAsk` with `--permission-prompt-tool mcp__eileen__approve`,
  so a tier-2 `act` tool the guard asks about reaches the same approval route as a worker's;
  the hooks in its settings file, so the `PreToolUse` guard fails closed for the brain too;
  `--include-hook-events`; and `--disallowedTools Task,Agent`, which section 2.5 gives every
  session. Its files sit under `run/<ws_id>/`, not `run\brain\`, because the hot spare runs
  beside the primary and each needs its own. The pull request lists these for an amendment of
  section 2.3.
- Every path a session is given is absolute: the data folder (`is_absolute`, so a Windows path
  with a root and no drive is refused), the shim, each third-party server's command and each
  plugin folder. A relative one would resolve from the worker's folder, a repository Eileen
  does not trust, and a bare command name would be searched for there and on `PATH`.
  `HarnessConfig::default()` has an empty data folder and shim, so a session never starts
  from it. The spec's `cwd` becomes `SessionFiles.cwd`, under the data folder when relative
  (core sends `run/<session>/cwd`), and is held to the same `.claude` rule.
- The CLI expands `${VAR}` in every MCP server's command, arguments and `env` from its own
  environment, which holds both tokens, so a third-party server with a `$` in its command or
  an argument is refused (`SessionError::ExpandableMcpArg`): blanking the tokens in its `env`
  does not stop an expansion in its arguments.
- Each hook posts to `http://127.0.0.1:<port>/<path>/<event>`, with `HookEndpoint.path` as the
  session's prefix and the event in kebab case (`pre-tool-use`, `stop-failure`), the layout the
  contract test used. WP2's endpoint serves these paths.
- The shim's token reaches it through `${EILEEN_SHIM_TOKEN}` in the MCP config's `env`, which
  the CLI expands from its own environment, and its pipe and role through `--pipe`, `--role`
  and, for a worker, `--session`; WP2's shim reads them. The drift job checks that the CLI
  accepts the generated config.
- The session's span in `TRACEPARENT` is the low 64 bits of its `ws_` ULID, so it is stable and
  needs no randomness.
- Whether two environment names are one variable is the host's rule,
  `HostEnv::names_ignore_case`, which the runtime sets to `cfg!(windows)`: on Windows `Path`
  is `PATH` and a session variable replaces every spelling of its name, while on Linux
  `http_proxy` and `HTTP_PROXY` both survive, as curl needs. The denylist matches any case on
  every host, so `github_token` goes on both.
- A subagent tool in a spec's `builtin_tools` or `allowed_tools` is dropped, not refused: the
  session still starts, without it.

## Outcome

Open.
