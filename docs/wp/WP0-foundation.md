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
| 2 | `wp0/contracts` | Pull request #15, 2026-10-21: green on Windows and Linux with no Python job; its amendment of section 3.3, pull request #16, merged first |
| 3 | `wp0/base-crates` | Not started |
| 4 | `wp0/stubs` | Not started |
| 5 | `wp0/xtask` | Not started |
| 6 | `wp0/guidance` | Not started |

`wp0/ci-bootstrap` holds the workspace root with the pins of section 4.1, the configuration of
section 4b, `ci.yml` (`plan`, `hygiene`, `logic-linux`, `windows-clippy`, `windows-test`,
`all-green`), the first cut of `cargo xtask` with `pr-meta` and its fixture tests, `pr-meta.yml`,
the pull request template, Dependabot and `CODEOWNERS`. The tag `python-scaffold-final` marks
`main`'s last commit with the Python scaffold, `7694c44`, and is on GitHub.

`wp0/contracts` adds `crates/eileen-contracts` (and `crates/*` to the members): every type of
section 3.3 with serde and schemars, one module per producing area, the schema catalogue, one
insta snapshot per module and per toolset (24 in `crates/eileen-contracts/tests/it/snapshots/`,
keys sorted), `modules.toml` (the eight modules on slice S1's path pinned, the other sixteen
provisional with the package that pins each), `text-allowlist.toml` with the text walk,
`desktop::sanitize`, the scenario expiry rule, the speech frame header, the recording formats and
the seed corpora of the `uia_text` and `desktop_state` fuzz targets, whose `fuzz_entry` functions
the corpus replay test calls. The cargo-fuzz target files join with `wp0/stubs`, which creates
`fuzz/Cargo.toml` with every target.

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
Python job, held on `wp0/contracts`: pull request #15's first run (run 39580553639) passed
`all-green` and `pr-meta`, the two required checks, and ran no Python job.

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

- schemars gives every instantiation of a generic type the same schema name and resolves the
  clash with a numeric suffix (`Envelope2`, `Envelope3`), which would make snapshots depend on
  registration order. `Envelope` carries `#[schemars(rename = "Envelope_{E}")]`, and the test
  `every_definition_has_one_owner` fails on any definition no module registered.
- Doc comments become schema descriptions, so editing a doc comment in a pinned module changes its
  snapshot, which makes it an amendment like any other contract change.
- clippy reads integration-test helpers as ordinary code (no test allowances, and
  `tests_outside_test_module` fires) unless each module of `tests/it/main.rs` is `cfg(test)`.
- The first lock of the contracts' dependencies took `pest` 2.9.4 (through `insta`'s
  `redactions` feature), 2026-10-16, which also brought `stacker`, `psm`, `cc` 1.9.0 and
  `find-msvc-tools` 0.1.15, all inside the 14-day window, and `libc` 0.2.191 (2026-10-16). They
  are held back with `cargo update --precise` to `pest` 2.9.2 (2026-09-21) and `libc` 0.2.190
  (2026-10-02); `pest` 2.9.2 needs none of the four build crates.

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

- Contracts, `wp0/contracts`: a type that appears only in another crate's own signatures (driver
  and platform errors, `HarnessConfig`, the brain benchmark's suite and result, the platform
  handles of section 3.4) is that crate's. Types the plan writes under another crate's heading but
  that a contract type or a port names (`SessionSpec`, `JobBinding`, `BrainSlot` under
  `eileen-harness`; `ResolveCtx` and `Resolution` under `eileen-intent`; `SettingChange`,
  `Proposal` and `SettingsVersion` under `eileen-config`) are contracts, because
  `eileen-contracts` may depend on nothing.
- Contracts: `contracts/modules.toml` and `contracts/text-allowlist.toml` in section 3.3 are read
  as the crate's own folder, `crates/eileen-contracts/` (section 3.1 has no root `contracts/`).
- Contracts: one snapshot per module holds that module's own definitions, with other modules'
  types as `$ref`s, so a field added in one module changes one file; a toolset's snapshot also
  holds each tool's self-contained input schema, which does change when a type it uses changes,
  because that is what the model sees.
- Contracts: `Option` fields serialize as `null` and read a missing field as `None`; a list or
  flag a model or a person may leave out (tool arguments, scenario files, the configuration)
  defaults when missing; wire types between Eileen's own processes require everything else.
  Scenario files refuse unknown fields in every table they write (steps, HUD inputs and
  selectors included), and a step with no fields is an empty struct variant (`Approve {}`), so
  `approve = {}` reads the same in serde and in the generated schema. A unit test holds the
  plan's example, extended with every such shape, against that schema with a small checker of
  the keywords the contracts use (no validator crate is pinned); phase B's check 18 uses a real
  validator.
- Contracts: the plan's names are kept. The `capabilities` tool's argument struct keeps the
  name `Capabilities` beside the resolver's `intent::Capabilities` and publishes its schema as
  `CapabilitiesArgs`; `config::Component` (a component a setting restarts) and the
  `ports::Component` trait share a name in separate modules. Large enum payloads are boxed
  (`Effect::Hud`, `Record::JobUpsert`, `WorkerCmd::Start`, `SidecarEvent::Ready`,
  `LearnEvent::RouteProposed`, `RecordLine::Input` and a few more), which leaves the wire unchanged.
- Contracts: two plain strings of the plan carry vendor text and are wrapped as `Untrusted`
  (`WorkerEvent::AuthFailed.message`, `HookRequest::StopFailure.error`); every other plain string
  reachable from `Input`, `ToolResult` and `DesktopState` is in the allowlist with its reason.
  The text walk also counts a schema that accepts any value (a `serde_json::Value`) and the keys
  of a map. Of what that found, slot values are `Untrusted` (the user's words or a model's
  dispatch), the HUD's setting value is `Personal` (the user's own input), and the rest (tool
  and op schemas Eileen generates, recipe arguments the linter traces, slot names, engine
  names) is allowlisted. `ElementInfo.automation_id` and a selector's name and automation id
  are `Untrusted`, because in Chromium, Firefox and Electron apps the automation id is the DOM
  `id` a page sets.
- Contracts: `TraceCtx::traceparent` takes the span id as an argument, and returns `None` when the
  context has neither turn nor job or an id would be all zeros (the W3C rule); `BrainCmd::Promote`
  names the slot it promotes, so every brain command names a slot.
- Contracts: every shape above that differs from BUILD.md's text, in pinned and provisional
  modules alike, is written into BUILD.md by the one-file amendment `wp0/contracts-amendment`
  (pull request #16), which merged before `wp0/contracts`, so the spec of record and the pinned
  contract agree.
- Contracts: pinned modules hold only the vocabulary the plan states. `UserErrorKind` is
  `JobKindUnavailable` and `Unexpected` (WP1 adds the kinds with their sentences and fixes);
  `IncidentTrigger` lists the triggers of sections 2.13 and 4.7 as unit variants (WP1 adds
  more). The speech pipe's PCM framing (how the JSON header ends) and the cap on command and
  event bodies are code, not schema, and WP4 settles both with its `speech_frame` parser.
- Contracts: the shim's pipe lines of section 3.3.9 are contract types in the provisional
  `tools` module (`ShimLine`, `DispatcherLine` and their bodies), which WP2 pins with the
  dispatcher.
- Contracts: a written recording line may hold `{"$blob": "b3:..."}` in place of any string over
  the writer's limit, and escapes an object that would read as one as `{"$lit": ...}`;
  `record::stub_blobs` and `record::resolve_blobs` are exact inverses (a property test), so
  `RecordLine` itself never holds a stub. An operand literal shaped like a reference is written
  `{"$lit": ...}` the same way.
- Contracts: a `SimRegression` TOML file writes each input as a string of JSON and the seed as a
  decimal string, because TOML has no `null` (a tool call's arguments, `Calibration(None)`,
  `Foreground(None)`) and no integer above `i64::MAX`; the struct's fields are as the plan writes
  them.
- Contracts: `SessionToken` keeps its text private (`new`, `expose`), compares every byte
  whatever the first difference, and has no `Hash`; the dispatcher and the hook endpoint (WP2)
  own verification.
- Contracts: the fuzz entry points return `Result` instead of asserting, because every Eileen
  binary links the crate; the corpus test and the cargo-fuzz targets fail on an `Err`.

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

The next run, pull request #15 (`wp0/contracts`) on 2026-10-21 (run 39580553639, the workspace of
`xtask` and `eileen-contracts`): `logic-linux` and `windows-test` restored #13's `main` caches as
partial matches (the lockfile had changed), so only the contracts' new dependencies and the crate
itself compiled; `windows-clippy` found no cache and built cold. `plan` 4 s, `hygiene` 14 s,
`logic-linux` 91 s, `windows-clippy` 153 s, `windows-test` 184 s, `all-green` 3 s; 195 s from the
first job's start to `all-green`'s end, and `pr-meta` 19 s. Locally, on the owner's PC with 3.9 GB
free and 4 jobs, the warm `cargo nextest run --workspace` ran 165 tests in 11 s.

## Outcome

Open.
