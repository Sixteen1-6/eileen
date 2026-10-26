# WP9: The test fleet, benchmarks and the integration demo

The living plan for work package 9 of [BUILD.md](../BUILD.md) section 6.11: the fleet, the
benchmarks, the soak and the M6 demo. The agent doing the work keeps it current.

## Goal

- `eileen-fleet`, shipped: `eileen test --fleet` with one-time consent, the machine hash from the
  user's own `fleet\machine.toml`, recorded and live modes, attended runs and armed unattended runs
  with their safety rules, previewed `FleetResult` files submitted to the `devhub` branch through
  the contributor's own `gh`, and the validation behind `cargo xtask fleet-validate`.
- `eileen-e2e`, development only: benchmarks B1 to B10 and B8-curve with A/B runs against the
  base binary, the soak runner behind `cargo xtask soak`, ingest and clustering of fleet results,
  flake tracking, the data the nightly Pages build renders, and `cargo xtask triage`.
- The `flaky-login` fixture repository, and `docs/FLEET.md`.

Then every desktop-lane scenario, B1 to B10, B8-curve, the first-run demo and the M6 demo run on
the owner's PC. The results, the memory admission record and each run's machine plan are recorded
here, followed by an amendment that tunes the memory settings and the plan's rules from them, and
an `amendment` pull request that adds the measured results to `docs/PLAYBOOK.md`.

## Branch, dependencies and files

- Branch: `wp9/<topic>`.
- Depends on: WP0 to WP8. Wave: W6. Desktop: yes.
- Owns: `crates/eileen-fleet`, `crates/eileen-e2e`, `scenarios/bench/` (including
  `_fixtures/flaky-login/`), `docs/FLEET.md`, the fuzz target `fleet_result` with its corpus and
  regressions, and this file.

## Done when

The acceptance list of BUILD.md section 6.11, items 1 to 10, each recorded under Progress with its
evidence when it holds. The lines below are reminders; the full text in BUILD.md is the bar.

1. Every desktop-lane scenario runs on the owner's PC with a recorded result; failures are issues,
   not skips.
2. B1 runs for real with a sharpness gain of at least 15 percent, B8's learned runs are cheaper,
   B8-curve matches section 2.10, and B10 records Claude's numbers.
3. A second Windows 11 machine installs Eileen from `TESTING.md` unaided, saves a scenario by
   voice and submits it from its own GitHub account; CI validates and replays it.
4. Recorded-mode fleet runs on two machines give result files with no personal data and a stable
   machine hash, submitted all three ways, validated and clustered.
5. The soak runs 4 hours on `windows-latest` with flat handles, GDI objects and threads, and a core
   memory slope under 1 MB per hour.
6. The M6 demo script runs end to end on the owner's single monitor with no restart and no typed
   command, asserting the run-1 learning row and the admission times.
7. The fuzz target `fleet_result` runs clean for 10 minutes (`long.yml`).
8. The unattended-run rules on the owner's PC: the locked-session skip, abort on physical input,
   auto-denied tier 2, the refusals, the power request and the lock afterwards.
9. The machine profile across PCs: machines of different classes record different plans with no
   setting changed, and the results page groups benchmark lines by class.
10. The hosted desktop lane runs nightly on `windows-2025` with owned fixtures at medium
    integrity, and skips by name the scenarios whose `requires` it cannot meet.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

Not started.

## Surprises

None yet.

## Decisions

None yet.

## Outcome

Open.
