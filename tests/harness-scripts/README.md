# Harness scripts

Scripts that `eileen-stub-harness` plays in place of the claude CLI: one
`StubScriptLine` (BUILD.md section 3.3.16) per line, as JSON. The stub writes each `event` line
to stdout as the CLI writes it, waits at `await_user` for the next stream-json user line on
stdin, and plays `delay`, `crash`, `malformed` and `narration` lines as its crate doc says.

Each script is derived from the real runs of the approval contract test on CLI 2.1.304
(`docs/research/fixtures/wp3-approval/`, described in
`docs/research/2026-10-wp3-approval-contract.md`). Those fixtures were already redacted
(the user's folders, transcript paths and plan usage removed); every line here also passes
through `eileen_trace::Redactor` unchanged, which `eileen-stub-harness::it::corpus` checks.
Tool use ids, message ids and the prose are rewritten for each script's task; the field sets
and the `result` shape are the recorded ones.

`system/init` is the recorded one with one difference. The fixtures kept only the fields an
approval parser reads and listed the names of the others under a `$omitted` key, which the CLI
never writes, so the scripts drop that key and put back, with placeholder values, the fields
whose types the Agent SDK documents for this message: `cwd` (the fixtures' redacted scratch
path, `<runs>\pt-allow\proj` for the workers and `<runs>\brain` for the brain),
`slash_commands`, `skills` and `plugins` (empty lists) and `output_style` (`"default"`). The
other names the fixtures list are left out, because no recording holds their values or types:
`additional_directories`, `analytics_disabled`, `fast_mode_disabled_reason`, `fast_mode_state`,
`messaging_socket_path`, `original_cwd`, `per_turn_effort_active`, `powershell_path`,
`product_feedback_disabled`, `terminal_slash_commands` and `view_mode`. The owner-run recording
puts back the real values of all of them. A key that starts with `$` is a fixture's annotation,
never the CLI's, and `eileen-stub-harness::it::corpus` fails on one in any `event` line.

| Script | What it plays | Source |
|---|---|---|
| `worker-hello.ndjson` | A worker job: narrates, runs a Bash call that writes `hello.txt`, gets its result, answers and ends the turn | `stream-pt-allow.json`: its `system/init`, the `tool_use`, the `tool_result` user line, the closing assistant text and the `result`, with the command changed from `touch tier2-asked.txt` to `echo hi > hello.txt`; the narration line is the stub's own `narration` kind |
| `worker-skill.ndjson` | A worker calls a learned skill through the `Skill` tool, gets the skill's launch result and answers | The same recorded shapes as `worker-hello.ndjson`, with the `Skill` tool in `system/init`'s tools and a `Skill` `tool_use` whose input is `skill` and `args`, the input shape `eileen_harness::stream` turns into `SkillInvoked` |
| `worker-crash.ndjson` | A worker narrates, starts a Bash call and the process exits with code 3 before the result | `stream-pt-allow.json`'s `system/init` and `tool_use`, cut where the CLI would be waiting for the tool, then a `crash` line |
| `worker-malformed.ndjson` | A truncated assistant line between `system/init` and a normal answer | `stream-pt-allow.json`'s `system/init`, closing assistant text and `result`; the malformed line is the first bytes of an assistant line |
| `brain-two-turns.ndjson` | A brain conversation of two turns, each streamed as text deltas and ended by a `result` | `stream-events.json`'s `system/init` (with no tools, as the brain runs) and `result`, and the assistant message shape of `stream-pt-allow.json`. The approval runs did not pass `--include-partial-messages`, so no `stream_event` line was recorded; those lines follow the Anthropic streaming events the CLI wraps (`message_start`, `content_block_start`, `content_block_delta` with `text_delta`, `content_block_stop`, `message_delta`, `message_stop`), as `docs/research/2026-10-subscription-harnesses.md` describes them. The owner-run recording replaces them |

Scripts recorded from new real sessions on the owner's PC, including the partial messages of a
brain turn, come with the owner-run pull request, and `cargo xtask contract-claude` compares
these scripts' event shapes with the real CLI's every night.
