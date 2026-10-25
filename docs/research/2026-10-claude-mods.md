# Claude's extension points, and how Eileen uses them

Research pass, 2026-10-20. Question: the owner said "Claude has mods now; if the mods can help this be better and we can change the app we should, as long as we still get latest updates" ([OWNER-DIRECTION.md](../OWNER-DIRECTION.md), 2026-10-20). What exactly are mods and Claude's other official extension points as of today, which of them make Eileen better, how is each built, and how does each keep working when Claude updates?

This report is for contributors building M4 (harness adapters, event bus, permission tiers) and the Claude-facing pieces after it. It builds on [2026-10-subscription-harnesses.md](2026-10-subscription-harnesses.md) (the `claude -p` stream-json route, the legal reading), [2026-10-brain-latency.md](2026-10-brain-latency.md) and [2026-10-learning-loop.md](2026-10-learning-loop.md) (learned skills as the `eileen-learned` plugin). It does not repeat them.

## How to read this

- **[ran 10-20]**: run on the owner's PC today (Windows 11 Home 26200, Core Ultra 7 155H, 16 GB, 1.6 to 2.5 GB free during the runs), Claude CLI 2.1.304.
- **[primary]**: read today from Anthropic's own docs, changelog, or the declaration file the CLI writes for this build.
- **[secondary]**: a search summary or a third party.
- **[inferred]**: our reasoning, not tested.

Nothing was installed into the owner's Claude app, and no user-scope settings were touched. Every test ran from a throwaway folder with session-only flags (section 4).

## Verdict

1. **"Mods" are real, documented and supported.** A mod is a plugin whose `hooks/hooks.json` names a JavaScript or TypeScript hooks module; Claude Code runs it inside its own process. It can draw a pane beside the transcript, a band above the prompt, a status entry and toasts; add slash commands that run with no model turn; watch, rewrite or refuse tool calls and prompts; and reach files, processes and the network through the host [primary, [mods overview](https://code.claude.com/docs/en/plugins/mods/overview)]. Mods are on by default from CLI 2.1.287 and Desktop 2.1.286 [primary].
2. **Mods do not change the app's files, so updates keep coming.** A mod lives in a plugin folder; Claude installs it through `/plugin` or `claude plugin install`, and Claude's own updater replaces the app around it. Nothing in this report patches `claude.exe`, the Desktop app's bundle, or turns off auto-update [primary, inferred].
3. **But the mod API is early access and moving fast.** The declaration file opens with "EARLY ACCESS: this surface may change between releases without notice" [primary, 2.1.304 types]. The changelog records at least six behaviour changes to it between 2.1.295 and 2.1.304 alone, ten releases [primary, CHANGELOG]. Anthropic can also turn installed mods off remotely, and a crashing hooks worker unloads them for the session [primary]. So a mod can be a convenience for the owner, never a part Eileen depends on.
4. **Settings hooks are the stable path, and they cover everything Eileen needs from its workers.** HTTP hooks POST each event to a URL; `PreToolUse` can deny, ask or rewrite; `onFailure: "block"` makes a guard fail closed [primary, [hooks](https://code.claude.com/docs/en/hooks)]. On this PC, hooks passed with `--settings` fired under `--setting-sources ""`, reached a loopback listener with a bearer token, and blocked a tool call; a dead guard port blocked a Write [ran 10-20].
5. **Where mods draw:** the terminal CLI, the Code tab of the Claude desktop app (not WSL sessions) and the VS Code extension. Under `claude -p` and the Agent SDK their hooks run but nothing draws. The desktop app's chat tab does not run mods or plugin hooks; it takes local MCP servers as desktop extensions instead [primary, platform table; secondary for desktop extensions].
6. **A mod inside an Eileen worker works today.** Loaded with `--plugin-dir`, a test mod's `session.start`, `tool.call` and `turn.complete` hooks ran under `claude -p`, reached loopback through `$.http.fetch`, and reported the owner's plan windows (five hour 32 percent, seven day 23 percent) from `$.session.usage()` without touching a credential [ran 10-20]. Useful, but stream-json already carries tool calls and a `rate_limit_event`, so Eileen does not need a mod in workers.
7. **Policy is clear and Eileen fits it:** unmodified binary; each user signs in themselves; no Claude Code or Anthropic name or logo in Eileen's product, feature or plugin names; plugin names starting `claude-`, `anthropic-` or `cc-plugin-` are refused by `claude plugin validate` [primary].

**Recommendation in one paragraph.** For v0.1, use the stable surfaces for everything Eileen relies on: each worker gets a per-job `--settings` file whose HTTP hooks send `PreToolUse` to Eileen's tier guard (fail closed) and report `Notification`, `PermissionDenied`, `PostToolUseFailure`, `SubagentStart`, `SubagentStop`, `Stop` and `StopFailure` to the loopback bus, alongside the stream-json Eileen already reads; learned skills stay in `eileen-learned` via `--plugin-dir`. Then ship one opt-in plugin named `eileen`, installed by the owner through Claude's own `/plugin` flow from a marketplace file in Eileen's repository: a handoff skill (`/eileen:handoff`, which passes `${CLAUDE_SESSION_ID}` to Eileen), Eileen's MCP tools for Claude sessions, and a small display-only mod (a status entry, a band with Mute and Hide, and an `/eileen` command answered with no model turn). The mod never approves, never answers `tool.check`, never submits prompts, and CI runs `claude plugin validate --strict` and `claude plugin test` on it against each new Claude release. The full pane, Eileen handing prompts into an open Claude session, a desktop extension for the chat tab, and a directory listing come in v0.2.

## 1. What Claude offers, as of 2026-10-20

### 1.1 Mods

A mod is three files plus optional types and tests [primary, [mods reference](https://code.claude.com/docs/en/plugins/mods/reference)]:

| File | Contents |
|---|---|
| `.claude-plugin/plugin.json` | The plugin manifest; `types` names the state contract when the mod keeps `$.state` |
| `hooks/hooks.json` | `{ "modules": ["./register.tsx"] }`, one path; may also hold ordinary settings hooks under `hooks` |
| `hooks/register.tsx` (or `.ts`, `.js`) | `export const register: Register = (on, options) => { ... }`, an ES module with no DOM and no Node; JSX compiles against `h` |
| `types/index.d.ts` | `interface PluginState` declarations for the mod's reactive state |
| `*.test.ts(x)` | Tests that `claude plugin test` runs against the engine, with no session and no model |

Every hook is `($, e, next)`: `$` is the engine interface, `e` the frozen event, `next(e)` the rest of the chain and Claude's own behaviour. A hook observes (`return next(e)`), rewrites (`next({ ...e, x })`) or answers (returns without `next`). The events cover tool calls (`tool.call`, `tool.check`, `tool.describe`), prompts and the system prompt (`prompt.submit`, `prompt.compose`, `prompt.section`, `skill.prompt`), commands, turns (`turn.start`, `turn.step`, `turn.complete`), the session (`session.start`, `session.end`, `session.append`, `session.receive`, `session.send`, `session.measure`), subagents, the interface (`ui.render` and presses), other mods (`plugin.register`), and every settings hook event as `classic.<Event>` [primary, types].

Render sites: `Pane` (terminal, desktop, VS Code), `AbovePrompt` (terminal, desktop), plus restyling of `UserMessage`, `AssistantMessage`, `ToolUse`, `Spinner`, `AskUserQuestion` and others. A mod cannot restyle the permission prompt [primary].

What `$` reaches, relevant to Eileen: `$.http.fetch` (http or https "to whatever the host reaches", 4 MiB bodies, 30 s per call), `$.process.run` and `spawn`, `$.fs`, `$.env`, `$.store` (4 MiB, shared across sessions), `$.clock.every`, `$.prompt.submit` (starts a turn when the session is idle), `$.session.id()`, `usage()` (context and plan rate-limit windows), `version()`, `$.ui.status`, `toast`, `notify`, `open`, and `$.audio.speak` (the platform synthesizer, hookable as an event) [primary, types].

**Where mods run** [primary, mods overview]:

| Where | Hooks run | Drawing appears |
|---|---|---|
| `claude` in a terminal | Yes | Yes |
| Claude desktop app, Code tab, local session | Yes | Yes, except terminal-only elements |
| Claude desktop app, WSL session | No (no plugins there) | No |
| VS Code extension | Yes | Yes, from extension 2.1.299 |
| `claude -p`, Agent SDK | Yes | No |
| Remote Control from the phone | Yes, in the session on the PC | In the terminal on the PC |
| Cloud session | For plugins that reach it | No |
| Claude chat (web, desktop chat tab, mobile) | No | No |

**Trust.** A mod runs with the user's permissions and is not sandboxed: it can read and write files, start programs, make network requests, read environment variables, rewrite prompts and tool calls, approve a tool call before the user is asked, and spend the user's plan [primary]. A built-in guard (`cc-plugin-sec-default`) loads ahead of user mods only on machines with managed settings or under a Team or Enterprise login. On a personal plan with no managed settings, which is the owner's case as far as we know [inferred], a user's mod can approve a call that an `ask` rule would prompt for [primary, [mods admin](https://code.claude.com/docs/en/plugins/mods/admin)]. Eileen's mod therefore must never answer `tool.check` or approve anything.

**Kill switches.** `--safe-mode` (one session), `disableAllHooks` (every installed mod and settings hook), disabling the plugin in `/plugin`; for organizations, `allowManagedModsOnly`, `allowManagedHooksOnly`, `disableSideloadFlags`. The built-in `plugin-authoring` entry is on "unless Anthropic has turned installed mods off remotely". If the hooks worker crashes three times, every non-built-in mod unloads until `/reload-plugins` [primary].

**Stability.** The declaration file says the surface "may change between releases without notice" [primary]. Between 2.1.295 and 2.1.304 the changelog lists: `.catch` handlers now run only on a throw or timeout (2.1.301); `$.env.set` resolves `{}` or `{ deny }` (2.1.301); `ui.render` trees carry `part: { plugin }` (2.1.299); `bold` deprecated for `weight` (2.1.299); `turn.start` now fires for subagents (2.1.299); `settings.read` hooks receive each layer (2.1.304); and validate no longer predicts where hooks run (2.1.304) [primary, CHANGELOG]. The docs ask authors to regenerate types per build and treat the declaration file as the authority.

### 1.2 Settings hooks

The settings-file kind, which the mod docs now call "settings hooks" [primary, [hooks](https://code.claude.com/docs/en/hooks)]:

- **33 events**: `SessionStart`, `Setup`, `UserPromptSubmit`, `UserPromptExpansion`, `PreToolUse`, `PermissionRequest`, `PermissionDenied`, `PostToolUse`, `PostToolUseFailure`, `PostToolBatch`, `Notification`, `MessageDisplay`, `SubagentStart`, `SubagentStop`, `TaskCreated`, `TaskCompleted`, `Stop`, `StopFailure`, `TeammateIdle`, `InstructionsLoaded`, `ConfigChange`, `CwdChanged`, `DirectoryAdded`, `FileChanged`, `WorktreeCreate`, `WorktreeRemove`, `PreCompact`, `PostCompact`, `PreModelSwitch`, `PostModelSwitch`, `Elicitation`, `ElicitationResult`, `SessionEnd`. The 2.1.304 types declare the same 33 [primary].
- **Five hook types**: `command`, `http`, `mcp_tool`, `prompt`, `agent` (agent hooks are experimental). `SessionStart` takes only `command` and `mcp_tool` [primary].
- **HTTP hooks**: `url`, `headers` (with `$VAR` interpolation only for names in `allowedEnvVars`), `timeout` (default 600 s for http), `onFailure` (`"block"` from 2.1.295), `if`, `statusMessage`, `once`. A 2xx JSON body is read like a command hook's output; non-2xx and connection failures are non-blocking unless `onFailure: "block"` [primary]. `allowedHttpHookUrls` and `httpHookAllowedEnvVars` restrict them [primary].
- **Decisions**: `PreToolUse` answers `hookSpecificOutput.permissionDecision` of `allow`, `deny`, `ask` or `defer` (defer only in `-p` with a single tool call), with `permissionDecisionReason` and `updatedInput`. Staying silent does not approve. `PermissionRequest` answers `decision.behavior`. In non-interactive runs, a `PermissionRequest` hook that returns no decision lets the call be denied [primary].
- **Notification** types include `permission_prompt`, `idle_prompt`, `agent_needs_input`, `agent_completed` and the quota auto-resume ones; these hooks cannot block [primary].
- **`mcp_tool` hooks** call a tool on a connected MCP server; an `isError` result is a non-blocking error, so they fail open [primary].
- **Async**: command hooks take `async: true` or `asyncRewake: true`; under `-p`, async hooks still running at teardown are killed [primary].
- **Locations**: user, project, local and managed settings, a plugin's `hooks/hooks.json`, skill and subagent frontmatter, and (verified here) the `--settings` flag [primary, ran 10-20].
- **In the stream**: `--include-hook-events` puts `hook_started` and `hook_response` events into stream-json; an HTTP hook's status code shows as `exit_code` [ran 10-20].

### 1.3 Plugins and marketplaces

A plugin bundles skills, commands, agents, hooks, a hooks module (a mod), MCP and LSP servers, output styles, workflows, monitors, `bin/` executables and `userConfig` options [primary, [manifest reference](https://code.claude.com/docs/en/plugins-reference)].

- **Loading**: `--plugin-dir <dir>` or `CLAUDE_CODE_PLUGIN_DIRS` for one session, as `<name>@inline`; a folder under `~/.claude/skills/<name>/` with a manifest loads in every session as `<name>@skills-dir`; marketplace installs at user, project or local scope as `<name>@<marketplace>`; claude.ai account plugins sync in as `<name>@synced` [primary, [loading](https://code.claude.com/docs/en/plugins/loading)].
- **Marketplace**: a `.claude-plugin/marketplace.json` in a git repository publishes with no submission. Install with `/plugin install <name> --marketplace <owner>/<repo>` in a session (2.1.275+) or `claude plugin install <name> --marketplace <owner>/<repo>` in a shell (2.1.292+). In the desktop app: + next to the prompt box, Plugins, Add marketplace [primary].
- **Updates**: the manifest `version` pins users until it changes; without it, a git marketplace uses the commit SHA. Auto-update is off by default for third-party marketplaces; updates land on disk and apply after `/reload-plugins` or a restart; old versions are kept 14 days [primary].
- **In place**: a relative-path plugin in a marketplace added from a local folder loads from that folder at every session start, with no reinstall [primary]. This suits a plugin Eileen keeps on disk.
- **Names**: validate errors on names starting `claude-`, `anthropic-`, `anthropics-`, `cc-plugin-`, on `claude`, `anthropic`, `anthropics`, `claude-code`, `claude-mods`, and on `official` beside `claude` or `anthropic`; it warns on `claude` as a whole word elsewhere [primary].
- **Where components load**: skills load in Claude chat, Cowork and Claude Code; hooks and agents only in Cowork and Claude Code; local MCP servers are ignored in chat; mods are listed for Claude Code only [primary, [platform support](https://claude.com/docs/plugins/platform-support)].

### 1.4 Agent Skills

A skill is a `SKILL.md` folder in the open Agent Skills format. Personal, project, nested, `--add-dir`, plugin and claude.ai-account locations; frontmatter includes `disable-model-invocation`, `user-invocable`, `allowed-tools`, `hooks`, `context: fork`, `paths`; bodies can use `${CLAUDE_SESSION_ID}`, `${CLAUDE_SKILL_DIR}` and `$ARGUMENTS`. Descriptions are always in context (each cut at 1,536 characters); bodies load on use [primary, [skills](https://code.claude.com/docs/en/skills)]. Plugin skills are fixed at session start, so a new learned skill reaches a worker only in its next session [ran 10-20, learning-loop report].

### 1.5 Status line and output styles

The status line is one `statusLine` command in settings that receives session JSON on stdin (including `rate_limits.five_hour` and `seven_day` for subscribers) and is debounced at 300 ms [primary, [status line](https://code.claude.com/docs/en/statusline)]. There is one per user, so Eileen taking it over would replace whatever the owner has; a mod's `$.ui.status` adds an entry beside the engine's notices instead. Output styles change the main system prompt for interactive sessions and load only in Claude Code; Eileen's workers already set `--system-prompt`, so output styles add nothing for v0.1 [primary, inferred].

### 1.6 Channels

A channel is an MCP server that pushes `notifications/claude/channel` events into a running session and can relay permission prompts. It is a research preview: custom channels need `--dangerously-load-development-channels`, which is ignored under `-p` and the SDK [primary, [channels reference](https://code.claude.com/docs/en/channels-reference)]. Not usable for Eileen until it leaves preview.

### 1.7 Agent SDK and stream-json

The Agent SDK runs the Claude Code binary as a library with hooks, skills and plugins; its in-process hooks travel over the same stdio control protocol (`hook_callback`) that `claude -p --input-format stream-json` speaks, which is not documented as a public API [primary, harness report]. Eileen drives the CLI directly from Rust, so the documented HTTP hooks are the right v0.1 choice; registering hooks over the control protocol (no port at all) is a later option once a CLI version range is pinned. The SDK's branding rules apply to Eileen either way (section 5).

### 1.8 Claude desktop app, chat tab

Mods and plugin hooks do not run in chat. Chat does take local MCP servers packaged as `.mcpb` desktop extensions, installed by the user under Settings, Extensions, Advanced settings, Install Extension; directory listings for `.mcpb` are deprecated in favour of plugins [secondary, search summary of support.claude.com and claude.com docs].

### 1.9 Summary table

| Extension point | Runs in | Stability | Distributed as | Survives a Claude update |
|---|---|---|---|---|
| Settings hooks (HTTP, command) | CLI, desktop Code tab, `-p`, SDK, Cowork | Documented; additions are versioned in the changelog | Settings file, `--settings`, plugin `hooks.json` | Yes; the config is ours, the app is untouched |
| Skills | Everywhere, chat included | Open standard plus documented extensions | Folder, plugin, claude.ai | Yes |
| MCP servers | Code: all; chat: as `.mcpb` | Open protocol | Plugin `.mcp.json`, `--mcp-config`, desktop extension | Yes |
| Plugins and marketplaces | Claude Code (part of each also in chat and Cowork) | Documented | Git repository with `marketplace.json` | Yes; updates of the plugin are separate from updates of the app |
| Mods | CLI, desktop Code tab, VS Code; hooks only in `-p` and SDK | Early access, changes between releases | Plugin with a hooks module | The app updates fine; the mod itself can break and is skipped |
| Status line setting | CLI | Documented | User settings | Yes, but one per user |
| Channels | Interactive Claude Code | Research preview | Plugin, dev flag | Not for us yet |

## 2. What "keep getting updates" means in practice

Nothing Eileen does touches the installed app. The rules:

1. Never patch, wrap, replace or repackage `claude.exe`, the desktop app's MSIX package or the copy of Claude Code it ships (`%LOCALAPPDATA%\Claude-Data\...\claude-code\2.1.304\`). The legal page requires the binary to run "as published by Anthropic" [primary].
2. Never set `DISABLE_AUTOUPDATER`, `DISABLE_UPDATES` or `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` in the owner's environment or settings. The last one also stops plugin auto-update and claude.ai plugin sync [primary].
3. Worker configuration is session-only: `--settings`, `--plugin-dir`, `--mcp-config`, `--setting-sources ""`. Nothing is written to `~/.claude/settings.json` for workers.
4. Anything in the owner's own Claude sessions arrives through the official install flow, which the owner runs or approves, and leaves through `claude plugin uninstall`.
5. Mods are kept out of Eileen's critical path. If a Claude update breaks the mod, Claude skips the failing hook with a dim line in the transcript and the session goes on; Eileen's workers, voice loop and HUD do not notice [primary, inferred].
6. A compatibility job in CI (section 6) runs `claude plugin validate --strict` and `claude plugin test` against the newest Claude Code nightly and on every pull request, so a breaking change shows up as a red build before the owner sees it.

## 3. Integrations for Eileen

| # | Integration | Gives the owner | Built with | Version |
|---|---|---|---|---|
| A | Worker hooks to the bus, and the tier guard | Every Claude job reports approvals, waits, failures and completion to Eileen; risky calls are stopped before they run | Per-job `--settings` file, HTTP hooks | v0.1 |
| B | Learned skills in every worker | Faster, cheaper repeats | `eileen-learned` via `--plugin-dir` (learning-loop report) | v0.1 |
| C | The `eileen` plugin for the owner's own sessions: handoff skill and MCP tools | "Eileen, take this over" from inside Claude; ask Eileen for status from Claude | Skill plus `.mcp.json`, installed from Eileen's repo marketplace | v0.1, opt-in |
| D | The display mod: status entry, band with Mute, `/eileen` | Eileen's state at a glance in the Claude window | Hooks module in the same plugin | v0.1, opt-in |
| E | Eileen pane with jobs and transcript | A live sidebar of Eileen's work | `$.ui.open` plus `Pane` render | v0.2 |
| F | Eileen hands a spoken request into an open Claude session | "Tell Claude to run the tests" while the Claude window has focus | Mod polling the bus, `$.prompt.submit` | v0.2 |
| G | Eileen in the desktop app's chat tab | Ask Eileen and see jobs from Claude chat | The same MCP server as a `.mcpb` desktop extension | v0.2 |
| H | Claude's speech through Eileen's voice | Any mod that speaks uses Kokoro or the cloned voice | Hook on `audio.speak` | Later |
| I | Marketplace listing for other people | The same integration for anyone who downloads Eileen | `marketplace.json` in the repo; Anthropic's directory later | Repo marketplace in v0.1; directory later |

### A. Worker hooks to the event bus, and the tier guard (v0.1)

**What it gives.** Today's ARCHITECTURE.md (section 5) plans "a tiny hook script that POSTs to `http://127.0.0.1:<port>/events`" as a backstop. HTTP hooks do this with no script and no process per event. Eileen also gets a decision point before every tool call in every Claude worker, including subagents [inferred for subagents], which is where the tiers in ARCHITECTURE section 7 can be enforced.

**How it is built.** For each job Eileen writes `%LOCALAPPDATA%\eileen\run\<job>\claude-settings.json` and spawns the worker with `--setting-sources "" --settings <that file>`, plus the flags in the harness report. The token never sits in the file: it rides in the worker's constructed environment as `EILEEN_HOOK_TOKEN` and is interpolated into the header.

```json
{
  "hooks": {
    "PreToolUse": [
      { "hooks": [ { "type": "http", "url": "http://127.0.0.1:<port>/v1/hooks/claude/pre-tool-use",
        "timeout": 10, "onFailure": "block",
        "headers": { "Authorization": "Bearer $EILEEN_HOOK_TOKEN", "X-Eileen-Job": "$EILEEN_JOB_ID" },
        "allowedEnvVars": ["EILEEN_HOOK_TOKEN", "EILEEN_JOB_ID"] } ] }
    ],
    "PermissionDenied":   [ { "hooks": [ { "type": "http", "url": "http://127.0.0.1:<port>/v1/hooks/claude/permission-denied", "timeout": 5, "headers": { "Authorization": "Bearer $EILEEN_HOOK_TOKEN", "X-Eileen-Job": "$EILEEN_JOB_ID" }, "allowedEnvVars": ["EILEEN_HOOK_TOKEN", "EILEEN_JOB_ID"] } ] } ],
    "PostToolUseFailure": [ "same shape, path /post-tool-use-failure" ],
    "Notification":       [ "same shape, path /notification" ],
    "SubagentStart":      [ "same shape, path /subagent-start" ],
    "SubagentStop":       [ "same shape, path /subagent-stop" ],
    "Stop":               [ "same shape, path /stop" ],
    "StopFailure":        [ "same shape, path /stop-failure" ]
  }
}
```

The quoted entries stand for the full object on the line above, with only the path changed.

The guard only ever narrows what Claude would do on its own:

| Eileen tier (ARCHITECTURE section 7) | Guard answer | What follows |
|---|---|---|
| 0 Observe, 1 Reversible, allowed for this job | `{}`, no decision | The worker's `--allowedTools` and permission mode decide, as now |
| 2 Consequential | `permissionDecision: "ask"` | Routed to `--permission-prompt-tool mcp__eileen__approve`, which asks the owner by voice or phone (harness report) |
| 3 Blocked, or unknown and risky | `permissionDecision: "deny"` with a one-line reason | The model sees the reason as an errored tool result and the denial lands in `permission_denials` |

The guard never answers `allow`, so a hook can never raise a permission (D14, and the owner's rule that a learned skill never skips a confirmation). With `onFailure: "block"`, a crashed or slow Eileen stops risky work instead of waving it through. The guard must answer in milliseconds; it is a lookup in the core, not a model call. Progress keeps coming from the stream-json events Eileen already parses; the hooks add the decision and the events the stream does not carry well (notifications, subagent boundaries, failures).

**How it survives updates.** It is documented configuration passed per session. New hook fields and events are announced with version numbers; the input fields Eileen reads (`session_id`, `tool_use_id`, `tool_name`, `tool_input`, `prompt_id`, `last_assistant_message`) were all present on 2.1.304 [ran 10-20].

### B. Learned skills in every worker (v0.1)

Already designed and verified in the learning-loop report: the `eileen-learned` plugin under `%LOCALAPPDATA%\eileen\learning\`, passed with `--plugin-dir`, skills namespaced `eileen-learned:<name>`, frontmatter allowlist, hot spare recycled after a change. This pass adds two checks: a plugin skill loads under `--setting-sources ""` next to a `--settings` file [ran 10-20], and a plugin can carry its own settings hooks that fire in the same run (the test plugin's `Stop` hook did) [ran 10-20].

For the owner's own interactive sessions, learned skills cost context in every session (every description is listed). With the learning loop's 300-character cap, 100 skills are about 30,000 characters, roughly 7,500 tokens per session [inferred]. So v0.1 does not put `eileen-learned` into the owner's own sessions. Later, the same folder can be registered as a local marketplace (`claude plugin marketplace add <folder>`, then install), which loads it in place, if the owner wants it and the count stays small [primary, inferred].

### C. The `eileen` plugin: handoff skill and MCP tools (v0.1, opt-in)

**What it gives.** Typing `/eileen:handoff finish the migration and tell me when tests pass` in any Claude Code or desktop Code-tab session hands the conversation to Eileen, who continues it as a job and reports back by voice. Claude can also call Eileen's tools: status, jobs, say something aloud, mute.

**How it is built.** One plugin folder in the repository, `integrations/claude-plugin/`, named `eileen`:

```
integrations/claude-plugin/
  .claude-plugin/plugin.json      name "eileen", displayName "Eileen", types ./types/index.d.ts
  .mcp.json                       { "mcpServers": { "eileen": { "command": "eileen", "args": ["mcp", "--for", "claude-session"] } } }
  skills/handoff/SKILL.md
  hooks/hooks.json                { "modules": ["./register.tsx"] }   (integration D)
  hooks/register.tsx
  types/index.d.ts
  tests/eileen.test.tsx
```

The handoff skill (validated in the draft, section 4):

```markdown
---
name: handoff
description: Hand this conversation to Eileen, who carries it on as a background job and reports back by voice. Use only when the person asks to hand the work to Eileen.
disable-model-invocation: true
argument-hint: "[what Eileen should do next]"
---

Hand this session to Eileen.

1. Write a summary of at most five lines: the goal, what is done, what is left, and any decision the person still owes.
2. Call the `handoff` tool on the `eileen` MCP server with `session_id` set to ${CLAUDE_SESSION_ID}, `cwd` set to the current working directory, `summary` set to that summary, and `next` set to: $ARGUMENTS
3. Reply with one line saying Eileen has it, then stop. Do not carry on with the work here.
```

`disable-model-invocation: true` means only the person can start it. On the Eileen side, `handoff` creates a job of kind `continue-session` that runs `claude -p --resume <session_id> --fork-session` in that `cwd` with the job's worker flags, so the owner's original conversation is left as it was [inferred; resume of a desktop-tab session needs a test]. Eileen never reads `~/.claude` to find the session (ARCHITECTURE section 8): the id comes from the skill.

The MCP server is the existing `eileen-mcp` crate run in a mode that exposes only the session-safe tools (`status`, `jobs`, `say`, `mute`, `handoff`, `ask`) and forwards them to the bus. It cannot approve an `ApprovalRequested`; approvals still need the owner's voice or a phone nonce (ARCHITECTURE section 10).

When the owner says "Eileen, take this over" out loud, Eileen knows the Claude window has focus (DesktopState) and asks the mod's session registry (integration D posts `session.start` with the session id and cwd) which session that is. Without the mod, Eileen answers "type /eileen:handoff in that window" instead of guessing.

**How it survives updates.** Skills, `${CLAUDE_SESSION_ID}`, MCP and `--resume`/`--fork-session` are documented and stable. The plugin installs and updates through Claude's own flow.

### D. The display mod (v0.1, opt-in, in the same plugin)

**What it gives.** In the terminal and the desktop Code tab: a status entry under the prompt (`Eileen: listening, 2 jobs`), a one-line band above the prompt with what Eileen last heard and Mute and Hide buttons, and `/eileen`, which prints mute state, the last utterance and each job with its progress, instantly, with no model turn and no tokens.

**How it is built.** Eileen writes `%LOCALAPPDATA%\eileen\bus.json` (`{ "port": n, "token": "<per boot>" }`, readable by the user only) at start. The mod reads it with `$.fs.read`, polls `GET /v1/state` every 2 s with one poll in flight at most, keeps the result in `$.state`, and posts `POST /v1/control/mute` from the button. The full module is in the draft folder (section 4); its validation summary:

```
./register.tsx hooks: session.start, command.run{command=eileen}, ui.render{component=AbovePrompt}
./register.tsx answers its own command: command.run{command=eileen}
./register.tsx calls: $.clock.every, $.command.register, $.env.get, $.fs.read, $.http.fetch, $.state.get, $.state.set, $.ui.resolve, $.ui.status, $.ui.toast
./register.tsx env reads: LOCALAPPDATA
```

No gating hooks, no `tool.call`, no `tool.check`, no `prompt.submit`, no model calls. That list is what the README shows people before they install it, and what a CI check pins: a pull request that adds a gating hook or a new `$` call to the mod fails until a reviewer accepts it.

**How it survives updates.** It uses a small, central part of the API (`session.start`, `command.run`, `AbovePrompt`, `$.state`, `$.http.fetch`, `$.ui.status`), it fails quiet (Eileen down means the band and status disappear), and CI runs its tests against each new Claude release. If an update breaks it anyway, Claude skips it and nothing else changes.

### E. Eileen pane (v0.2)

A `Pane` opened by `/eileen pane` (or unasked from 144 columns, the engine's rule) listing jobs with progress, the running transcript of what Eileen heard and said, and what it is watching. Same polling and state as D. Deferred because Eileen's own HUD (rust-architecture report) is the primary visual layer, the pane is the larger API surface, and the HUD's state model should settle first so both draw from one `eileen-contracts` type.

### F. Eileen hands a request into an open Claude session (v0.2)

The mod polls `GET /v1/claude/inbox?session=<id>`; when the owner says "tell Claude to run the tests" while that window has focus, Eileen queues the words and the mod calls `$.prompt.submit({ text })`. Without `asUser`, Claude reads it after a sentence naming the mod as the sender, which keeps the provenance honest [primary]. Rules: only the owner's own spoken words go through, never text read from the screen; the HUD shows what was sent. Deferred because it puts the mod on a path that acts, which needs the v0.1 mod to have survived a few Claude releases first.

### G. Eileen in the desktop app's chat tab (v0.2)

Package the same session-safe MCP server as an `.mcpb` desktop extension the owner installs from Settings, Extensions. Chat then gets `status`, `jobs`, `say` and `ask`. No mods are involved, so this is stable; it waits for v0.2 only because the chat tab is not where the owner's benchmark tasks run.

### H. Claude's speech through Eileen's voice (later)

`$.audio.speak` is an event: a mod hooked on it can send the text to Eileen's TTS (Kokoro or the chosen Chatterbox voice) and answer for the platform synthesizer. Small, but it only matters once other mods speak.

### I. Marketplace for other people (v0.1 repo, directory later)

A `.claude-plugin/marketplace.json` at the root of `Sixteen1-6/eileen`, listing `eileen` at `./integrations/claude-plugin`, makes the repository a marketplace with no submission [primary]. Eileen's installer offers it, and `eileen claude install` prints the two lines and runs them only after the user says yes, since they write the user's Claude settings:

```
claude plugin marketplace add Sixteen1-6/eileen
claude plugin install eileen@eileen
```

or, inside a session, `/plugin install eileen --marketplace Sixteen1-6/eileen`. `eileen claude uninstall` runs `claude plugin uninstall eileen@eileen`. Each person who installs it uses their own signed-in Claude; Eileen never brokers anyone's account. Anthropic's directory is later, after the policy review in section 5.

### Not doing

- Patching or replacing the app, its bundled Claude Code, or `app.asar`; injecting into the desktop app's web view.
- Turning off Claude's updater or nonessential traffic in the owner's environment.
- Taking over the owner's `statusLine` setting.
- Channels, until they leave research preview.
- `--bare` for workers (it drops subscription sign-in, harness report).
- A mod that approves tool calls, answers `tool.check`, or calls `$.session.authorize()` or `$.model.complete`.

## 4. What was verified on this PC

All work under `C:\Users\shubh\AppData\Local\Temp\claude\C--Users-shubh-Projects--claude\8b2a7b12-9d74-4cce-a7ac-9d5f3ba7299c\scratchpad\claude-mods\`:

| Path | What it is |
|---|---|
| `listener\listener.py` | Stdlib loopback listener in a uv venv (`.venv`, Python 3.13): 127.0.0.1:47615 only, bearer token check, one JSON line per request, a deny for any command containing `eileen-blocked` |
| `project\.claude\settings.json` | Project-scope HTTP hooks: `UserPromptSubmit`, `PreToolUse` (`Bash\|PowerShell`, `onFailure: "block"`), `PostToolUse`, `Notification`, `Stop` |
| `project\.claude\skills\eileen-probe\SKILL.md` | Project-scope test skill |
| `worker-settings.json` | Flag-settings HTTP hooks: `PreToolUse` on Bash to the listener, `PreToolUse` on Write to a dead port 47699 with `onFailure: "block"`, `PostToolUse`, `Stop` |
| `eileen-test\` | Test plugin: a skill (`open-resolve-at-timecode`), a plugin `Stop` HTTP hook, and a mod reporting `session.start`, `tool.call` and `turn.complete` to the listener; `tests\bus.test.ts` |
| `eileen-draft\` | The proposed v0.1 `eileen` plugin: mod (integration D), handoff skill, `.mcp.json`, `marketplace.json`, types, `tests\eileen.test.tsx` |
| `runs\` | `run-a.jsonl`, `run-b.jsonl` (stream-json), `events.jsonl` (listener log) |

Common flags: `--model haiku` (resolved to `claude-haiku-5-5`), `--effort low`, `--strict-mcp-config` with no MCP config, `ENABLE_CLAUDEAI_MCP_SERVERS=false`, `--no-session-persistence`, `--allowedTools "Bash(echo *)"` (plus `Write` in run B), `--permission-mode dontAsk`, `--output-format stream-json --verbose --include-hook-events`. One Claude process at a time.

| Check | Result |
|---|---|
| `claude plugin validate ./eileen-test` | Passed; listed hooks, calls and env reads; flagged `tool.call` as a gating hook without `.catch` (correct for an observer that should fail open) [ran 10-20] |
| **Run A**: project scope, `--setting-sources project` | Project skill listed in `system/init`. HTTP hooks fired for `UserPromptSubmit`, `PreToolUse` twice, `PostToolUse` and `Stop`, all with the token and `X-Eileen-Job` header interpolated. `echo eileen-allowed` ran; `echo eileen-blocked` was denied by the listener, the model saw "PreToolUse:Bash hook error: Eileen: tier 3 in this test, blocked by the loopback guard", and the result's `permission_denials` named it. 12.6 s total, 3.4 s API, 3 turns [ran 10-20] |
| **Run B**: worker shape, `--setting-sources "" --settings worker-settings.json --plugin-dir eileen-test` | `system/init` listed `eileen-test@inline` and `eileen-test:open-resolve-at-timecode`. Flag hooks fired with empty setting sources. Write blocked: "failed; blocking because onFailure is "block"; connect ECONNREFUSED 127.0.0.1:47699"; no file written. The plugin's own `Stop` hook fired. The mod's hooks ran under `-p`: `session.start` (`surface: null`, `isInteractive: false`, version 2.1.304 built 2026-10-19T18:11:48Z), two `tool.call` reports (the Write arrived as `isError: true`, not as a deny, because the settings hook sits beneath mods), and `turn.complete` with context 37,365 of 1,000,000 tokens and rate limits five hour 32 percent, seven day 23 percent. 12.3 s total, 4.5 s API, 16 s wall [ran 10-20] |
| `claude plugin test ./eileen-test` | 2 pass in 1.06 s, no model call [ran 10-20] |
| `claude plugin validate --strict ./eileen-draft` | Passed (after adding a marketplace `description`); no gating hooks other than its own command [ran 10-20] |
| `claude plugin test ./eileen-draft` | 3 pass in 1.57 s: `/eileen` with Eileen down, `/eileen` with state, the band yielding to the engine on terminal and desktop surfaces [ran 10-20] |
| `claude plugin details` | Works only for installed plugins; not run, to avoid installing anything |

Caveats:

- My `claude -p` runs inherited this desktop session's environment (`CLAUDE_CODE_ENTRYPOINT`, `ANTHROPIC_BASE_URL` and about thirty more names; values not read), which is why run B's `startup_timing` says `entrypoint: claude-desktop` and the tool list included desktop-host tools. Hook and plugin behaviour does not depend on it, but Eileen must spawn workers with a constructed environment, as the learning-loop report already says.
- `PostToolUse` reported 5,206 ms for `echo` through the Bash tool on the loaded PC. Not this report's topic, but worth knowing for worker latency.
- Not tested: the `ask` path through `--permission-prompt-tool`; hooks on subagent tool calls; the draft mod in a live interactive session (that needs either installing it into the owner's app or an interactive session, both out of bounds here); `--resume --fork-session` on a desktop-tab session.

Cleanup: listener stopped (port 47615 closed), no Claude or Python process of this run left running, nothing written outside the scratchpad except this report.

## 5. Policy and naming

**Running Claude Code inside Eileen.** The [legal and compliance page](https://code.claude.com/docs/en/legal-and-compliance) requires the binary to be unmodified with no built-in sign-in method removed, each end user to authenticate with their own subscription or key, and no reselling or intermediating of usage. It "does not prevent an end user from signing in to the unmodified Claude Code binary with their own Claude subscription" [primary]. Eileen fits: the owner signs in himself, Eileen spawns the installed CLI, and every other person who installs Eileen signs in their own CLI.

**Names.** You may "accurately say, in plain text," that a product runs Claude Code, but "can't use the Claude Code or Anthropic names or logos as part of your own product, feature, or company name, in your own logo, or in a way that suggests Anthropic built, endorses, or is partnered with your product" [primary]. The Agent SDK rules allow "Claude Agent", "Claude" inside a menu labelled "Agents" and "<Name> Powered by Claude", and forbid "Claude Code", "Claude Code Agent" and Claude Code-like ASCII art [primary]. For Eileen:

| Thing | Name |
|---|---|
| Integration plugin | `eileen`, display name "Eileen" |
| Learned skills plugin | `eileen-learned` |
| Marketplace | `eileen` |
| Slash command, pane title, MCP server | `/eileen`, "Eileen", `eileen` |
| Worker route in Eileen's UI | "Claude" under an "Agents" heading (harness report) |
| Plain-text docs | "works in Claude Code and the Claude desktop app" is fine as a statement of fact |
| Never | "Claude Code Eileen", "Eileen for Claude Code", "claude-eileen", "Claude mods by Eileen", any Claude or Anthropic logo |

`claude plugin validate` enforces part of this: it refuses `claude-*`, `anthropic-*`, `cc-plugin-*` and similar, and warns on `claude` as a whole word [primary].

**Usage.** Pro and Max limits "assume ordinary, individual usage of Claude Code and the Agent SDK" [primary]; worker volume counts against the owner's plan, and the plan windows Eileen can now read (run B) should drive throttling.

**Anthropic's directory.** The [software directory policy](https://support.claude.com/en/articles/13145358-anthropic-software-directory-policy) (dated 2026-04-15) says listed software "must not collect extraneous conversation data, even for logging purposes", needs a privacy policy link when it collects user data, "must not direct Claude to dynamically pull behavioral instructions", and must keep instructions human-readable [primary]. Submissions need a public repository and a paid plan; mods are listed for Claude Code only [primary]. The public `eileen` plugin is built to pass this: it records nothing, its mod is display only, its skill acts only on an explicit request. The learning loop's recording lives in Eileen's own process and worker configuration, never in the public plugin, and learned skills are never pulled into the owner's sessions at runtime. A directory submission still waits for a careful read of the full review criteria, which this pass could not fetch in full.

**Trust disclosure.** Mods run unsandboxed with the user's permissions [primary]. The README shows the plugin's `claude plugin validate` output (hooks, calls, env reads) so people see exactly what it does before installing.

## 6. Changes for BUILD.md and the work packages

1. **M4, harness adapter (Claude route):** generate a per-job `--settings` file with HTTP hooks (integration A) and spawn workers with `--setting-sources "" --settings <file> --include-hook-events`; token in `EILEEN_HOOK_TOKEN`, job id in `EILEEN_JOB_ID`, both through `allowedEnvVars`; explicit `timeout` on every hook (the http default is 600 s).
2. **M4, permission tiers:** the `PreToolUse` guard returns no decision for tiers 0 and 1, `ask` for tier 2, `deny` for tier 3; never `allow`; `onFailure: "block"`; acceptance test is run B's two cases (listener deny, dead port blocks).
3. **M4, event bus:** add `/v1/hooks/claude/<event>` endpoints mapping to `ApprovalRequested`, `TaskBlocked`, `TaskProgressed`, `TaskCompleted` and `TaskFailed`; add `GET /v1/state`, `POST /v1/control/mute`, `POST /v1/handoff`; write `%LOCALAPPDATA%\eileen\bus.json` with port and per-boot token, user-only ACL; no endpoint reachable with that token can approve anything.
4. **ARCHITECTURE section 5 amendment, with a decision record:** replace "a tiny hook script" with HTTP hooks from a generated settings file; stream-json stays the primary progress path; hooks are the decision point and the backstop. New decision: "Claude integration only through official extension points; mods are never in the critical path."
5. **Worker environment:** constructed, not inherited (seen in section 4).
6. **New package, `integrations/claude-plugin/`:** the `eileen` plugin from the draft (mod D, handoff skill, `.mcp.json`, types, tests), plus `.claude-plugin/marketplace.json` at the repository root listing it by relative path.
7. **`eileen-mcp`:** a `--for claude-session` mode exposing only `status`, `jobs`, `say`, `mute`, `handoff`, `ask`.
8. **Job kind `continue-session`:** `claude -p --resume <id> --fork-session` in the given cwd, with acceptance test on a desktop-tab session.
9. **CLI:** `eileen claude install` and `eileen claude uninstall`, which print and, after a yes, run the official commands; `eileen doctor` reports the installed Claude version against the tested range.
10. **CI, new `claude-compat` job:** install the Claude CLI (npm) on a Windows runner, run `claude plugin validate --strict integrations/claude-plugin` and `claude plugin test integrations/claude-plugin` on every pull request (pinned version) and nightly (latest); fail if the validate report gains a gating hook or a new `$` call without a reviewed allowlist change.
11. **Lint:** a docs and UI-string check that rejects "Claude Code" used as part of an Eileen name (for example "for Claude Code", "Claude Code Eileen").
12. **ROADMAP:** M4 gains integration A; M6's demo shows the band and `/eileen:handoff`; v0.2 lists E, F, G and the directory review; later lists H and channels.

## 7. Open risks

1. **Mod API churn.** At least six behaviour changes in ten releases; a remote off switch; unload after three worker crashes. Mitigated by keeping the mod small and display only, CI on new releases, and nothing depending on it.
2. **Untested paths.** `ask` through `--permission-prompt-tool` combined with the HTTP guard; hooks on subagent tool calls; the mod in a live desktop-tab session; resuming a desktop-tab session from `-p`.
3. **CI without sign-in.** Whether `claude plugin validate` and `claude plugin test` run on a CI machine with no Claude sign-in is unverified [inferred: neither makes a model call].
4. **No guard on personal plans.** Without managed settings, any mod the owner installs (not only Eileen's) can approve tool calls past `ask` rules. Eileen's mod stays non-gating; the owner should review any other mod before installing.
5. **Bus token exposure.** Any process or mod running as the user can read `bus.json`. The bus must treat that token as status and control only: approvals keep requiring the owner's voice or a phone nonce.
6. **Directory policy.** If Eileen ever submits to Anthropic's directory, "extraneous conversation data" and "dynamically pull behavioral instructions" need a careful reading against the learning loop; keep those parts out of the public plugin.
7. **Context cost of learned skills** if they ever reach the owner's own sessions.
8. **Inherited environment in tests.** Both runs used this desktop session's environment; a clean-environment repeat from Eileen's own process is still owed.

## Sources

Anthropic, read 2026-10-20: [mods overview](https://code.claude.com/docs/en/plugins/mods/overview); [mods reference](https://code.claude.com/docs/en/plugins/mods/reference); [manage mods for your organization](https://code.claude.com/docs/en/plugins/mods/admin); [hooks reference](https://code.claude.com/docs/en/hooks); [plugin manifest reference](https://code.claude.com/docs/en/plugins-reference); [plugin loading](https://code.claude.com/docs/en/plugins/loading); [publish and distribute a plugin](https://code.claude.com/docs/en/plugins/publish); [skills](https://code.claude.com/docs/en/skills); [status line](https://code.claude.com/docs/en/statusline); [channels reference](https://code.claude.com/docs/en/channels-reference); [legal and compliance](https://code.claude.com/docs/en/legal-and-compliance); [Agent SDK overview and branding](https://code.claude.com/docs/en/agent-sdk/overview); [plugin feature support across platforms](https://claude.com/docs/plugins/platform-support); [software directory policy](https://support.claude.com/en/articles/13145358-anthropic-software-directory-policy); [Claude Code CHANGELOG](https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md) (2.1.295 to 2.1.304); the declaration file Claude Code 2.1.304 writes for the plugin-authoring skill (`types/claude-code.d.ts`, 22,593 lines) and that skill's `reference.md`.

Secondary: search summaries of [local MCP servers on Claude Desktop](https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop) and [build a desktop extension with MCPB](https://claude.com/docs/connectors/building/mcpb).

Eileen: [2026-10-subscription-harnesses.md](2026-10-subscription-harnesses.md), [2026-10-brain-latency.md](2026-10-brain-latency.md), [2026-10-learning-loop.md](2026-10-learning-loop.md), [2026-10-rust-architecture.md](2026-10-rust-architecture.md), [ARCHITECTURE.md](../../ARCHITECTURE.md).
