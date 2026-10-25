# scenarios

The public scenario corpus: tests written as data, from developers and from real use on many PCs
(BUILD.md sections 3.3.14 and 4.8). Every area belongs to the package that owns it (BUILD.md
section 3.1's table of folders). WP0 phase B adds `README.md`, the schema in `_schema/` and the
examples in `core/`.

## Why this folder has its own rules

Anyone can contribute a scenario from their own PC, and everything here is public, so privacy
rules apply to every line, and a scenarios-only pull request is checked differently from code.

## Rules

- A scenario lives in `scenarios/<area>/<id>/scenario.toml` and validates against the schema
  generated from `eileen_contracts::scenario` into `_schema/`.
- No personal data: every input line passes the Redactor and the PII scan, no personal paths, no
  binary files, no audio unless it is marked and consented, no `origin = "user"`, and fixtures
  only from an area's `_fixtures/` templates (check 18).
- A `stale` status or a quarantine has an expiry within its cap and an issue link, and no
  scenario's `since` is later than the commit that added it.
- A scenarios-only pull request touches only `scenarios/<area>/<id>/` (check 28). A contributed
  scenario needs the `scenario-reviewed` label before the desktop and fleet lanes run it.
- Contributed scenarios are MIT; consented audio is CC-BY-4.0 with the speaker's consent record
  (owner answer 18).
- `cargo xtask scenarios` runs the checks and the replays once WP0 phase B builds it.
