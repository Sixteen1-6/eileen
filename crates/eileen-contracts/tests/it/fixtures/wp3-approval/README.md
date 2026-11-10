# Approval fixtures

Byte-for-byte copies of two fixtures from WP3's approval contract test on CLI 2.1.304, kept here
so the wire tests (`tests/it/wire.rs`) read their own inputs and a change to them runs the Rust
lanes. The sources are in `docs/research/fixtures/wp3-approval/`, recorded and redacted by that
test's `make_fixtures.py`
([2026-10-wp3-approval-contract.md](../../../../../../docs/research/2026-10-wp3-approval-contract.md)).
A re-record there does not change these copies; update them on purpose, in a pull request that
also runs the wire tests.

| Fixture | Source | What the tests read |
|---|---|---|
| `mcp-approve.json` | `docs/research/fixtures/wp3-approval/mcp-approve.json` | The CLI's `tools/call` for `approve` and the `allow` and `deny` replies it accepted |
| `hooks.json` | `docs/research/fixtures/wp3-approval/hooks.json` | The tier guard's `PreToolUse` request it answered `ask`, for the same call |
