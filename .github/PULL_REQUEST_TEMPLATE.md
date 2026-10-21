## Evidence

<!--
Lead with what shows the change is right. This section runs to the next "## " heading and is
checked by `cargo xtask pr-meta` (BUILD.md section 4b.3, check 8). Write prose lines, plus at most
one of each of these field lines, each starting at the beginning of a line:

tests: <package>::<module>::...::<fn>, ...
scenarios: <area>/<id>, ...
numbers: what was measured, before and after
replay: rec_<ulid>
incident: inc_<ulid>
owner-run: a link to an Actions run of this repository, or to a file on the devhub branch
override: why the evidence job cannot show this fix failing on Linux (needs the owner's label)

A test path starts with the package (any workspace member, hyphens or underscores). A unit test
then gives its module path from the crate root, as in eileen-core::turn::tests::barge_in_pauses.
An integration test gives its test target first: eileen-core::it::job_model::<fn> is a test in
tests/it/job_model.rs. Only the tests: and scenarios: lines are read as ids, and every id listed
must exist at the head commit; a test path must name a test (#[test], #[tokio::test] and the
like), not other code.

A feature, fix or refactor lists at least one id. A fix lists the test or scenario that failed
before the change and passes now, and adds or changes a regression: a scenario, a file under
tests/, or the listed test or the test module around it. Docs, scenario and deps pull requests
say what was checked and how: a command and its result, or a link. This section is never left
empty or as this comment.
-->

## Type

type: <!-- one of: feature, fix, refactor, docs, scenario, amendment, deps -->

## What

<!-- One paragraph on what this change does. -->

## Why

<!-- Why it is needed now. Link the owner direction, decision record or research it serves. -->

## Tests at which layer

<!-- Unit, property, snapshot, integration, simulation, fuzz, end to end, benchmark, or none and
why. -->

## Seen failing before the fix

<!-- Fixes only: how the failure was reproduced before the change. Leave it empty otherwise. -->

## Scenario

<!-- The scenario this change adds or relies on, if any. -->

## Suggested guidance changes

<!-- Rules for AGENTS.md or a crate doc that this work suggests. Guidance files are not edited in
feature pull requests. -->

## Architectural changes

<!-- New dependencies, contract changes, new top-level Input, Effect or BusEvent variants, or
"none". -->

## Playbook

<!-- If this changes docs/DECISIONS.md: what changes in docs/PLAYBOOK.md, or why it is
unaffected. -->
