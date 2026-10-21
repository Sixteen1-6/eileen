## Evidence

<!-- Lead with what shows the change is right, before anything else. This section runs to the next "## " heading.
Write prose lines, plus any of these field lines, each at the start of a line and at most once:
tests: crate_name::module::test_name, xtask::module::test_name
scenarios: area/scenario-id
numbers: what was measured, before and after
replay: rec_...
incident: inc_...
owner-run: link to an Actions run of this repository or a file on the devhub branch
override: why the evidence job cannot show this fix failing on Linux (needs the owner's evidence-reviewed label)
Only the tests: and scenarios: lines are read as ids. For a feature, fix or refactor, list at least one id there; every one listed must exist.
For a fix: the test or scenario that failed before this change and passes now.
For docs, scenario and amendment pull requests: say what you checked and how (a command and its result, or a link).
This section must not be left empty or as this comment. -->

## Type

type: <!-- feature | fix | refactor | docs | scenario | amendment -->

## What

One paragraph on what this change does.

## Why

Why it is needed now. Link the owner direction, decision record or research section it serves if that helps a reviewer.

## Tests at which layer

Unit, property, snapshot, integration, simulation, fuzz, end to end, benchmark, or none (and why).

## Seen failing before the fix

For fixes only: how the failure was reproduced before the change.

## Scenario

The scenario this change adds or relies on, if any.

## Suggested guidance changes

Rules for AGENTS.md or a crate doc that this work suggests, if any. Do not edit guidance files in a feature pull request.

## Architectural changes

New dependencies, contract changes, new top-level Input, Effect or BusEvent variants, or "none".

## Playbook

If this changes docs/DECISIONS.md: what changes in docs/PLAYBOOK.md, or why it is unaffected.

## Checklist

- [ ] No credentials, tokens or personal paths in code, tests, fixtures or docs
- [ ] No em dashes in docs (use commas, periods or parentheses)
