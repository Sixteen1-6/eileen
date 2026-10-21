# The approval contract, tested against the real CLI

Research pass, 2026-10-21, on branch `wp3/approval-spike`. Question: BUILD.md section 6.5 makes the approval contract test WP3's first task, and section 2.5 lists three ways a worker's tier-2 call could reach the user: the `PermissionRequest` hook, `--permission-prompt-tool mcp__eileen__approve`, or deny then steer. The [claude-mods report](2026-10-claude-mods.md) (section 4) proved the tier guard's `deny` and a dead guard port, but left the `ask` path untested, and decision [D43](../DECISIONS.md) left the choice to this test. Which route works on the installed CLI, in which permission mode, with what shapes on the wire, what happens when a piece fails, and what does a round trip cost?

This report is for whoever builds `eileen-harness`, the hook endpoint (WP2) and the `approve` tool behind `eileen-shim`. It covers the approval half of WP3's first task. The `--restricted` runs per session kind and the containment measurements for D40 are still owed (section 5).

## How to read this

- **[ran 10-21]**: run on the owner's PC today. Windows 11 Home 26200, Core Ultra 7 155H, 16 GB. Claude CLI 2.1.304 (`claude --version`), the owner's own signed-in install (`claude auth status` exit code 0 under the worker environment), model alias `haiku` resolved to `claude-haiku-5-5` in every `system/init`, `--effort low`.
- **[docs]**: read from the CLI's own `--help` output on this build.
- **[inferred]**: our reasoning, not tested.

Another run was building WP0 on the same PC, so every case waited until at least 2 GB of physical memory was free (`FreePhysicalMemory`), and only one Claude process ran at a time. Free memory at each start ranged from 2.1 to 4.4 GB (median 3.2 GB). The CLI ran at below-normal priority, as `eileen_supervisor` will start it. Latencies are therefore from a loaded machine.

## Verdict

1. **The guard's `deny` holds in every mode.** A `PreToolUse` HTTP hook answering `deny` stopped a Bash call that `--allowedTools` allowed, under `dontAsk`, `manual`, `acceptEdits`, `auto` and `bypassPermissions` alike. The model saw `PreToolUse:Bash hook error: <reason>`, no file was written, and `result.permission_denials` named the call [ran 10-21].
2. **The permission-prompt tool route works on 2.1.304.** `--help` no longer lists `--permission-prompt-tool` [docs], but the CLI accepts it. After the guard answered `ask`, the CLI called `mcp__eileen__approve` with `{tool_name, input, tool_use_id}`, where `tool_use_id` is the same id the guard saw. An `allow` ran the command; a `deny` left no file and showed the model the tool's message verbatim. It worked in every mode that runs tools (in `plan` the model made no call at all), held through a 30 s and a 125 s wait, and worked for a subagent's call too [ran 10-21].
3. **The `PermissionRequest` hook route does not work for the guard's asks.** After a `PreToolUse` `ask`, the `PermissionRequest` hook never fired, in `dontAsk` or in `manual` mode; with no prompt tool the call was denied at once with the guard's own reason as the error. The hook fired only for the CLI's own rule-based prompt (a command not in `--allowedTools`, in `manual` mode), and never in `dontAsk`. Its body carries no `tool_use_id` [ran 10-21].
4. **Deny then steer works.** The guard denied with "awaiting user approval appr_01TEST; do not retry until told"; the model stopped. A second user turn ("The user approved ... Retry it now") plus a one-shot grant in the guard ran the exact command [ran 10-21].
5. **Pin `--permission-mode dontAsk`.** In `dontAsk` only the guard's asks reach the prompt tool, and a command outside `--allowedTools` is denied without asking. In `manual` (which `system/init` reports as `default`) every unlisted command also reaches the prompt tool. `acceptEdits`, `auto` and `bypassPermissions` ran an unlisted `mkdir` with no question. `plan` ran nothing [ran 10-21].
6. **Every failure we could cause failed closed, but not every failure looks like a denial.** A dead, slow, erroring or malformed guard blocked the call under `onFailure: "block"` (and let it through without it). A malformed `approve` reply was denied. An `approve` call that errored, timed out or lost its server became a tool error, not a permission denial, and was missing from `permission_denials`. A missing approve server ended the whole session with exit code 1 at the first ask [ran 10-21].
7. **`updatedInput` is a second write path.** When the approve tool answered `allow` with a changed command, the CLI ran the changed command without calling the guard again. Eileen's dispatcher must answer only with the input it stored when the guard asked [ran 10-21].
8. **The hold is Eileen's to bound.** With no timeout set, the CLI waited on a silent `approve` call for more than 197 s, until the runner killed it. `MCP_TOOL_TIMEOUT=15000` in the worker's environment turned the same wait into a tool error after 15 s [ran 10-21].
9. **An ask costs about 45 to 70 ms on top of the guard.** The guard's round trip as the CLI sees it had a median of 23 ms (the guard itself took 0.4 ms). From the guard's `ask` reply to the approve server receiving the call: median 69 ms for the first ask in a process, 44 ms in a warm one; through the stdio control protocol, 63 ms. The approve server answered in under 2 ms. The user's own answer dwarfs all of it [ran 10-21].
10. **A fourth route exists.** `--permission-prompt-tool stdio` sends the same ask over stream-json as a `control_request` of subtype `can_use_tool` (the Agent SDK's protocol), carrying the guard's reason, its hook event and the `tool_use_id`; a `control_response` allowed or denied it [ran 10-21]. It is not documented as a public interface, so v0.1 keeps it only as the fallback if the MCP route breaks [inferred].

**Recommendation in one paragraph.** Spawn every worker with `--permission-mode dontAsk` and `--permission-prompt-tool mcp__eileen__approve`. The guard answers no decision for tiers 0 and 1, `ask` for tier 2 and `deny` for tier 3, with `onFailure: "block"`. The dispatcher accepts an `approve` call only when its `tool_use_id` matches a pending ask the guard issued for that session and its input matches the stored input, holds it up to `approvals.hook_hold_s` (120 s) while the user decides, and answers `{"behavior":"allow","updatedInput":<the stored input>}` or `{"behavior":"deny","message":...}` as one JSON text block. An approval that outlives the hold is answered `deny` with "awaiting user approval appr_<id>" and completes by deny then steer. The worker environment sets `MCP_TOOL_TIMEOUT` a little above the hold (150,000 ms) as a backstop. The harness checks `system/init` for `eileen` connected before binding a job, and counts errored `approve` calls as denials. Section 4 lists the BUILD.md amendment this pull request makes.

## 1. What was run

Everything ran from a throwaway folder under the session's scratch directory, one fresh project folder per case. Nothing was installed and no user-scope setting was touched. The scripts are in [fixtures/wp3-approval/tools/](fixtures/wp3-approval/tools/): `runner.py` (the cases, the loopback listener, the spawn), `approve_mcp.py` (the approve server), `latency.py` (the numbers in section 2.6) and `make_fixtures.py` (the redacted fixtures). To repeat a case, copy the folder somewhere outside the repository and run `python -I runner.py <case>` (or `list`).

**The worker shape.** Each case spawned the real `claude.exe` behind the npm shim (resolved from `%APPDATA%\npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe`) with the flags of BUILD.md section 2.5, minus the plugin directory and partial messages, which approvals do not touch. The exact argument list of the `pt-allow` case:

```
claude.exe -p --input-format stream-json --output-format stream-json --verbose --include-hook-events
  --model haiku --effort low
  --append-system-prompt "You are a test worker in a permissions experiment. Use only the tools the user
                          names. Never retry a denied call and never work around a denial."
  --setting-sources "" --settings <case>\settings.json
  --strict-mcp-config --mcp-config <case>\mcp.json
  --tools Bash --allowedTools "Bash(touch *)" --disallowedTools Task,Agent
  --permission-mode dontAsk --no-session-persistence --no-chrome
  --permission-prompt-tool mcp__eileen__approve
```

Cases varied `--permission-mode`, `--permission-prompt-tool` (absent, `mcp__eileen__approve` or `stdio`), `--permission-prompts none`, `--tools` and the hooks; [worker-command.json](fixtures/wp3-approval/worker-command.json) has the full shape and [cases.json](fixtures/wp3-approval/cases.json) every case's flags and outcome.

**The environment.** The worker policy of section 2.12, built in Python: the inherited environment minus every `CLAUDE*`, `ANTHROPIC*`, `MCP_*`, `OPENAI*`, `CODEX_*`, `XAI_*`, `DEEPSEEK*` and `AWS_*` variable and anything ending in `_API_KEY`, `_TOKEN` or `_SECRET`, keeping the pass-through list; then `ENABLE_CLAUDEAI_MCP_SERVERS=false`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, the two `GIT_CONFIG_*` pins, `EILEEN_SESSION_ID` and `EILEEN_HOOK_TOKEN` (a fresh random token per case). Sign-in held under it.

**The settings file.** `disableSkillShellExecution: true` and HTTP hooks to a loopback listener (127.0.0.1, a random port, inside the runner): `PreToolUse` with a 10 s timeout and `onFailure: "block"`, `PermissionRequest` in the route-1 cases, and `PermissionDenied`, `PostToolUse`, `PostToolUseFailure`, `Notification`, `SubagentStart`, `SubagentStop`, `Stop` and `StopFailure` with 5 s timeouts. Every hook sent `Authorization: Bearer $EILEEN_HOOK_TOKEN` and `X-Eileen-Session: $EILEEN_SESSION_ID` through `allowedEnvVars`; every request arrived with both interpolated and the token matching.

**The guard.** A stand-in for `claude_tool_tier`: `deny` when the Bash command contains `tier3`, `ask` when it contains `tier2` (unless a one-shot grant matches the exact command), no decision (`{}`) otherwise. `Agent` and `Task` get `deny` except in the subagent case.

**The approve server.** A stdlib-only stdio MCP server (newline-delimited JSON-RPC: `initialize`, `tools/list`, `tools/call`, `ping`), the shape the [rust-architecture report](2026-10-rust-architecture.md) found fastest to start. Its reply mode was set per case: `allow`, `deny`, no `updatedInput`, a rewritten command, not JSON, JSON without `behavior`, `isError`, exit mid-call, never answer, or answer after N seconds.

**The prompt.** One stream-json user line asking for one Bash call per listed command, in order, without retrying a denied call. A case "ran" a command only if its file appeared in the case's project folder; the model's own report was not trusted.

**The clock.** The runner, the listener and the approve server stamp `time.time_ns()`, so the three logs line up; the runner stamps each stdout line as it arrives.

38 cases ran, 38 Claude processes in total, 823 s of CLI time.

## 2. Results

### 2.1 The guard (behaviour 1)

The command was `touch tier0-allowed.txt`, listed in `--allowedTools`, in `dontAsk` mode, except for the first row.

| Case | Guard | Ran | What the model saw |
|---|---|---|---|
| `guard-deny` | `deny` (command `touch tier3-denied.txt`) | no | `PreToolUse:Bash hook error: Eileen: tier 3, blocked (test guard)` |
| `guard-dead-block` | nothing listening, `onFailure: "block"` | no | `...: failed; blocking because onFailure is "block"` then `connect ECONNREFUSED 127.0.0.1:<port>` |
| `guard-dead-open` | nothing listening, no `onFailure` | **yes** | `(Bash completed with no output)` |
| `guard-hang-block` | no reply within a 5 s timeout | no | `...: timed out; blocking because onFailure is "block"`; the hook event's outcome was `cancelled` |
| `guard-garbage-block` | 200 with a body that is not JSON | no | `HTTP hook must return JSON, but got non-JSON response body: this is not json` |
| `guard-500-block` | HTTP 500 | no | `HTTP 500 from http://127.0.0.1:<port>/v1/hooks/claude/pre-tool-use` |
| `guard-baddecision-block` | `permissionDecision: "maybe"` | no | `Hook JSON output validation failed - hookSpecificOutput.permissionDecision: Invalid option: expected one of "allow"\|"deny"\|"ask"\|"defer"`, then the whole reply |

Every blocked call was listed in `result.permission_denials`. `guard-dead-open` is the reason `onFailure: "block"` must be on every gating hook: without it a dead Eileen waves the call through.

### 2.2 Ask through the permission-prompt tool (behaviour 2)

The guard answered `ask` for `touch tier2-asked.txt`, listed in `--allowedTools`; `dontAsk` mode; `--permission-prompt-tool mcp__eileen__approve`.

| Case | Approve answered | Ran | In `permission_denials` | What the model saw |
|---|---|---|---|---|
| `pt-allow` | `{"behavior":"allow","updatedInput":<input>}` | yes | no | `(Bash completed with no output)` |
| `pt-deny` | `{"behavior":"deny","message":"Eileen: the user declined this (test)"}` | no | yes | `Eileen: the user declined this (test)` |
| `pt-allow-noinput` | `{"behavior":"allow"}` | yes, as asked | no | normal output |
| `pt-allow-rewrite` | `allow` with `tier2` replaced by `rewritten` in the command | **the rewritten command** (`rewritten-asked.txt`), with no second `PreToolUse` | no | normal output |
| `pt-garbage` | the text `this is not json` | no | yes | `The permission prompt tool returned an invalid permission result. Expected {behavior: 'allow', updatedInput?: object} or {behavior: 'deny', message: string}. ...` |
| `pt-wrongshape` | `{"decision":"approve"}` | no | yes | the same message |
| `pt-iserror` | `isError: true` | no | **no** | `<tool_use_error>Error calling tool (Bash): Eileen: approval service failed (test)</tool_use_error>` |
| `pt-exit` | the server process exits mid-call | no | **no** | `<tool_use_error>Error calling tool (Bash): Connection closed</tool_use_error>` |
| `pt-missing-server` | no MCP server in the config | no | no `result` event | the tool error `MCP tool mcp__eileen__approve (passed via --permission-prompt-tool) not found. Available MCP tools: none`, then the CLI **exited with code 1** |
| `pt-hang` | never answers | no | no `result` event | nothing for 197 s; the runner killed the CLI at 200 s |
| `pt-hang-mcptimeout` | never answers, `MCP_TOOL_TIMEOUT=15000` | no | **no** | after 15 s, `<tool_use_error>Error calling tool (Bash): MCP server "eileen" tool "approve" timed out after 15s</tool_use_error>` |
| `pt-hold-30s` | `allow` after 30 s | yes | no | normal output |
| `pt-hold-125s` | `allow` after 125 s, `MCP_TOOL_TIMEOUT=150000` | yes | no | normal output |

Three things follow. The prompt tool is the only place the ask surfaces: the stream has no event for it, so the guard's own pending ask and the `approve` call are Eileen's record. The rewrite row means `updatedInput` is trusted as given. And the three tool-error rows mean the harness must treat an errored `approve` call as a denial itself; the `result` event does not.

The CLI also keeps the prompt tool away from the model: with `--permission-prompt-tool mcp__eileen__approve`, `system/init` listed only `Bash`; without it, the same server's tool appeared as `mcp__eileen__approve` in the model's tool list. A missing server did not stop the session from starting (`mcp_servers` was simply empty), which is why the harness should check for `eileen` with status `connected` in `system/init` before it binds a job.

### 2.3 The other routes

| Case | Mode | Route | Ran | What happened |
|---|---|---|---|---|
| `pr-allow` | `dontAsk` | `PermissionRequest` hook set to answer `allow`; no prompt tool | no | The hook never fired. A `system` event of subtype `permission_denied` with `decision_reason_type: "hook"` and the guard's reason, and the model saw `Eileen: tier 2, asking the user (appr_01TEST)` |
| `pr-allow-manual` | `manual` | the same | no | The same: the hook never fired after a `PreToolUse` ask |
| `pr-unlisted-manual` | `manual` | the same, but the guard gave no decision and the command (`mkdir tier0-unlisted`) was not allowlisted | yes | The hook fired for the CLI's own prompt and its `allow` ran the command. The body has `permission_suggestions` and no `tool_use_id` |
| `pr-unlisted-dontAsk` | `dontAsk` | the same | no | The hook never fired; the mode denied the call (`decision_reason_type: "mode"`) |
| `pt-no-tool-host` | `dontAsk` | none (`--permission-prompts` left at `host`, no prompt tool, no SDK host) | no | Denied at once; the model saw the guard's ask reason as the error |
| `pt-none-prompts` | `dontAsk` | none (`--permission-prompts none`) | no | Denied at once with the CLI's own text: "this session has no approval surface ... do not retry it ... What required approval: Eileen: tier 2, asking the user (appr_01TEST)" |
| `steer` | `dontAsk` | the guard denies with "awaiting user approval appr_01TEST; do not retry until told"; after the turn's `result`, a grant and a second user turn | yes, on the second turn | The model stopped after the denial and retried only when told; the second `PreToolUse` got no decision and the allowlisted command ran |
| `stdio-allow` | `dontAsk` | `--permission-prompt-tool stdio`, the runner answering `control_response` allow | yes | `control_request` `can_use_tool` with `decision_reason`, `decision_reason_type: "hook"`, `decision_reason_hook_event: "PreToolUse"` and `tool_use_id` |
| `stdio-deny` | `dontAsk` | the same, answering deny | no | The model saw the deny message verbatim; listed in `permission_denials` |

So route 1 of BUILD.md section 2.5 is out on this CLI: the hook the plan would hold open is never called for the guard's asks. Route 3 works as written. The guard's ask reason is what the model reads whenever no route answers, so it should read correctly as a refusal too ("Eileen needs the user's approval for this").

### 2.4 Permission modes (behaviour 4)

Four commands per run, prompt tool answering `allow`: `touch tier0-allowed.txt` (allowlisted, no decision), `touch tier2-asked.txt` (allowlisted, guard `ask`), `touch tier3-denied.txt` (allowlisted, guard `deny`) and `mkdir tier0-unlisted` (not allowlisted, no decision).

| `--permission-mode` | `system/init` says | tier 0 allowlisted | tier 2 asked | tier 3 denied | unlisted `mkdir` |
|---|---|---|---|---|---|
| `dontAsk` | `dontAsk` | ran | approve called, ran | blocked | **denied without asking** |
| `manual` | `default` | ran | approve called, ran | blocked | approve called, ran |
| `acceptEdits` | `acceptEdits` | ran | approve called, ran | blocked | ran without asking |
| `auto` | `auto` | ran | approve called, ran | blocked | ran without asking |
| `bypassPermissions` (with `--allow-dangerously-skip-permissions`) | `bypassPermissions` | ran | approve called, ran | blocked | ran without asking |
| `plan` | `plan` | the model made no tool call and said plan mode forbids changes | | | |

`dontAsk` is the only mode where the prompt tool hears from the guard alone. Its denial text for an unlisted call tells the model it "*may* attempt to accomplish this action using other tools" in reasonable ways, so the worker prompt's "stop on refusal" line and the guard's own reasons matter: they are what keeps a worker from looking for another way.

### 2.5 The tool fence and the subagent case

`tools-fence` ran with `--tools Bash,Write --disallowedTools Write,Task,Agent`. `system/init` listed only `Bash` and `mcp__eileen__approve`: a tool named in `--disallowedTools` was removed outright, and `Read`, never named in `--tools`, did not exist. Asked to use Write, Read and Agent, the model said none existed and made no call.

`subagent` (recorded for v0.2, as section 6.5 asks) allowed `Agent` and `Task` in `--tools` and `--allowedTools`. `system/init` listed the tool as `Task`, but the model called it as `Agent` and the guard saw `tool_name: "Agent"`, so the harness's flag constant needs both names. The CLI launched the subagent in the background ("Async agent launched successfully") and the parent turn ended with its own `result` and `Stop` while the child ran. The child's calls went through the same guard, each `PreToolUse` body carrying `agent_id` and `agent_type`: its tier-2 `touch` reached the prompt tool and ran after `allow`, its tier-3 `touch` was blocked. A second `result` arrived when the child finished. Two facts matter for v0.2: the guard and the prompt tool do cover subagents, and one user turn can produce two `result` events, so "the first `result` ends the job" is wrong once subagents exist. The child's denial did not appear in either `result.permission_denials`. The child also left a small metadata file under the user's `.claude\projects` folder despite `--no-session-persistence` (section 6).

### 2.6 Latency (behaviour 5)

From [latency.py](fixtures/wp3-approval/tools/latency.py) over all runs. The guard rows leave out the timeout and dead-port cases; the approve row leaves out the hold and hang cases.

| Round trip | n | Median | 90th percentile | Range |
|---|---|---|---|---|
| Guard, as the CLI sees it (`hook_started` to `hook_response`) | 57 | 23.4 ms | 64.5 ms | 3.2 to 1,057 ms |
| Guard's own time (receive to reply) | 57 | 0.4 ms | 1.0 ms | 0.2 to 3.9 ms |
| Guard's `ask` reply to the approve server receiving `tools/call`, first ask in a process | 20 | 68.7 ms | 257 ms | 46 to 1,696 ms |
| The same, later asks in one warm process (`latency-5`) | 4 | 43.5 ms | 45.1 ms | 40 to 52 ms |
| Guard's `ask` reply to the stdio `control_request` arriving | 2 | 63.1 ms | | 62 to 64 ms |
| Approve server's own time | 19 | 1.8 ms | 4.0 ms | 0.9 to 10.1 ms |

The worst values came from the `mode-manual` run, which took 119 s on a heavily loaded PC. The command itself dominated every allowed call: `PostToolUse` reported 0.2 to 57 s of `duration_ms` for a single `touch` (median 2.0 s), because the first Bash call of a session starts Git Bash on a loaded PC. From the first user line to the first tool call took 1.8 to 19.6 s.

## 3. Wire shapes (behaviour 3)

The fixtures hold every shape below in full, redacted: the user's folder is `<home>`, the scratch folders `<spike>` and `<runs>`, a transcript path keeps only its file name, and the bearer token never reached a log. `system/init` keeps only the fields an approval parser reads, rate-limit events are left out (they describe the account's plan usage), and em dashes in the model's prose were replaced by hyphens.

| File | Holds |
|---|---|
| [cases.json](fixtures/wp3-approval/cases.json) | Every case: route, mode, guard answer, route answer, files created, `approve` calls, `permission_denials`, what the model saw, `init` tools and mode, exit code |
| [worker-command.json](fixtures/wp3-approval/worker-command.json) | The argument list, the settings file, the MCP config and the stream-json user line |
| [hooks.json](fixtures/wp3-approval/hooks.json) | `PreToolUse` bodies with the `ask`, `deny` and no-decision replies, one from a subagent, `PermissionRequest`, `PostToolUse`, `Stop`, `SubagentStart` and `SubagentStop`, with the headers seen |
| [mcp-approve.json](fixtures/wp3-approval/mcp-approve.json) | The approve server's whole conversation: the `server/discover` probe, `initialize`, `tools/list`, `tools/call` and the `allow` and `deny` replies |
| [stdio-control.json](fixtures/wp3-approval/stdio-control.json) | The `can_use_tool` `control_request` and the `control_response` for allow and deny |
| [stream-events.json](fixtures/wp3-approval/stream-events.json) | `system/init`, `hook_started`, `hook_response` (success and a failed block), `system/permission_denied` from a hook and from the mode, every failure case's `tool_result`, and `result` events |
| `stream-pt-allow.json`, `stream-steer.json`, `stream-stdio-allow.json` | Three whole runs in order, with times relative to the first line |

The shapes a parser needs, in short:

- **`PreToolUse` request** (HTTP POST body): `session_id`, `transcript_path`, `cwd`, `prompt_id`, `permission_mode`, `effort.level`, `hook_event_name`, `tool_name`, `tool_input`, `tool_use_id`, and in a subagent `agent_id` and `agent_type`. User-Agent `axios/1.15.2`. `transcript_path` points into the user's `.claude\projects` folder even with `--no-session-persistence`; no transcript was written there (the subagent case is the exception, section 6), and Eileen never opens the path.
- **`PreToolUse` replies**: `{}` for no decision; `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask"|"deny","permissionDecisionReason":"..."}}`. The stream's `hook_response` event echoes the reply in `output` and `stdout`, with the HTTP status as `exit_code` and `outcome` `success`, `error` or `cancelled`.
- **`approve` call** (`tools/call`): `params.name: "approve"`, `params.arguments: {tool_name, input, tool_use_id}`, and `params._meta: {"claudecode/toolUseId": <the same id>, "progressToken": <n>}`. It does not carry the guard's reason.
- **`approve` reply**: `result.content: [{"type":"text","text":"<JSON>"}]`, the JSON being `{"behavior":"allow","updatedInput":{...}}` or `{"behavior":"deny","message":"..."}`. The CLI's own error text names the accepted forms and an optional `updatedPermissions`, which Eileen never sends.
- **`PermissionRequest` request**: like `PreToolUse` but with `permission_suggestions` and without `tool_use_id`. Reply `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}`.
- **stdio**: `{"type":"control_request","request_id","request":{"subtype":"can_use_tool","tool_name","display_name","input","description","decision_reason","decision_reason_type","decision_reason_hook_event","decision_reason_hook_source","tool_use_id"}}`, answered on stdin with `{"type":"control_response","response":{"subtype":"success","request_id","response":{"behavior":"allow","updatedInput":{...}}}}`.
- **Denials in the stream**: the errored `tool_result` is always there; `result.permission_denials` lists `{tool_name, tool_use_id, tool_input}` for guard denials, route denials and malformed route replies, but not for an errored, timed-out or vanished `approve` call; a `system` event of subtype `permission_denied` (`tool_name`, `tool_use_id`, `decision_reason_type`, `decision_reason`, `message`) appeared only when an ask found no route or the mode denied.
- **The MCP handshake**: 2.1.304 first sends `server/discover` with protocol version `2026-07-28` in `_meta`; on method-not-found it falls back to `initialize` with `2025-11-25`. Answering the probe with a JSON-RPC method-not-found error was enough for the session to connect.

Of the bus hooks, only `PostToolUse`, `Stop`, `SubagentStart` and `SubagentStop` ever fired. `PermissionDenied`, `PostToolUseFailure` and `Notification` never fired in 38 runs, including every denial above [ran 10-21]; the harness reads denials from the stream.

## 4. What BUILD.md assumed, what we found, and the amendment

| BUILD.md assumed | Found on 2.1.304 | Amendment in this pull request |
|---|---|---|
| `--permission-prompt-tool` "is no longer listed as a flag", route 2 only "if the pinned CLI still accepts the flag" (2.5) | Accepted and working, though hidden from `--help` | Section 2.5 says so and pins the route |
| Route 1 first: the guard answers `ask`, then the `PermissionRequest` hook is held open (2.5) | The hook never fires after a `PreToolUse` ask, in either mode tried (`dontAsk`, `manual`); its body has no `tool_use_id` | Route 1 is recorded as not working; the hook stays as a tripwire that answers `deny` |
| `--permission-mode <pinned by WP3's contract test>` (2.5, 6.5) | `dontAsk` is the only mode where only the guard asks | Pinned to `dontAsk` |
| The approve tool's reply shape "recorded from the real CLI by WP3" (3.3.9, the contracts crate) | `{tool_name, input, tool_use_id}` in; one JSON text block out; `updatedInput` runs unchecked | Section 2.5 states the shapes and the rule that the reply's input is the stored one |
| The hold, `approvals.hook_hold_s = 120`, under the hook's timeout | The prompt tool's wait is unbounded unless `MCP_TOOL_TIMEOUT` is set; 125 s held fine | The hold applies to the `approve` call, with `MCP_TOOL_TIMEOUT` as the backstop, then route 3 |
| `PermissionDenied`, `PostToolUseFailure` and `Notification` hooks feed the bus (2.5) | None of them fired in 38 runs | Section 2.5 says the harness reads denials from the stream |
| Route 3 for approvals that outlive the hold | Works | Unchanged |
| The weekly and nightly contract test re-run allow, deny, guard deny, the fence, a dead endpoint and the subagent case | Malformed and failed `approve` replies, the rewrite and the mode matter too | The re-run list gains them |
| `HookRequest::PermissionRequest` carries `tool_use_id: String` (3.3.3 and `eileen-contracts`) | The CLI sends none | Carried to WP3: make it optional when WP3 next changes the contracts (section 6.5), so the spec and the frozen crate do not diverge in this docs pull request |

The amendment also updates the worker flag block, the approvals table row of section 3.3.8, the worker toolset row of 3.3.9, WP3's task text in 6.5 and the revision log.

## 5. Still owed by WP3

- `--restricted` per session kind, and the Windows Sandbox, AppContainer and restricted-token measurements for D40 (section 6.5). Not run here.
- The approval cases as `cargo xtask contract-claude` against Eileen's own shim and hook endpoint, with a clean environment built by `eileen-harness` rather than a Python stand-in. This report's fixtures and case list are its starting point.
- The `PermissionRequest` hook's own failure modes (hang, malformed reply, dead port). Not run, because the hook never fires on the approval path in the pinned mode.
- The stdio route's timeout and malformed replies, if the fallback is ever needed.
- The mid-turn `control_request` interrupt (acceptance 3), not part of this pass.

## 6. Cleanup

Every Claude, Python and Bash process this pass started was stopped; the runner killed one CLI on purpose (`pt-hang`, with its process tree). No session transcript was written under the user's `.claude\projects` folder, but the subagent case left one small file there despite `--no-session-persistence`: a 231-byte `subagents\agent-<id>.meta.json` inside a folder named after that case's project folder. It was not opened and is left for the owner to delete. For v0.2 this means a session with subagents writes into the user's Claude folder even when Eileen asks it not to. Nothing else was written outside the scratch folder except this report and its fixtures.
