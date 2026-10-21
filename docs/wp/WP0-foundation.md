# WP0: Foundation

The living plan for work package 0 of [BUILD.md](../BUILD.md) section 6.2: the workspace, the
contracts, the build guard, the quality gates and the guidance. The agent doing the work keeps it
current.

## Goal

Phase A (wave W0) leaves a workspace with every pin and native dependency declared, the contracts,
the build guard, the basic gates and `pr-meta` green on Windows and Linux, `main`'s required checks
switched to `all-green` and `pr-meta`, and every crate as a documented stub. Phase B (wave W1) adds
the remaining repository checks and the nightly, weekly, long and release workflows.

## Done when

The acceptance list of BUILD.md section 6.2, items 1 to 12, each recorded below with its evidence.
Items 4 and 10 need measurements on the owner's PC and on the first CI runs, which this file
records when they exist.

## Progress

| Order | Branch | State |
|---|---|---|
| 1 | `wp0/ci-bootstrap` | Built and checked locally on 2026-10-21; waiting for its first CI run |
| 2 | `wp0/contracts` | Not started |
| 3 | `wp0/base-crates` | Not started |
| 4 | `wp0/stubs` | Not started |
| 5 | `wp0/xtask` | Not started |
| 6 | `wp0/guidance` | Not started |

`wp0/ci-bootstrap` holds the workspace root with the pins of section 4.1, the configuration of
section 4b, `ci.yml` (`plan`, `hygiene`, `logic-linux`, `windows-clippy`, `windows-test`,
`all-green`), the first cut of `cargo xtask` with `pr-meta` and its fixture tests, `pr-meta.yml`,
the pull request template, Dependabot and `CODEOWNERS`. The tag `python-scaffold-final` marks
`main`'s last commit with the Python scaffold, `7694c44`.

## Switching main's required checks

On 2026-10-21 `main` required `test (3.12)` and `test (3.13)`, both from GitHub Actions (app id
15368), with `strict` on (read with `gh api repos/Sixteen1-6/eileen/branches/main/protection`). The
order of the switch, from BUILD.md sections 3.2 and 6.2:

1. Push `wp0/ci-bootstrap` with the Python workflow still present, as
   `.github/workflows/python-scaffold.yml`, so the old checks keep reporting.
2. Wait until `all-green` and `pr-meta` have reported on the pull request.
3. Switch the required checks (owner answer 2 authorises the assistant to run this):

   ```sh
   gh api --method PATCH repos/Sixteen1-6/eileen/branches/main/protection/required_status_checks \
     --input - <<'EOF'
   {"strict": true, "checks": [{"context": "all-green", "app_id": 15368}, {"context": "pr-meta", "app_id": 15368}]}
   EOF
   ```

   Then confirm it:

   ```sh
   gh api repos/Sixteen1-6/eileen/branches/main/protection/required_status_checks \
     --jq '[.strict, (.checks[].context)]'
   ```

   which prints `[true,"all-green","pr-meta"]`.

4. Set the squash default to the pull request title and description. It already read `PR_TITLE`
   and `PR_BODY` on 2026-10-21; the command is idempotent:

   ```sh
   gh api --method PATCH repos/Sixteen1-6/eileen \
     -f squash_merge_commit_title=PR_TITLE -f squash_merge_commit_message=PR_BODY
   ```

5. Push the commit that deletes the Python scaffold (`src/`, `tests/*.py`, `pyproject.toml` and
   `python-scaffold.yml`), and merge on green.

## Surprises

- Cargo refuses a workspace member path or glob that matches nothing, so the root manifest lists
  only `xtask` for now. `crates/*` joins with the first crate (`wp0/contracts`) and
  `tools/repo-tools` with phase B. Unused path entries in `[workspace.dependencies]` are accepted,
  so every internal crate is declared already.
- nextest refuses a `package()` or `binary_id()` filter term that matches no workspace member, so
  the two overrides in `.config/nextest.toml` keep only their test-name terms until `wp0/stubs`
  creates the crates they name.
- `toml` 1.1.8 does not depend on `toml_edit`. The pin is `toml_edit` 0.25.17, released with it and
  sharing its `toml_datetime`, `toml_parser`, `toml_writer`, `serde_spanned` and `winnow`
  requirements, so the tree holds one copy of each.
- Section 4.1 names `shuttle` 0.9.7, but crates.io dates it 2026-10-14, inside the 14-day window.
  The rule wins: `shuttle` 0.9.5 (2026-10-01), `shuttle-tokio` 1.0.0 (2026-09-04), `syn` 2.0.119
  (2026-07-15) and `rtrb` 0.4.0 (2026-08-17, the newest release; 0.3.5 is a later backport).
- `cargo deny check` passes on the current tree with one warning per allowed license the tree does
  not use yet, IJG and the `webpki-roots` exception included. Whether `ring`'s `Apache-2.0 AND
  ISC` passes is confirmed when `wp0/stubs` brings it into the tree.

## Decisions

- Workflows install the toolchain with `rustup toolchain install`, which reads
  `rust-toolchain.toml`, before `rustup show`, so a job does not depend on whether `rustup show`
  installs a missing toolchain (that behaviour changed in rustup 1.28).
- The `plan` job computes the path classes with `dorny/paths-filter`, and `pr-meta` computes the
  same classes in Rust. The xtask test `ci_filters_match_the_classifier` fails when the two drift.
- `pr-meta` treats a fix as carrying a regression when it adds or changes a file under
  `scenarios/`, `fuzz/regressions/`, `crates/eileen-sim/regressions/` or any `tests/` folder, or
  the file that defines a test its Evidence lists, so unit tests in `#[cfg(test)]` modules count.
- `pr-meta` requires `## Seen failing before the fix` to say something for a fix, and allows that
  heading to be absent or empty for other types.
- The title check reads "imperative" as: the first word is not a gerund, a past tense or a
  third-person form, judged by its ending with short lists of base verbs that share those endings.

## Measurements

To record from the first CI runs and the owner's PC (acceptance 4 and 10): the Windows job times
cold and warm, the per-worktree target size of the stub workspace, the sccache hit rate after a
second worktree's build, and the build guard's baseline.

## Outcome

Open.
