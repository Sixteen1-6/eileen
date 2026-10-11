# Contributing to Eileen

Eileen is a voice-first AI operating layer for Windows 11, MIT licensed. This file covers how to pick work, how to send a change, and the rules that do not bend.

## Who this is for

Friends of the owner first. The repo is private while the v0.1 nucleus takes shape, and the people invited are the first contributors. Once v0.1 works end to end, the repo goes public and the same rules apply to anyone.

New here? Read [docs/DEV-SETUP.md](docs/DEV-SETUP.md) to get a working checkout, then [docs/RESEARCH.md](docs/RESEARCH.md) for why the stack looks the way it does.

## Picking work

Work is tracked as GitHub issues. Every issue carries two labels.

| Label group | Values | Meaning |
|---|---|---|
| `milestone` | `m1` to `m6` | Position in the v0.1 build order: desktop state, action layer, voice, harnesses, phone, later |
| `area` | `core`, `voice`, `desktop`, `browser`, `apps`, `harness`, `channels`, `docs` | Which subpackage or folder the work lands in |

To claim an issue, comment on it. If nobody has claimed it, it is yours. If you stop, say so in a comment so someone else can pick it up.

One issue per pull request. If a change needs a second issue, open it and link it.

## Branches and commits

Branch names follow `<type>/<milestone>-<short-topic>`: `feature/m1-desktop-state`, `fix/m3-echo-cancellation`, `docs/dev-setup`.

Commit messages are imperative and fit on one line: `Add monitor ordinals to DesktopState`. A body is optional. Use it when the why is not obvious from the diff.

## Pull request checklist

Before you ask for review:

- `pytest` passes.
- `ruff check .` and `ruff format --check .` are clean.
- Docs are updated if behavior or setup changed.
- No credentials, tokens, API keys or personal paths (`C:\Users\<you>`) in the diff.
- No em dashes in any Markdown you touched.
- The PR description names the issue it closes.

## Review

One approval from another contributor is required. The owner (Sixteen1-6) merges. Reviews look at correctness, at whether the change follows the design principles below, and at whether a newcomer could read it. Style nits go to ruff, not to reviewers.

## Design principles

These come from the research brief and settle most design arguments.

1. The user is the supreme router. A spoken override beats any policy.
2. Conversation never blocks on work. The voice loop owns the microphone; agent sessions are async jobs.
3. Deterministic tools before vision: App Actions, COM, UI Automation, browser DOM, MCP servers. Screenshots and mouse last.
4. Reversible actions run immediately. Consequential ones (send, delete, spend) confirm first.
5. Wrap existing agent harnesses. Never rebuild them, never touch their credentials.
6. Compose, do not fork. Secrets never enter model-visible text. Encrypt at rest. Sandbox by default.

## Code conventions

| Topic | Rule |
|---|---|
| Language | Python 3.12 or newer |
| Layout | `src/` layout, package name `eileen` |
| Cross-process messages | A pydantic model for every message that crosses a process, socket or pipe boundary |
| Types | Type hints on every function signature |
| Concurrency | `asyncio`. No blocking calls in the event loop; wrap them in `asyncio.to_thread` |
| Lint and format | `ruff` for both. The config in `pyproject.toml` is the only source of truth |
| Tests | `pytest`. Unit tests must run without a microphone, a display or a signed-in harness |
| Sidecar | Rust or C#. The Python core only talks to it over MCP |

## Repo map

| Path | What lives there |
|---|---|
| `src/eileen/` | The Python package |
| `docs/` | Design docs, research, setup, naming |
| `tests/` | pytest suite, mirrors the package layout |
| `.github/` | CI workflows, issue and PR templates |

Inside `src/eileen/`:

| Subpackage | Responsibility |
|---|---|
| `core` | Coordinator, job registry, event bus, router, YAML policy, permission tiers |
| `voice` | Audio capture and playback, VAD, turn detection, STT, TTS, the conversational loop, announcement policy |
| `desktop` | DesktopState client, Cua Driver adapter, window actions |
| `browser` | Playwright MCP client and selection helpers |
| `apps` | COM adapters (Office, Explorer), SMTC media control, later Resolve and Vegas |
| `harness` | ACP client and the Claude Agent and Codex adapters |
| `channels` | Telegram bot, ntfy push, presence detection |

## Writing style for docs

Plain, editorial, confident. Short paragraphs. Sentences under about 25 words. Use a table when it beats prose. Add a header only when it helps someone find their way.

No em dashes. Use commas, periods or parentheses instead. No emoji. Skip words that sound like marketing copy: "seamless", "unlock", "empower", "robust", "leverage", "cutting-edge", "journey", "game-changer", "delve", and anything that begins "in today's fast-paced world".

GitHub-flavored Markdown. Relative links between docs must resolve. Summarize the research brief and link to it; do not paste chunks of it.

## Hard rules

These are not open for review discussion. A PR that breaks one is closed.

1. Never commit or log credentials. That includes API keys, bot tokens, session cookies and anything from a `.env` file.
2. Never add code that reads vendor auth files (`~/.claude`, `~/.codex/auth.json` or their equivalents), proxies tokens, or runs an OAuth flow for the user. Users sign in to the official binaries themselves.
3. Never modify, patch or rebrand a vendor binary. Spawn the unmodified official one.
4. Workers are called "Claude Agent" or "Codex". Never "Claude Code <anything>".
5. Never bundle code under a license that conflicts with MIT. Talk to such tools over IPC or spawn them.

The reasoning behind rules 2 to 4 is in [docs/RESEARCH.md](docs/RESEARCH.md), section 3.
