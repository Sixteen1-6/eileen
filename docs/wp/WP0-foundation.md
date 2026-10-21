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
| 1 | `wp0/ci-bootstrap` | Pull request #13, 2026-10-21: green, required checks switched, Python scaffold deleted |
| 2 | `wp0/contracts` | Not started |
| 3 | `wp0/base-crates` | Not started |
| 4 | `wp0/stubs` | Not started |
| 5 | `wp0/xtask` | Not started |
| 6 | `wp0/guidance` | Not started |

`wp0/ci-bootstrap` holds the workspace root with the pins of section 4.1, the configuration of
section 4b, `ci.yml` (`plan`, `hygiene`, `logic-linux`, `windows-clippy`, `windows-test`,
`all-green`), the first cut of `cargo xtask` with `pr-meta` and its fixture tests, `pr-meta.yml`,
the pull request template, Dependabot and `CODEOWNERS`. The tag `python-scaffold-final` marks
`main`'s last commit with the Python scaffold, `7694c44`, and is on GitHub.

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

5. Push the commit that deletes the Python scaffold (`src/`, `tests/*.py`, `pyproject.toml`,
   `python-scaffold.yml` and the Python lines of `.gitignore`), and merge on green.

Done on 2026-10-21 in that order. On pull request #13's first run (`all-green` and `pr-meta`
green, `test (3.12)` and `test (3.13)` green beside them) the PATCH above ran and the read-back
printed `[true,"all-green","pr-meta"]`, both with app id 15368. The full protection, read before
and after, differs only in those checks: `strict`, enforce admins, linear history, conversation
resolution, no force pushes and no deletions are unchanged. The squash default command returned
`PR_TITLE` and `PR_BODY`. Acceptance 1's last clause, the next pull request going green with no
Python job, is checked by `wp0/contracts`.

## Steps for the later phase A pull requests

- `wp0/contracts`, `wp0/base-crates` and `wp0/stubs` each add the `[workspace.dependencies]` line
  of every crate they create, `name = { path = "crates/<name>" }`, beside the `members` entry.
- After `wp0/stubs` merges, check that Dependabot's next `cargo` run finishes (the repository's
  Insights, Dependency graph, Dependabot tab) and record it here.
- `wp0/stubs` is the first pull request to lock `shuttle` and `shuttle-tokio`. Its description
  lists the publish date of every package it adds to `Cargo.lock`, and any package younger than 14
  days that no section 4.1 pin names is held back with `cargo update -p <name> --precise
  <version>` to its newest older release, or named in the description with the reason it stays.

## Surprises

- Cargo refuses a workspace member path or glob that matches nothing, so the root manifest lists
  only `xtask` for now. `crates/*` joins with the first crate (`wp0/contracts`) and
  `tools/repo-tools` with phase B.
- Cargo accepts `[workspace.dependencies]` path entries for crates that do not exist yet, but
  Dependabot does not. Its cargo file fetcher (`cargo/lib/dependabot/cargo/file_fetcher.rb` in
  dependabot-core, read on 2026-10-21) fetches `<path>/Cargo.toml` for every workspace dependency
  and stops the whole run with "path dependencies not reachable" when one is missing. So the
  internal lines join with their crates, as `members` does, instead of all at once.
- nextest refuses a `package()` or `binary_id()` filter term that matches no workspace member, so
  the two overrides in `.config/nextest.toml` keep only their test-name terms until `wp0/stubs`
  creates the crates they name.
- `toml` 1.1.8 does not depend on `toml_edit`, so section 4.1's rule for it (the version `toml`
  depends on) names nothing. The pin follows the 14-day rule instead: `toml_edit` 0.25.15
  (2026-09-11). Its requirements accept the `toml_datetime` 1.1.2, `toml_parser` 1.1.5,
  `toml_writer` 1.1.3, `serde_spanned` 1.1.2 and `winnow` 1.0.4 that `toml` 1.1.8 locks, so the
  tree holds one copy of each. 0.25.16 and 0.25.17 (2026-10-08 and 2026-10-09) are inside the
  window.
- Section 4.1 names `shuttle` 0.9.7, but crates.io dates it 2026-10-14, inside the 14-day window.
  The rule wins: `shuttle` 0.9.5 (2026-10-01), `shuttle-tokio` 1.0.0 (2026-09-04), `syn` 2.0.119
  (2026-07-15) and `rtrb` 0.4.0 (2026-08-17, the newest release; 0.3.5 is a later backport).
- A version requirement in `Cargo.toml` is a caret range, and the first lock takes the newest
  release in it, so a cooled-off choice holds only when it is written exact. `shuttle = "=0.9.5"`
  and `toml_edit = "=0.25.15"` are exact (the first lock would otherwise take `shuttle` 0.9.8,
  2026-10-20, and `toml_edit` 0.25.17); `syn` 2.0.119, `rtrb` 0.4.0 and `shuttle-tokio` 1.0.0 are
  the newest releases in their ranges, and Dependabot proposes bumps of exact requirements too.
  The pins hold direct requirements only: transitive packages lock at their newest compatible
  release (this branch's lockfile has `syn` 3.0.9 of 2026-10-12, through `serde_derive`), which
  the `wp0/stubs` step above and phase B's `deps-age` check cover.
- `cargo deny check` passes on the current tree with one warning per allowed license the tree does
  not use yet, IJG and the `webpki-roots` exception included. Whether `ring`'s `Apache-2.0 AND
  ISC` passes is confirmed when `wp0/stubs` brings it into the tree.

## Decisions

- Workflows install the toolchain with `rustup toolchain install`, which reads
  `rust-toolchain.toml`, before `rustup show`, so a job does not depend on whether `rustup show`
  installs a missing toolchain (that behaviour changed in rustup 1.28).
- The `plan` job computes the path classes with `dorny/paths-filter`, and `pr-meta` computes the
  same classes in Rust. The xtask test `ci_filters_match_the_classifier` reads the two filter
  steps out of `ci.yml`, evaluates their globs and fails when any sample path's classes differ.
- `pr-meta` treats a fix as carrying a regression when it adds or changes a file inside a
  scenario folder (`scenarios/<area>/<id>/`, areas starting with `_` excluded), under
  `fuzz/regressions/`, `crates/eileen-sim/regressions/` or any `tests/` folder, or an added or
  changed line inside a listed test or the test module around it. The test module counts, not
  only the test, because a new case or fixture beside the listed test is how such fixes are often
  shown, and it is the inline counterpart of a file under `tests/`. A change elsewhere in the file
  does not count.
- A `tests:` id must name a test: a function with an attribute whose path ends in `test`
  (`#[test]`, `#[tokio::test]`, `#[test]` inside `proptest!`). Other functions do not resolve.
- `deps`, and every Dependabot pull request, may change manifests and lockfiles, and otherwise
  only `uses:` lines of an existing workflow under `.github/workflows/` or action file
  (`action.yml` at the root or under `.github/actions/`). A change with no text line (a binary or
  mode-only change) fails.
- `cargo xtask pr-meta` takes `--title` from CI. Without it, the local form section 4b.3 documents
  checks the head commit's subject, which GitHub proposes as the title of a one-commit pull
  request, and the report names the title it checked.
- `pr-meta` requires `## Seen failing before the fix` to say something for a fix, and allows that
  heading to be absent or empty for other types.
- The title check reads "imperative" as: the first word is not a gerund, a past tense or a
  third-person form, judged by its ending with short lists of base verbs that share those endings.

## Measurements

To record from the first CI runs and the owner's PC (acceptance 4 and 10): the Windows job times
cold and warm, the per-worktree target size of the stub workspace, the sccache hit rate after a
second worktree's build, and the build guard's baseline.

The first CI run, pull request #13 on 2026-10-21 (run 39562667916, workspace of `xtask` only, no
cache, since rust-cache saves only from `main`): `plan` 6 s, `hygiene` 15 s, `logic-linux` 28 s,
`windows-clippy` 51 s, `windows-test` 54 s, `all-green` 2 s; 70 s from the first job's start to
`all-green`'s end. `pr-meta` took 14 s in its own workflow. These are cold times for a one-crate
workspace. Warm times need the cache that this pull request's push to `main` saves, so the next
pull request records them, and `wp0/stubs` records both again with the full stub workspace,
beside its target size.

## Outcome

Open.
