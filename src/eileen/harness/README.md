# harness

The Agent Client Protocol client and one adapter per coding agent. A harness is an unmodified vendor binary that Eileen spawns in the user's own environment and drives as a job. Workers are called "Claude Agent" or "Codex", never `Claude Code <anything>`.

## Builds on

| Worker name | Adapter | Status |
|---|---|---|
| Claude Agent | claude-agent-acp | v0.1 |
| Codex | codex-acp | v0.1 |
| Gemini CLI | ACP | later |
| OpenCode | ACP | later |

## First task

An ACP client that starts a session, streams its events into `JobEvent`s on the core event bus, supports cancel, and stores the transcript path on the `Job`. Then the two v0.1 adapters.

## Must never

- Never read `~/.claude`, `~/.codex/auth.json` or any other vendor auth file.
- Never run an OAuth flow, store a token or proxy one. Users sign in to the official binaries themselves.
- Never patch a binary, spoof a header or imitate a vendor system prompt.
- API keys are first-class. Ship a flag that disables subscription-backed workers so users can comply instantly if vendor policy changes.

Background: [docs/RESEARCH.md](../../../docs/RESEARCH.md), sections 3 and 6.
