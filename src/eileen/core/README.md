# core

The coordinator: job registry, event bus, router and permission gate. It owns the rule that conversation never blocks on work. Every agent session is an async job it tracks, names and reports on.

## Builds on

| Piece | Choice |
|---|---|
| Job and event models | `eileen.schemas.jobs` |
| Routing policy | `src/eileen/policy/default.yaml` loaded by `eileen.schemas.policy` |
| Direct model calls | LiteLLM (MIT), with per-request override |
| Registry storage | SQLite: id, generated name, kind, status, origin, last progress, salience, transcript path |

## First task

A `JobRegistry` on SQLite and an in-process `EventBus`. Harness adapters publish `JobEvent`s; the voice loop and the phone bridge subscribe. Include `resolve_reference("that simulation")`: match by kind word, then recency, and ask when two jobs tie.

## Rules it enforces

- The user is the supreme router. A spoken `Override` is applied on top of policy and wins immediately.
- Permission tiers: reversible actions run at once; send, delete and spend confirm first.
- Secrets never enter model-visible text. The registry stores paths and names, never tokens.

Background: [docs/RESEARCH.md](../../../docs/RESEARCH.md), sections 6 and 9.
