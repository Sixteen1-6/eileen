# Brain latency on the owner's plan, measured 2026-10-20

The subscription-harness report (2026-10-11) could not time a model turn because the standalone Claude CLI was not signed in. The owner signed it in on 2026-10-20 (Claude CLI 2.1.304, `authMethod: claude.ai`), so this note fills that gap.

## Setup

- One `claude -p` process kept alive with `--input-format stream-json --output-format stream-json --verbose --include-partial-messages`, model `haiku`, `--no-session-persistence`, `--setting-sources ""`, `--tools ""`, a one-sentence system prompt.
- Three short prompts per process, sent as stream-json user messages. "First text" is the first `text_delta` stream event after the message is written; "done" is the `result` event.
- The PC (Core Ultra 7 155H, 16 GB) was busy with four research agents during the run, so these numbers are pessimistic.

## Results

| Configuration | Cold start to first answer | Warm turn, first text | Warm turn, done |
|---|---|---|---|
| Default (user MCP servers and claude.ai connectors load) | 42.6 s | 3.8 to 8.1 s | 6.2 to 10.4 s |
| `ENABLE_CLAUDEAI_MCP_SERVERS=false`, `--strict-mcp-config` with an empty config, run 1 | 39.1 s | 2.2 to 4.2 s | 4.8 to 6.6 s |
| Same, run 2 | 48.1 s | 1.0 to 1.5 s | 2.3 to 3.1 s |

A debug log of a single `-p` call showed where cold start goes: about 2 s loading and filtering CA certificates, 3.4 s from tool-search setup to settings, 1.9 s loading MCP configs, and event-loop stalls of 0.5 s or more while the CPU was contended. The model call itself was about 2 s.

## What this means for the design

1. Start the brain session when Eileen starts, keep one hot spare, and never spawn a harness per utterance.
2. Run the brain with claude.ai connectors and user MCP servers off (`ENABLE_CLAUDEAI_MCP_SERVERS=false`, `--strict-mcp-config` with only Eileen's own server, `--setting-sources ""`). Connectors made warm turns two to four times slower.
3. A warm turn starts speaking in roughly 1 to 2 s when the PC is not overloaded. Streaming text into TTS at the first clause keeps voice to voice inside the budget.
4. Common commands (mute, pause, open an app, move a window) go through a deterministic fast path with no model call, so they are instant regardless of model latency.

The measurement script is kept in `C:\Users\shubh\Projects\eileen-workfiles\` on the owner's PC.
