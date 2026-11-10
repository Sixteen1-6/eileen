# Stream fixtures

Byte-for-byte copies of four fixtures from WP3's approval contract test on CLI 2.1.304, kept here
so the stream parser's unit tests (`src/stream/tests.rs`) read their own inputs and a change to
them runs the Rust lanes. The sources are in `docs/research/fixtures/wp3-approval/`, recorded
and redacted by that test's `make_fixtures.py`
([2026-10-wp3-approval-contract.md](../../../../../docs/research/2026-10-wp3-approval-contract.md)).
A re-record there does not change these copies; update them on purpose, in a pull request that
also runs the stream tests.

| Fixture | What the tests read |
|---|---|
| `stream-events.json` | `system/init`, the hook events, both `system/permission_denied` events, every failure case's `tool_result` block and the recorded `result` events |
| `stream-pt-allow.json` | A whole run in order: the guard asks, `approve` allows, the command runs |
| `stream-steer.json` | A whole run of two turns: the guard denies and waits, then a steer turn retries |
| `stream-stdio-allow.json` | A whole run on the stdio route, with its `can_use_tool` control request |

`stream-events.json` holds bare `tool_result` blocks; the tests put each inside the `user` line
the whole runs show, as the CLI writes it. The seed corpus under `fuzz/corpus/stream_json_line/`
holds single lines taken from these files, written compactly; the seeds whose names start with
`hand-` hold shapes the fixtures leave out (the rate-limit event, partial messages, a failed
sign-in, a `Skill` call, narration, an unknown type and lines that do not parse), written from
the forms section 3.3.8 and the research reports document.
