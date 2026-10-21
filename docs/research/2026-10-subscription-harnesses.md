# Driving paid subscriptions through official harnesses

Research pass, 2026-10-11. Question: how does Eileen use the subscriptions the owner already pays for (Claude, ChatGPT, Google AI, SuperGrok) through each vendor's own signed-in harness, with DeepSeek as the only API key, from a Rust core? And what should the fast conversational "brain" run on?

This updates [RESEARCH.md section 3](../RESEARCH.md#3-licensing-and-subscriptions-settled) and [section 6](../RESEARCH.md#6-orchestration-and-routing) (both 2026-09-19), and decisions D3, D4 and D9 in [DECISIONS.md](../DECISIONS.md). It is written for contributors who will build the harness layer and the brain.

## How to read this

Every claim carries a tag:

- **[ran]**: run or measured on the owner's PC on 2026-10-11 (Windows 11 Home 26200, Core Ultra 7 155H, 16 GB RAM, Rust 1.99.0, Node 24.13.0, Python 3.13 in a uv venv).
- **[primary]**: vendor docs, blog, changelog, repository, npm registry, or the ACP registry.
- **[secondary]**: press, community forums, third-party guides. Treat as soft.
- **[inferred]**: our reasoning from the above.

Two things limit what was measured, and both matter when reading the numbers:

1. **The owner's standalone `claude` CLI is not signed in.** `claude auth status` returns `"loggedIn": false` on the installed 2.1.175 build, and the 2.1.294 build bundled with the Python Agent SDK reports `Failed to authenticate: OAuth session expired and could not be refreshed` [ran]. Claude Code sessions inside the Claude desktop app authenticate through the host app, not through the CLI's own login, so being signed in to the desktop app does not make `claude -p` work [ran, inferred]. The research rules forbid running a login flow, so **no model turn completed**. Everything up to the API call was measured (process start, protocol handshakes, MCP attach, session creation, per-turn harness overhead up to the auth failure). Model latency comes from published benchmarks and is tagged as such. Section 9 has the exact commands to finish the measurements after the owner runs `claude auth login` once.
2. **The machine was saturated.** Other research agents were compiling Rust and running Node processes at the same time; CPU load sat between 56 and 100 percent and free RAM near 1 GB for most runs [ran]. Timings below give the best run and the range. Best runs approximate an idle machine; worst runs show what contention does.

Codex, Gemini CLI, Copilot CLI, OpenCode and Qwen Code were installed into a scratch npm prefix (not globally) and started with `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA` and `CODEX_HOME` pointed at a throwaway directory, so nothing touched the owner's real config [ran]. None of them was logged in. Grok Build, Cursor CLI, Kimi CLI and Antigravity CLI were not installed; their rows are from docs and the ACP registry.

## 1. Verdict

1. **Every vendor that matters now has an official, scriptable harness, and most of them speak ACP.** The [ACP registry](https://github.com/agentclientprotocol/registry) on 2026-10-11 lists vendor-authored entries for Claude Agent, Codex, Gemini CLI, Google Antigravity, Grok Build, GitHub Copilot, Cursor, OpenCode, Qwen Code and Kimi CLI [primary]. D3 (ACP as the harness contract) holds.
2. **But the two harnesses Eileen will use most have richer native protocols, and their ACP adapters are Node wrappers around those protocols.** `claude-agent-acp` 0.89.1 is Node plus the TypeScript Agent SDK plus its own bundled `claude` binary; `codex-acp` 2.2.2 is Node that starts `codex app-server` [ran]. From a Rust core, driving `claude -p --input-format stream-json` and `codex app-server` directly removes a Node process (99 MB idle) and roughly a second of startup per session, and exposes features ACP does not carry (Codex `turn/steer` and `account/rateLimits/read`, Claude's `--permission-prompt-tool`). Recommendation: keep ACP as Eileen's internal session model and the default driver, and add two native drivers for Claude and Codex.
3. **Subscription policy, as written on 2026-10-11:**
   - **Anthropic** permits an end user to sign in to the unmodified Claude Code binary with their own subscription, including when another product runs it, and says Agent SDK, `claude -p` and third-party app usage draws from subscription limits. It also says developers building products "should use API key authentication" and may not offer claude.ai login or route requests through a plan on behalf of users. Eileen fits the end-user carve-out because the owner signs in himself and Eileen only spawns the binary. Keep the kill switch from D4 [primary].
   - **OpenAI** says local or open-source apps built on Codex app-server authentication may keep using it, that it was never permitted for commercial or hosted services, and recommends migrating to the new Sign in with ChatGPT plan-usage permission [primary]. Eileen is local and open source, so Codex is the most explicitly permitted route.
   - **Google** is the problem. Gemini CLI stopped serving Google AI Pro, Ultra and free individual accounts on 2026-06-18; the successor Antigravity CLI is closed source, and its FAQ says third-party software accessing Antigravity violates the terms and that headless mode inside third-party coding agents is not supported on consumer plans [primary]. There is no clean way to use a Google AI subscription from Eileen today.
   - **xAI** ships Grok Build as an official, Apache-2.0 CLI with ACP support pitched for "agent orchestration apps", usable with SuperGrok and X Premium+ [primary]. Grok is a clean route.
   - **DeepSeek** is a plain API key. Its Anthropic-compatible endpoint lets the unmodified Claude Code binary run on DeepSeek models [primary].
4. **MCP image results work in the three harnesses that matter most.** Claude Code shows PNG, JPEG, GIF and WebP tool results to the model; Codex has passed MCP images to the model since PR #5600 (merged 2025-10-27); Gemini CLI documents rich image content in tool results [primary]. A Rust MCP server built on `rmcp` 3.5.1 returned a PNG to both Claude Code and Codex app-server handshakes in this pass [ran]. This is how Eileen gives any harness full computer use without an API key.
5. **The brain should be a warm, Rust-driven `claude -p` stream-json session on Haiku with thinking off, behind a backend trait that can swap in Codex app-server or the DeepSeek API.** Harness overhead per warm turn was 160 to 415 ms up to the API call [ran]; Haiku 4.5 without thinking has a published 0.48 s time to first token on a 10k-token input [secondary]. The new Haiku 5.5 runs adaptive thinking by default and published 9.5 s to the first answer token at low effort [secondary], so thinking must be turned off for voice, and that configuration still needs measuring on this machine.
6. **Rust earns its place here, but not on model latency.** The Rust MCP server is ready 40 to 77 ms after spawn and idles at 4.9 MB; a Node ACP adapter idles at 99 MB and a Python process with the Agent SDK at 57 MB [ran]. The vendor harnesses themselves are fixed costs (`claude.exe` 245 to 256 MB on disk and 128 MB idle; `codex.exe` 318 MB on disk and 38 MB idle) [ran]. Model time to first token dominates every voice turn regardless of language [inferred].

## 2. Versions on 2026-10-11

All from the npm registry, PyPI, crates.io or the ACP registry on 2026-10-11 [primary], plus what was installed [ran].

| Component | Version | Notes |
|---|---|---|
| Claude Code (npm `@anthropic-ai/claude-code`) | 2.1.296 latest; 2.1.175 installed globally; 2.1.294 bundled in the Python SDK | The `haiku` alias resolves to `claude-haiku-4-5-20251001` on 2.1.175 and to `claude-haiku-5-5` on 2.1.294 [ran] |
| Claude Agent SDK, Python (`claude-agent-sdk`) | 0.2.165 | Wheel bundles a 256 MB `claude.exe` [ran] |
| Claude Agent SDK, TypeScript | 0.3.296 | |
| `@agentclientprotocol/claude-agent-acp` | 0.89.1 | Apache-2.0 in its `package.json`; the ACP registry lists it as "proprietary". Depends on the TS Agent SDK 0.3.293 and its own bundled Claude binary [ran] |
| `@openai/codex` | 0.162.1 | `codex.exe` 318 MB [ran] |
| `@agentclientprotocol/codex-acp` | 2.2.2 | Node; depends on `@openai/codex` and starts `codex app-server` [ran] |
| `@google/gemini-cli` | 0.63.0 | ACP flag is now `--acp` (registry) |
| Antigravity ACP server (`agy_acp_server`) | 1.3.0 | Google-published binary in the ACP registry, Windows x64 build available |
| Grok Build (`@xai-official/grok`) | 1.0.50 on npm, 1.0.51 in the ACP registry | Apache-2.0, win32-x64 binary; ACP via `grok agent stdio` |
| `@github/copilot` | 1.0.95 | ACP via `copilot --acp --stdio` |
| Cursor agent CLI | 2026.10.01 (registry) | ACP via `cursor-agent acp`, Windows x64 build |
| `opencode-ai` | 1.19.0 | MIT; ACP via `opencode acp` |
| `@qwen-code/qwen-code` | 0.25.1 | ACP via `--acp` |
| Kimi CLI | 1.52.0 (registry) | MIT; ACP via `kimi acp`, Windows x64 build |
| ACP Rust crate `agent-client-protocol` | 3.3.0 (schema crate 1.11.0) | Stable wire protocol v1; features `process`, `stdio`, unstable `unstable_mcp_over_acp` |
| ACP Python SDK `agent-client-protocol` | 0.12.1 | |
| ACP TypeScript SDK `@agentclientprotocol/sdk` | 1.8.0 | |
| Rust MCP SDK `rmcp` | 3.5.1 | Targets MCP spec 2026-07-28; negotiated down to 2025-06-18 with our client [ran] |

## 3. Policy, vendor by vendor

### Anthropic (Claude Code, Pro and Max)

- The [legal and compliance page](https://code.claude.com/docs/en/legal-and-compliance) says OAuth sign-in is for subscribers' ordinary use of Claude Code and native Anthropic apps; developers building products, including on the Agent SDK, should use API keys; third parties may not offer claude.ai login, route requests through Free, Pro or Max credentials on behalf of their users, or collect, store or intermediate claude.ai credentials. It then says this does not prevent an end user from signing in to the unmodified Claude Code binary with their own subscription, including where a platform hosts Claude Code. The binary must not be modified, and products may not use the Claude Code name or logo in their own names [primary].
- The [Agent SDK support article](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan) (update dated 2026-10-07) says the Agent SDK, `claude -p` and third-party apps still count against subscription limits. The separate SDK credit announced on 2026-05-13 was paused on 2026-06-15 and never took effect. From 2026-10-07, Max 5x gets $100 and Max 20x $200 of monthly API credits that cover the Claude API, the Agent SDK and Managed Agents (Pro gets none) [primary, amounts from [secondary](https://www.developersdigest.tech/blog/claude-max-team-monthly-api-credits-2026)].
- The [Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview) branding rules allow "Claude Agent", and "Claude" inside a menu already labeled "Agents"; they forbid "Claude Code" or "Claude Code Agent" [primary]. The owner's naming ("Claude", "Codex", "Gemini", "Grok", "DeepSeek") is compliant if the UI groups workers under an "Agents" label. D4's "Claude Agent" wording can relax to that.
- **Reading for Eileen [inferred]:** the owner installs Claude Code, runs `claude auth login` himself, and Eileen spawns the unmodified binary in his environment. That is the carve-out case. Eileen never offers a login, never reads `~/.claude`, never proxies or pools. The developer-facing sentence ("should use API key authentication") is the soft spot; keep `workers.subscription_backed: false` working from day one.
- **New risk:** the [headless docs](https://code.claude.com/docs/en/headless) say `--bare` "will become the default for `-p` in a future release", and bare mode never reads OAuth credentials [primary]. If that ships without an opt-out, subscription-backed `claude -p` stops working. Eileen's driver must detect the auth failure and say so plainly.

### OpenAI (Codex CLI, ChatGPT plans)

- The [Codex App Server docs](https://learn.chatgpt.com/docs/app-server) say local or open-source applications built on app-server authentication may continue to use it, recommend migrating to Sign in with ChatGPT for usage control and visibility, and say app-server authentication was never permitted for commercial or hosted services. The app-server and its WebSocket transport are labeled experimental and not supported for production workloads. `clientInfo.name` feeds OpenAI's compliance logs [primary].
- **Sign in with ChatGPT plan usage** launched at DevDay on 2026-09-29: an app asks for a separate "Use your ChatGPT plan" permission, Plus and Pro users cap each app as a weekly percentage, and OpenCode, OpenClaw, Pi and T3 are listed as open-source integrations ([OpenAI](https://learn.chatgpt.com/docs/sign-in-with-chatgpt) [primary]; flow details from [a third-party walkthrough](https://flaviocopes.com/sign-in-with-chatgpt/) [secondary]). Using it means Eileen runs an OAuth flow and holds tokens, which D4 forbids today. Stay on app-server auth (the `codex` binary owns its login) and revisit with a new decision if OpenAI deprecates that path.
- [Codex auth docs](https://learn.chatgpt.com/docs/auth) recommend API keys for CI and other programmatic use, store cached logins in `~/.codex/auth.json` or the OS keyring, and support device-code login for headless machines [primary].

### Google (Gemini CLI, Antigravity CLI, Google AI plans)

- [Google Developers Blog, 2026-05-19](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/): on 2026-06-18 Gemini CLI and Code Assist IDE extensions stop serving Google AI Pro, Ultra and free individual accounts; enterprise licenses and paid API keys keep working; Antigravity CLI is the successor [primary]. [Discussion #28017](https://github.com/google-gemini/gemini-cli/discussions/28017) confirmed the cutoff on 2026-06-18 [primary]. The repository README still advertises the personal free tier, and Gemini CLI 0.63.0 still advertises an `oauth-personal` auth method over ACP [ran]; both are stale relative to Google's announcement.
- [Antigravity FAQ](https://antigravity.google/docs/faq): using third-party software to access Antigravity violates the terms; `agy -p` headless mode is meant for direct scripting and CI, and use inside third-party coding agents is not supported on consumer plans [primary]. Forum threads report Pro and Ultra suspensions after third-party use ([example](https://discuss.ai.google.dev/t/account-restricted-without-warning-google-ai-ultra-oauth-via-openclaw/122778)) [secondary].
- Google does publish an ACP server binary for Antigravity in the ACP registry (version 1.3.0, Windows x64 build), listed in Zed's registry [primary]. That is the only Google-made bridge to a non-Google client, and the FAQ language still covers Eileen [inferred].
- **Reading for Eileen [inferred]:** no Gemini subscription route by default. Offer "Gemini" only as an explicit, labeled opt-in through Google's own ACP server, with a warning that Google's terms may not allow it on consumer plans. A Gemini API key would be clean but the owner allows only DeepSeek keys.

### xAI (Grok Build, SuperGrok and X Premium+)

- [xAI's launch post](https://x.ai/news/grok-build-cli): Grok Build is a terminal coding agent in early beta for all SuperGrok and X Premium+ subscribers, with headless `-p`, MCP, and "full ACP support" for building bots and agent orchestration apps [primary]. The [repository](https://github.com/xai-org/grok-build) is Apache-2.0, written in Rust, with prebuilt Windows binaries (`irm https://x.ai/cli/install.ps1 | iex`); Windows source builds are best-effort [primary].
- **Reading for Eileen:** the cleanest grant of any vendor. Drive `grok agent stdio` over ACP. Not installed or measured in this pass.

### DeepSeek (API key)

- [Anthropic-compatible API](https://api-docs.deepseek.com/guides/anthropic_api): base URL `https://api.deepseek.com/anthropic`, `x-api-key` supported; model names starting `claude-opus` map to `deepseek-v4-pro`, `claude-sonnet` and `claude-haiku` to `deepseek-flash`; tools, streaming, thinking and image inputs (base64 JPEG, PNG, GIF, WebP) are supported; server-side `mcp_servers` is ignored, which does not matter because Claude Code runs MCP client-side [primary].
- [Pricing](https://api-docs.deepseek.com/quick_start/pricing), per million tokens: `deepseek-flash` (V4.1-Flash, 1M context) $0.30 input cache miss, $1.20 output at peak, half that off-peak, cache hits $0.006; `deepseek-v4-pro` $1.32 and $3.96 at peak. Off-peak is everything outside 01:00 to 04:00 and 06:00 to 10:00 UTC on weekdays [primary].
- **Privacy [inferred]:** DeepSeek processes requests in China. A computer-use worker sends screenshots of the owner's screen. Default DeepSeek to text-only work and ask before sending images.

### The other harnesses

| Harness | Subscription in headless mode | Written stance on third-party driving |
|---|---|---|
| GitHub Copilot CLI | GitHub login (`copilot-login` over ACP [ran]); BYOK through `COPILOT_PROVIDER_*` | GitHub documents the ACP server for custom clients and CI ([docs](https://docs.github.com/en/copilot/reference/acp-server), public preview) [primary]. One Copilot plan reaches Claude, GPT and Gemini models through premium requests [secondary]. |
| Cursor CLI | Cursor login; `CURSOR_API_KEY` for unattended runs | Cursor's docs present ACP as the way to build custom clients [secondary mirror of [cursor.com/docs/cli/acp](https://cursor.com/docs/cli/acp)]. |
| OpenCode | `opencode-login` over ACP [ran]; ChatGPT plan through Sign in with ChatGPT; many API providers including DeepSeek | Anthropic subscriptions are not allowed in third-party harnesses (see Anthropic above) [primary]. |
| Qwen Code | The Qwen OAuth free tier was to close on 2026-04-15 ([issue #3203](https://github.com/QwenLM/qwen-code/issues/3203)) [primary], reported closed [secondary]; ACP initialize on 0.25.1 advertises only an OpenAI-compatible API key method [ran] | Not a subscription route any more. |
| Kimi CLI | Kimi membership login by device code; membership API keys for third-party tools | Kimi's docs list third-party tools through a membership API key as a supported use, personal use only ([Kimi Code](https://www.kimi.com/code/docs/en/)) [primary]. |

## 4. Interfaces, MCP images, models and approvals

### Headless and programmatic interfaces

| Harness | Interfaces | Verified here |
|---|---|---|
| Claude Code | `claude -p` with `--input-format stream-json --output-format stream-json --verbose --include-partial-messages`; Agent SDK (Python, TypeScript) over the same stdio with an SDK control protocol; ACP through `claude-agent-acp` | All three driven from Rust or Python up to the API call [ran] |
| Codex | `codex app-server` (JSON-RPC 2.0 over JSONL on stdio; also `ws://` and `unix://`, experimental); `codex exec --json`; TypeScript `@openai/codex-sdk`; ACP through `codex-acp` | app-server driven from Rust, 105 client request methods enumerated from `codex app-server generate-json-schema` [ran] |
| Gemini CLI | `gemini -p`, `--acp` | ACP initialize [ran] |
| Antigravity CLI | `agy -p` with `--output-format stream-json` and `--input-format stream-json`; Google's `agy_acp_server` | docs only |
| Grok Build | `grok -p`, `grok agent stdio` (ACP) | docs only |
| Copilot CLI | `copilot -p --output-format`, `--acp --stdio` or `--acp --port` | ACP initialize and `--help` [ran] |
| Cursor CLI | `cursor-agent -p --output-format stream-json`, `cursor-agent acp` | docs only |
| OpenCode | `opencode run`, `opencode acp`, HTTP server | ACP initialize [ran] |
| Qwen Code | `qwen -p`, `--acp` | ACP initialize [ran] |
| Kimi CLI | `kimi acp` | docs only |

ACP capabilities each agent reported on `initialize` [ran]:

| Agent | Auth methods offered | Prompt images | MCP transports besides stdio | Session extras |
|---|---|---|---|---|
| claude-agent-acp 0.89.1 | none (user must run `claude auth login`) | yes | http, sse | list, resume, fork, close, delete, subagents |
| codex-acp 2.2.2 | `api-key`, `chat-gpt` | yes | http (`acp: false`) | resume, list, fork, close, delete, subagents |
| Gemini CLI 0.63.0 | `oauth-personal`, `gemini-api-key`, `vertex-ai`, `gateway` | yes, plus audio | http, sse | load |
| Copilot CLI 1.0.95 | `copilot-login` | yes | http, sse | list, close |
| OpenCode 1.19.0 | `opencode-login` | yes | http, sse | list, resume, fork, close |
| Qwen Code 0.25.1 | `openai` (API key) | yes, plus audio | http, sse | list, resume |

The ACP schema requires every agent to accept stdio MCP servers in `session/new` ("All Agents MUST support this transport", `agent-client-protocol-schema` 1.11.0) [primary]. That is the uniform way to hand Eileen's tools to any harness.

### MCP tool results that carry images

| Harness | Model sees MCP image results? | Source |
|---|---|---|
| Claude Code | Yes. PNG, JPEG, GIF, WebP shown inline, possibly downscaled; originals saved under the session's `tool-results` folder; still subject to `MAX_MCP_OUTPUT_TOKENS` (default 25,000) | [MCP docs](https://code.claude.com/docs/en/mcp) [primary] |
| Codex | Yes since PR #5600 (merged 2025-10-27); replaced by a text placeholder for text-only models since PR #11346 (2026-02-10) | [openai/codex#5600](https://github.com/openai/codex/pull/5600), [#11346](https://github.com/openai/codex/pull/11346) [primary] |
| Gemini CLI | Yes, "rich content" including images and audio | [docs/tools/mcp-server.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md) [primary] |
| Copilot, Grok Build, Cursor, OpenCode, Kimi, Antigravity | Not verified | |

One Claude Code bug to design around: image blocks were dropped when a tool result also carried `structuredContent` ([reported, closed 2026-05-02](https://claudeissues.com/issue/54737-bug-mcp-tool-results-image-content-blocks-dropped-when-structuredcontent-is-also)) [secondary]. Eileen's screenshot tools should return text plus image content and no `structuredContent`. The Rust probe server does exactly that.

What was run: a Rust MCP server (`probe-mcp`, rmcp 3.5.1) with `get_time` (text) and `screenshot` (a synthetic 256x128 PNG, red left half, blue right half, green square, so no real screen content left the machine). Called directly over stdio it answered `tools/call screenshot` with a text block and an `image/png` block in 0 to 1 ms [ran]. Claude Code 2.1.294 connected to it through `--mcp-config` and listed both tools (`mcp_servers: [{"name":"probe","status":"connected"}]`, `tools=2`) [ran]. Codex app-server connected to it through `-c mcp_servers.probe.command=...` and listed both tools in `mcpServerStatus/list` [ran]. The model-side check (asking Haiku to describe the image) is in section 9 for after login.

### Models and what each is best at

| Route | Models on 2026-10-11 | Best at | Limits and cost |
|---|---|---|---|
| Claude | Fable 5.1 (most capable), Opus 5.5, Sonnet 5.5, Haiku 5.5 (released 2026-10-07), Haiku 4.5 (deprecated). API prices per million tokens: Fable $10/$50, Opus 5.5 $4/$20, Sonnet 5.5 $2/$10, Haiku 5.5 $0.10/$0.50 [primary, Anthropic model table cached 2026-10-06] | Long-horizon coding and agentic tool use; computer use through the computer toolset | Pro $20, Max 5x $100, Max 20x $200 a month; five-hour and weekly windows; Max now adds $100 or $200 of API credits |
| Codex | `gpt-6.1-sol` (default, "workhorse for coding"), `gpt-6-astra` (frontier), `gpt-6-luna` (fast, affordable), `gpt-6-sol`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5`; efforts `low` through `max`, plus `ultra` on Sol and Astra [ran, from `model/list`] | Coding in a sandbox, review, cloud tasks that survive the PC being off | Plus and Pro plans; quota is readable at runtime through `account/rateLimits/read` [ran, method list] |
| Gemini (Antigravity) | Gemini 3.8, 3.7, 3.6 Flash; Gemini 3.1 Pro; on Pro and Ultra also Claude Sonnet 5.5 and Opus 5.5 [primary, [models page](https://antigravity.google/docs/models/)] | Long-context multimodal work, fast Flash | Separate five-hour and weekly meters for Gemini and for Claude/GPT models |
| Grok | Grok Build's coding model; check `grok models` after install [secondary] | Coding with parallel subagents | SuperGrok, X Premium+ |
| DeepSeek | `deepseek-flash` (V4.1-Flash), `deepseek-v4-pro`, 1M context [primary] | Cheap volume, fast non-reasoning replies | Pay per token; concurrency 2,500 (flash) and 500 (pro) |

### Permission and approval hooks

| Harness | How approvals reach Eileen |
|---|---|
| Claude Code `-p` | `--permission-prompt-tool mcp__eileen__approve` sends every prompt to a tool on Eileen's own MCP server (documented); `--permission-prompts none` denies anything nobody can answer; `--allowedTools` rules; `PermissionRequest` and `PreToolUse` hooks; the SDK control protocol's `can_use_tool` request [primary, [CLI reference](https://code.claude.com/docs/en/cli-reference)] |
| Codex app-server | Server requests `item/commandExecution/requestApproval` and `item/fileChange/requestApproval` (decisions `accept`, `acceptForSession`, `decline`, `cancel`), `item/permissions/requestApproval`; per-thread `approvalPolicy` [primary, confirmed in the generated schema [ran]] |
| Any ACP agent | `session/request_permission` with options of kind `allow_once`, `allow_always`, `reject_once`, `reject_always` [primary] |
| Antigravity `agy -p` | No interactive prompt; unlisted tools are soft-denied unless `permissions.allow` covers them [primary] |
| Cursor `-p` | File changes apply only with `--force` [secondary] |
| Copilot | `--allow-tool`, `--allow-all-tools`; ACP `requestPermission` [ran, `--help`] |

These map cleanly onto D14's tiers: reversible tools go in allow lists, consequential ones route to the approval tool or ACP request, and the voice loop turns each into an ApprovalRequested event [inferred].

## 5. Driving the harnesses from Rust

Four small binaries were written in a scratch crate (`edition = 2024`, release profile with thin LTO). All four pass `cargo fmt --check` and `cargo clippy --all-targets -- -W clippy::pedantic -D warnings` [ran]. The two lint suppressions are `expect`s with written reasons, both caused by `rmcp` macros (`#[tool]` dispatches tools as methods; `#[tool_handler]` emits async trait methods with no `.await`). Release binaries: `probe-mcp` 2.5 MB, `probe-acp` 3.0 MB, `probe-stream` 0.68 MB, `probe-jsonrpc` 0.68 MB [ran]. The first release build took 13 min 16 s while the machine was saturated; incremental rebuilds took 1 to 2.5 min [ran].

Dependencies: `tokio` 1 (full), `serde_json` 1, `anyhow` 1, `rmcp` 3.5.1 (`server`, `transport-io`, `macros`, `schemars`), `agent-client-protocol` 3.3.0 (`process`, `stdio`), `png` 0.17, `base64` 0.22.

### Option A: the official ACP crate as a client

`agent_client_protocol::Client.builder()` with `on_receive_notification` for `SessionNotification`, `on_receive_request` for `RequestPermissionRequest`, then `connect_with(AcpAgent::new(config), ...)` and `send_request(InitializeRequest / NewSessionRequest / PromptRequest).block_task().await`. It worked on Windows the first time it compiled [ran]. Against `claude-agent-acp` with the Rust MCP server attached: initialize 0.75 s best (1.07 to 1.78 s typical, 9.5 to 106 s under heavy load); `session/new` 1.07 s best (1.65 to 8.9 s); a prompt reached the API and came back with `-32000 Authentication required` 577 to 591 ms later [ran]. The same flow through the Python ACP SDK took 3.1 to 5.7 s to initialize and 3.3 to 14.5 s for `session/new`, all under load [ran].

Two Windows details the crate leaves to the caller [primary, read from `acp_agent.rs`]: it puts the child in a process group only on Unix, so on Windows killing an `npx` or `node` wrapper can orphan the real agent; and Rust's `Command` does not run npm `.cmd` shims. Eileen should resolve the real executable (or `node` plus the package's entry script) and put every harness in a Windows Job Object with kill-on-close.

### Option B: `claude -p` stream-json, driven directly

About 200 lines of Rust. Spawn `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --no-session-persistence --setting-sources "" --strict-mcp-config --mcp-config eileen.json --tools "" --model <id> --system-prompt <short prompt>`, then write one line per user turn:

```json
{"type":"user","message":{"role":"user","content":"..."},"parent_tool_use_id":null}
```

and read lines: `system/init` (model, tools, MCP status, a `capabilities` array for feature detection), `stream_event` (Anthropic streaming events; text arrives as `content_block_delta` with `text_delta`), `assistant`, `user` (tool results), and one `result` per turn with `is_error`, `duration_api_ms`, `usage` and `total_cost_usd` [ran, [headless docs](https://code.claude.com/docs/en/headless)].

Measured with the 2.1.294 binary, Haiku 5.5 and the Rust MCP server attached: spawn to `system/init` 1.17 to 1.55 s in the best four of ten runs, 2.1 to 20 s in the other six under load; the first run of a freshly downloaded 256 MB binary took 20 s, which looks like a one-time scan by Windows Defender [ran, inferred]. With `--mcp-config`, `-p` waits for MCP servers before the first turn (up to `MCP_TIMEOUT`, 30 s by default) [primary], so a slow MCP server delays the brain's first answer. The Rust server is ready in about 50 ms, so it costs nothing here [ran].

Interrupts and in-process tools travel over the same stdio as `control_request` and `control_response` lines (subtypes seen in the Python SDK source: `initialize`, `interrupt`, `set_permission_mode`, `set_model`, `can_use_tool`, `mcp_message`, `hook_callback`) [ran, read from `claude_agent_sdk/_internal/query.py`]. The official SDKs speak this protocol, but it is not documented as a public API. Use it for `interrupt` (Windows has no clean SIGINT for a child process), pin tested CLI versions, and check `system/init.capabilities` before relying on any of it [inferred].

### Option C: Codex app-server JSON-RPC

A generic JSONL JSON-RPC prober (about 100 lines) drove `codex app-server` with `initialize` (`clientInfo` plus `capabilities.experimentalApi`), the `initialized` notification, `account/read`, `mcpServerStatus/list` and `model/list`. Initialize answered in 0.23 to 0.33 s on warm runs (1.0 to 2.3 s cold or loaded), `account/read` in 13 to 32 ms (`requiresOpenaiAuth: true`), `mcpServerStatus/list` with the Rust server in 41 to 95 ms (412 ms on first connect), `model/list` in 3 to 10 ms [ran]. `codex app-server generate-json-schema` writes the full protocol as JSON Schema for the installed version [ran]; generate Rust types from it at build time and regenerate on every Codex upgrade.

Methods worth building on [ran, from the schema]: `thread/start` (with `baseInstructions`, `developerInstructions`, `model`, `approvalPolicy`, `ephemeral`, experimental `dynamicTools`), `turn/start`, `turn/steer` (inject a new user message into a running turn, which is what barge-in wants), `turn/interrupt`, `thread/inject_items`, `account/rateLimits/read`, `mcpServer/tool/call`, `windowsSandbox/readiness`. On a fresh `CODEX_HOME`, app-server started several `git` and `git-remote-https` child processes at launch (about 55 MB combined, transient) [ran]; the cause was not traced.

### Option D: Eileen's own MCP servers on `rmcp`

`#[tool_router]` on an impl, `#[tool]` per method, `#[tool_handler] impl ServerHandler`, then `Probe::new().serve(rmcp::transport::stdio()).await?.waiting().await`. Image results are `ContentBlock::image(base64, "image/png")` inside `CallToolResult::success` [ran]. Measured: spawn to initialize answered 40 to 77 ms total, initialize itself 19 to 31 ms, `tools/list` and `tools/call` 0 to 1 ms, idle working set 4.9 MB (2.0 MB private) [ran]. For comparison, a Node ACP adapter idles at 99 MB [ran]; a Node MCP server would be in the same range [inferred].

### Memory and startup, side by side

| Process (idle) | Working set | Private | Spawn to ready |
|---|---|---|---|
| Rust MCP server (`rmcp`) | 4.9 MB | 2.0 MB | 40 to 77 ms |
| `codex app-server` 0.162.1 | 38 MB | 16 MB | 0.23 to 0.33 s warm |
| Python 3.13 with `claude_agent_sdk` imported | 57 MB | 44 MB | `ClaudeSDKClient.connect()` 1.6 to 6.1 s |
| `claude-agent-acp` (Node) before `session/new` | 99 MB | 82 MB | initialize 0.75 s best |
| `claude -p` stream-json 2.1.294 | 128 MB | 255 MB | 1.17 to 1.55 s to `system/init` |

All [ran]. `claude --version` alone takes 0.26 to 0.30 s on 2.1.294 and 0.61 to 0.75 s on 2.1.175; a one-shot `claude -p` that fails at auth takes 3.2 to 6.0 s [ran], which is the cold-start cost a warm session avoids on every turn.

ACP `initialize` for the other agents, all under heavy load and not logged in [ran]: `codex-acp` 2.1 to 38 s (4.4 s typical), OpenCode 13.6 to 32 s, Gemini CLI 34 to 49 s, Copilot CLI 38 to 55 s, Qwen Code 106 s. These are too noisy to rank. They do say that cold-starting a harness per voice request is out of the question; sessions must be warm [inferred].

## 6. The brain

### Budget

Voice to voice under 2 s needs, roughly: end of turn detection 250 to 400 ms (VAD hangover plus Smart Turn), final STT 100 to 200 ms with streaming partials, brain time to first token 600 to 900 ms, first TTS audio 150 to 250 ms, playback 50 ms [inferred from RESEARCH.md section 4]. The brain's time to first token is the only large, uncertain term.

### Candidates

| Candidate | Harness overhead per warm turn | Model time to first token | Notes |
|---|---|---|---|
| Warm `claude -p` stream-json, driven from Rust, Haiku | Not separable from the SDK row; same binary and protocol | Haiku 4.5 without thinking 0.48 s on a 10k-token input ([Artificial Analysis](https://artificialanalysis.ai/models/claude-4-5-haiku/providers)) [secondary]. Haiku 5.5 thinks by default: 9.5 s to the first answer token at low effort ([AA](https://artificialanalysis.ai/models/claude-haiku-5-5-low/providers)) [secondary]. Thinking off is required and unmeasured. | No Node or Python. Custom `--system-prompt`, `--tools ""` and only Eileen's MCP tools keep the prompt small. 128 MB resident. |
| Agent SDK `ClaudeSDKClient` (Python), Haiku, in-process SDK MCP tools | 158 to 415 ms from `query()` to `system/init`; 205 to 912 ms to the (auth-failed) result, which includes a failed OAuth refresh round trip [ran] | Same model as above | Same CLI underneath. Adds a Python runtime (57 MB) to a Rust product for no latency gain. The in-process tools are just MCP over the control protocol. |
| Codex app-server, `gpt-6-luna` at low effort | Not measured (no login) | Not measured; published figures are at max effort and irrelevant here | `turn/steer` fits barge-in; `baseInstructions` replaces the coding prompt; `dynamicTools` lets Eileen expose tools without an MCP process. Most explicitly permitted route. |
| Gemini Flash through Antigravity | n/a | n/a | Excluded: consumer-plan headless use inside third-party agents is not supported (FAQ). |
| DeepSeek API, `deepseek-flash` non-reasoning, called from Rust | None (one HTTPS request) | Not measured (no key). Third-party providers of the same model show 0.84 to 0.95 s on a 10k-token input ([AA](https://artificialanalysis.ai/models/deepseek-v4-1-flash-non-reasoning/providers)) [secondary] | Cheapest and simplest. Data goes to DeepSeek. |
| Claude Sonnet 5.5 as the brain | Same as Haiku | Slower and pricier than Haiku [inferred] | Use as the escalation model for hard spoken questions, not the default. |

### Recommendation

1. **Default brain: a warm `claude -p` stream-json process owned by a Rust `BrainBackend`.** Start it at boot and keep a second one warm as a hot spare, so a crash or a policy-driven restart never costs the owner a 1.5 s pause. Flags: `--model claude-haiku-4-5` until Haiku 5.5 with thinking off is measured, then switch if it is at least as fast; `--effort low`; thinking off through `--settings` (test `alwaysThinkingEnabled: false` first); `--system-prompt` with Eileen's short persona plus the per-turn context blocks from ARCHITECTURE.md section 4; `--tools ""`; `--strict-mcp-config --mcp-config` pointing at Eileen's Rust MCP server only; `--setting-sources ""`; `--no-session-persistence`; `--permission-prompt-tool mcp__eileen__approve`. Do not use `--bare` (it disables subscription login) or `--safe-mode` (in our runs the `--mcp-config` server never started under it [ran]).
2. **Second backend: Codex app-server with `gpt-6-luna`,** selectable by policy or voice ("use Codex for chat"). Build it right after the Claude backend because it carries the cleanest written permission and `turn/steer`.
3. **Fallback backend: DeepSeek `deepseek-flash` over HTTPS from Rust,** used when the subscription backends are rate-limited or signed out, and for anything the owner marks as fine to leave the machine. Text only by default.
4. **The brain never does long work.** It answers, grounds pronouns against the jobs block, and calls `dispatch_job` on Eileen's MCP server. Coding and computer-use jobs go to worker sessions (section 7). This keeps every brain turn short and cheap against subscription limits.
5. **Speculate on partial transcripts.** Start the brain turn on a stable STT partial and cancel (stream-json `interrupt`, Codex `turn/interrupt`) if the user keeps talking [inferred]. With per-turn harness overhead in the low hundreds of milliseconds this can hide most of the endpointing delay.

The Python Agent SDK is not needed anywhere in the brain. Its only unique feature for Eileen, in-process tools with callbacks, is matched by a Rust MCP server plus `--permission-prompt-tool`.

## 7. The harness-driving layer

Keep ACP's model (sessions, prompts, streamed updates, permission requests, cancel) as Eileen's internal `Harness` trait, with three implementations:

| Route (worker name) | Driver | Why |
|---|---|---|
| Claude | Native `claude -p` stream-json | Uses the binary the owner installed and signed in to (the ACP adapter and the Python SDK each bundle their own copy, 2.1.293 and 2.1.294, while the owner's is 2.1.175 [ran]). No Node hop. Direct access to `--permission-prompt-tool`, partial messages, hook events, subagent forwarding. |
| Codex | Native `codex app-server` | `codex-acp` is a Node wrapper around the same server [ran]. Native gives `turn/steer`, `account/rateLimits/read`, `dynamicTools`, schema-generated types. |
| Grok | ACP to `grok agent stdio` | Official, Apache-2.0, invited for orchestration. |
| DeepSeek | Claude Code harness with `ANTHROPIC_BASE_URL=https://api.deepseek.com/anthropic` and the DeepSeek key in `ANTHROPIC_AUTH_TOKEN`, scoped to that child only and with its own `CLAUDE_CONFIG_DIR`; or OpenCode over ACP | One driver for two routes. A separate config dir keeps DeepSeek sessions apart from the subscription login. OpenCode is the alternative if Claude Code on a non-Anthropic endpoint misbehaves. |
| Gemini | ACP to Google's `agy_acp_server`, off by default | See Google policy above. |
| Copilot, Cursor, OpenCode, Kimi | ACP | Later milestones; all already speak it. |

Keep `claude-agent-acp` and `codex-acp` registered as alternate drivers. The generic ACP client exists anyway, so they cost nothing and give a behavioral cross-check when a native driver misbehaves.

MCP over ACP (`unstable_mcp_over_acp` in the Rust crate) would let Eileen serve its tools through the ACP connection itself instead of a separate stdio process. `codex-acp` already advertises `mcpCapabilities.acp: false` [ran], so adapters know about it but do not implement it. Use stdio MCP until it stabilizes.

Parallel agents ("launch parallel agents for X"): one harness session per job, each in its own worktree and Job Object, as D3 and ROADMAP M4 already say. Claude Code now has its own background sessions and `claude agents` view [primary, CLI reference], but those are a separate supervisor; Eileen should own its sessions so it can name, announce and cancel them.

## 8. What this changes in the decision log

Proposals for new entries in [DECISIONS.md](../DECISIONS.md); do not edit the old ones.

1. **D3 amendment:** ACP is the internal session model and the default driver; Claude and Codex use native drivers (`claude -p` stream-json, `codex app-server`) because their ACP adapters are Node wrappers over those same protocols.
2. **D4 amendment:** worker display names are route names ("Claude", "Codex", "Grok", "DeepSeek", "Gemini") shown under an "Agents" heading, which Anthropic's branding guidance allows; never "Claude Code" anything. DeepSeek is the one permitted API key. Google AI subscriptions are off by default because Google's own FAQ excludes third-party agents on consumer plans.
3. **D9 superseded by Rust:** the always-on core, the harness drivers, the brain backends and Eileen's MCP servers are Rust. Nothing in this pass needed Python; the Python Agent SDK is not used.
4. **New:** the brain is a warm Rust-driven harness session with a hot spare, behind a `BrainBackend` trait with Claude, Codex and DeepSeek implementations; the default model is chosen by a measured bake-off on the owner's PC.

## 9. Finishing the measurements after login

The owner runs these himself; Eileen and its contributors never run a login flow or read credential files.

```powershell
claude auth login          # owner, once; then confirm:
claude auth status         # exit code 0 means signed in
```

The probe sources live in the scratch folder for this research session (`...\scratchpad\harnesses\rust-probe`, `sdk_probe.py`, `acp_probe.py`, `measure-idle.ps1`, `rerun.sh`); scratch folders are temporary, so the essentials are reproduced above. To finish the brain numbers (about ten short prompts, almost all Haiku):

1. `probe-stream <claude.exe> claude-haiku-4-5 mcp-config.json "Reply with exactly: ready" "What time is it? Use the tool." "Call the screenshot tool and tell me the colors and which side each is on."` Records time to first event, first text, tool-use timing and the result per turn, plus whether the model correctly reads red on the left, blue on the right and a green square from an MCP image.
2. The same with `claude-haiku-5-5` plus `--effort low` and thinking off, then `claude-sonnet-5-5` for two turns.
3. `python -I sdk_probe.py haiku "Reply with exactly: ready" "What time is it?"` for the Agent SDK comparison.
4. `probe-acp --mcp probe-mcp.exe --prompt "Create a file named hello.txt containing hi" -- node <claude-agent-acp>\dist\index.js` for the ACP permission round trip (the client answers in microseconds; the number that matters is request to resumed output).
5. After `codex login` (owner), the same three turns through app-server with `gpt-6-luna` at low effort.

Run each three times on an idle machine and report the best and the median.

## 10. Pitfalls

1. **Inherited environment.** A shell started from a Claude Code session carries `CLAUDECODE`, `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_SDK_HAS_HOST_AUTH_REFRESH`, `ANTHROPIC_BASE_URL` and about thirty more [ran, names only]. Scrub `CLAUDE*`, `ANTHROPIC*` and `MCP_*` before spawning any harness, then add back only what that worker needs.
2. **Auth failures are results, not exit codes alone.** `claude -p` reports a missing login as a `result` with `is_error: true`, and the text changes by version ("Not logged in · Please run /login" on 2.1.175, "Failed to authenticate: OAuth session expired and could not be refreshed" on 2.1.294) [ran]. `eileen doctor` should use `claude auth status` (exit 0 or 1) and Codex `account/read`.
3. **Model aliases drift.** `haiku` meant Haiku 4.5 on one installed build and Haiku 5.5 on another [ran]. Pin full model IDs in policy.
4. **Several copies of Claude Code.** The Python SDK wheel and the ACP adapter each bundle their own binary, so three versions can sit on one machine [ran]. The native driver should call the owner's installed `claude`, found on `PATH` and resolved to the real `.exe`.
5. **`--bare` and `--safe-mode` are traps for subscription sessions.** Bare never reads OAuth; safe mode appeared to suppress `--mcp-config` servers [primary, ran].
6. **Windows process trees.** Use Job Objects with kill-on-close for every harness; the ACP crate only cleans up process groups on Unix [primary].
7. **`.cmd` shims.** Rust's `Command` will not run npm `.cmd` shims; resolve the vendor `.exe` (Claude, Codex, Copilot, OpenCode ship native binaries) or `node` plus the entry script [ran].
8. **MCP startup gates the first turn.** `-p` waits up to `MCP_TIMEOUT` for `--mcp-config` servers [primary]. Keep Eileen's servers fast and never attach a slow third-party server to the brain.
9. **Image tool results.** No `structuredContent` alongside images in Claude Code; downscale screenshots before returning them (Claude Code may compress them anyway, and the 25,000-token output cap still applies) [primary, secondary].
10. **Codex app-server is labeled experimental** and not supported for production workloads [primary]. Pin the version and regenerate types on upgrade.
11. **Haiku 5.5 thinks by default.** Leaving the default on would put roughly ten seconds before the first spoken word [secondary].
12. **DeepSeek and screenshots.** Do not send screen images to DeepSeek without asking.
13. **Measurement noise.** Every timing here was taken while other agents loaded the machine. Re-run on an idle machine before using any number as a budget.

## 11. Open risks

1. Anthropic's stated plan to make `--bare` the default for `-p` would cut off subscription-backed headless use unless an opt-out ships [primary].
2. Anthropic's stance changed five times in the first half of 2026; the developer clause ("should use API key authentication") sits next to the end-user carve-out Eileen relies on [primary, inferred].
3. Google's consumer-plan terms exclude third-party agents; the owner's Google AI subscription may simply not be usable [primary].
4. OpenAI recommends Sign in with ChatGPT over app-server auth; if it deprecates the latter, Eileen needs an OAuth decision that contradicts today's D4 [primary, inferred].
5. Haiku 5.5 latency with thinking off through Claude Code is unmeasured; if it is slow, the brain stays on Haiku 4.5 until Anthropic retires it, then moves to Codex or DeepSeek [inferred].
6. The Claude SDK control protocol (`interrupt`, `can_use_tool`) is undocumented and can change without notice [inferred].
7. Gemini CLI, Copilot CLI and Qwen Code took 34 to 106 s to answer ACP `initialize` under load [ran]. If that holds on an idle machine it is a startup problem in those tools, and their sessions must be started long before they are needed.

## Sources

Anthropic: [legal and compliance](https://code.claude.com/docs/en/legal-and-compliance); [Agent SDK with your Claude plan](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan); [API credits for Max and Team](https://platform.claude.com/docs/en/about-claude/api-credits-for-subscribers); [Agent SDK overview and branding](https://code.claude.com/docs/en/agent-sdk/overview); [headless mode](https://code.claude.com/docs/en/headless); [CLI reference](https://code.claude.com/docs/en/cli-reference); [MCP](https://code.claude.com/docs/en/mcp); [Zed on the paused billing change](https://zed.dev/blog/anthropic-subscription-changes).

OpenAI: [Codex App Server](https://learn.chatgpt.com/docs/app-server); [Codex authentication](https://learn.chatgpt.com/docs/auth); [Sign in with ChatGPT](https://learn.chatgpt.com/docs/sign-in-with-chatgpt); [openai/codex#5600](https://github.com/openai/codex/pull/5600); [openai/codex#11346](https://github.com/openai/codex/pull/11346); [Sign in with ChatGPT walkthrough](https://flaviocopes.com/sign-in-with-chatgpt/) (secondary).

Google: [Transitioning Gemini CLI to Antigravity CLI](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/); [gemini-cli discussion #28017](https://github.com/google-gemini/gemini-cli/discussions/28017); [gemini-cli discussion #22970](https://github.com/google-gemini/gemini-cli/discussions/22970); [Gemini CLI MCP docs](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md); [Antigravity FAQ](https://antigravity.google/docs/faq); [Antigravity headless mode](https://antigravity.google/docs/cli/headless/); [Antigravity models](https://antigravity.google/docs/models/); [Zed ACP listing for Antigravity](https://zed.dev/acp/agent/antigravity-acp).

xAI: [Introducing Grok Build](https://x.ai/news/grok-build-cli); [xai-org/grok-build](https://github.com/xai-org/grok-build).

DeepSeek: [Anthropic API guide](https://api-docs.deepseek.com/guides/anthropic_api); [pricing](https://api-docs.deepseek.com/quick_start/pricing).

Others: [ACP registry](https://github.com/agentclientprotocol/registry) and its [authentication requirements](https://github.com/agentclientprotocol/registry/blob/main/AUTHENTICATION.md); [agent-client-protocol crate](https://docs.rs/agent-client-protocol/3.3.0); [rmcp](https://crates.io/crates/rmcp/3.5.1); [GitHub Copilot ACP server](https://docs.github.com/en/copilot/reference/acp-server); [Qwen Code issue #3203](https://github.com/QwenLM/qwen-code/issues/3203); [Kimi Code docs](https://www.kimi.com/code/docs/en/); [Artificial Analysis: Haiku 4.5](https://artificialanalysis.ai/models/claude-4-5-haiku/providers), [Haiku 5.5 low](https://artificialanalysis.ai/models/claude-haiku-5-5-low/providers), [DeepSeek V4.1 Flash](https://artificialanalysis.ai/models/deepseek-v4-1-flash-non-reasoning/providers) (secondary).
