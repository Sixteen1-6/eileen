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
| 3 | `wp0/base-crates` | Pull request #73, merged on 2026-10-25 as a67d096: the six base crates, with the first review's fixes and the three fixes from WP10's Ubuntu 24.04 run; its amendment of section 6.1, pull request #22, merged on 2026-10-21. The second review's fixes follow in `wp0/base-crates-review` |
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

`wp0/base-crates` adds the six base crates and their `[workspace.dependencies]` lines:
`eileen-stdx` (the clock and ID ports with `SystemClock` and `OsIds`, `ManualClock` and `SeqIds`
for tests, `thread::spawn_named` and `thread::sleep`, the seeded `CoreRng`, `always!`, `never!`,
`buggify!` and `num`), `eileen-trace` (init per process kind, the JSON-lines file, console,
flight recorder ring and slow-span layers, `names` with the span-parent table and its checker,
the `pii.` policy, the `Redactor`, `privacy`, the Chrome converter, `traceparent`,
`test_support`), `eileen-rt` (`task::spawn`, `task::spawn_blocking`, `task::dump`, named bounded
channels, `runtime::build`), `eileen-platform` (every trait of section 3.4 with a fake beside
it, and the `machine` module), `eileen-supervisor` (supervised children in kill-on-close Job
Objects, the guard's detached spawn, deadlines, heartbeats, restart backoff, `qos`) and
`eileen-buildguard` (slots, admission, the samples module, the baseline, `spawn_cargo`,
`desktop.lock`). Acceptance 4 (all but the owner's-PC measurements), 5, 6 and 12 are recorded
below; the owner's-PC half of acceptance 4 waits for `wp0/xtask`'s `slots --baseline`.

A second review of the six crates had started when the owner merged #73 so that WP10's drafts
could retarget to `main`; its findings ship as `wp0/base-crates-review`. That pull request
moves the machine root off Windows to the XDG state folder and drops the `HOME` fallback on
Windows, runs every guard wait on the caller's clock, keeps secrets and the per-install key out
of `Debug`, keeps dependencies' absolute source paths out of the logs, logs panic messages and
child stderr as `pii.` fields, bounds the Redactor's literals, makes `ThreadTicket` thread-bound,
removes the overflow panics from the supervisor's backoff and heartbeat, and tightens the
span-parent table, with a test for each.

## Acceptance records

Acceptance 4 (its fake-memory half), 5, 6 and 12 passed in pull request #73's CI run on its
head ecd80d2, run 40162319168: `logic-linux` ran 640 of 640 tests on `ubuntu-latest` and
`windows-test` 311 of 311 on `windows-latest`. The tests, by acceptance clause (crate-relative
paths; `it::` is a crate's integration test target):

- Acceptance 4, in `eileen-buildguard`: slot 0 at 1.3 GB with low paging and 2 jobs, with its
  holder recorded (`it::admission::slot0_is_admitted_at_1_3_gb_with_low_paging_and_two_jobs_and_records_its_holder`);
  refused while paging is high, then exit 75 after `--wait-max`
  (`it::admission::slot0_is_refused_while_paging_is_high_and_exits_75_after_wait_max`); slot 1
  refused below its requirement, also when the free slot is slot 0
  (`it::admission::slot1_is_refused_below_its_requirement`,
  `it::admission::a_second_build_on_slot0_meets_slot1s_requirement_while_slot1_runs`); 2 jobs at
  2.0, 2.4 and 2.5 GB and never fewer as memory rises
  (`admission::tests::jobs_are_two_up_to_two_and_a_half_gigabytes`, the property test
  `admission::tests::jobs_never_fall_as_memory_rises`); a decision from 30 s of fresh shared
  samples (`it::admission::a_command_with_fresh_shared_samples_decides_without_sampling`); a
  sample from another process read back, and two processes writing one paging window leaving one
  line (`it::processes::a_sample_appended_by_another_process_is_read_back_by_recent`,
  `it::processes::two_processes_appending_one_window_leave_one_line`); `baseline()` reading back
  the keys (`it::admission::baseline_reads_back_the_keys_slots_baseline_wrote`); the baseline
  leaving out slot-held samples, with a re-check limit of at least twice slot 0's and a 1 GB
  floor (`baseline::tests::slot_held_samples_are_left_out`,
  `baseline::tests::the_recheck_limit_is_at_least_twice_slot0_and_the_floor_at_least_a_gigabyte`);
  the headroom refusal
  (`it::admission::slot0_is_refused_when_headroom_minus_the_estimate_is_under_the_floor`); the
  bounded re-check (`it::admission::a_recheck_past_its_wait_max_releases_the_slot_and_exits_75`);
  `--kill-stale` (`it::processes::kill_stale_frees_a_dead_holders_slot`); and, on Windows, no
  `rustc` left after xtask is killed (`it::processes::killing_xtask_mid_build_leaves_no_rustc_running`).
- Acceptance 5, in `eileen-supervisor`, on Windows: `tests::killing_the_job_kills_the_grandchild`
  and `tests::the_guard_spawn_outlives_its_parent`. The second test does not yet check that its
  parent sits outside every kill-on-close job, which depends on the Job Object the test runner
  holds each test in; the second review's nit on it stays open for phase B's `user_launch`,
  whose own Windows test needs the same job reading.
- Acceptance 6, in `eileen-trace`: `it::canary::fake_secrets_never_reach_disk_or_a_dump` and
  `it::canary::fake_secrets_never_reach_the_console`, joined in `wp0/base-crates-review` by
  `it::canary::no_home_folder_reaches_disk_or_a_dump`.
- Acceptance 12, in `eileen-platform`, on both runners: `it::machine::the_golden_vector_is_stable`,
  `it::machine::windows_guid_form_and_linux_form_give_one_hash`,
  `it::machine::a_cloned_image_with_two_install_ids_gives_two_hashes`,
  `it::machine::an_unusable_os_id_gives_the_install_id_only_hash` and
  `it::machine::file_identity_reads_etc_then_falls_back_to_dbus`.

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

- `wp0/xtask` wires the build guard `wp0/base-crates` built: `acquire` with
  `SystemMemory::new(&RealClock)`, `Store::machine()`, `disk::free_disk_gb` of the worktree and
  `EILEEN_BUILD_SLOT_HELD` from the environment; the ticket's `env()` on every cargo it starts
  through `spawn_cargo`; `Ticket::recheck` before each invocation and `Ticket::sample` as the
  10 s tick of `CargoChild::wait_ticking`; the reason and holders printed by the `report`
  callback; exit code 75 from `Refusal::exit_code`. `slots` prints `slots::list`, `--kill-stale`
  runs `slots::kill_stale`, `--baseline` samples every 5 s into the store and writes
  `compute_baseline`'s result with `Store::write_baseline` (warning below four hours from
  `baseline::hours_covered`), `--pressure` calls `pressure::pressure`, and `desktop-lock --hold`
  takes `acquire_desktop`.
- `wp0/xtask` also keeps the guard's log that acceptance 4's owner's-PC half reads ("the guard's
  log shows at least one refusal"): `eileen-buildguard` writes none, so xtask appends one line
  per admission and per refusal (the time, the slot or slots, the reason, the job count, and the
  window's paging average, lowest headroom and available memory) to a machine-wide JSON-lines
  file under `samples::machine_root()`, and the acceptance record is taken from it.
- Check 14's list of `#[expect(clippy::disallowed_methods)]` functions after `wp0/base-crates`,
  which differs from section 4b.2's shorter names: `eileen_stdx::clock::SystemClock::now` and
  `::system_now`, `eileen_stdx::ids::OsIds::ulid` (its `Ids` impl),
  `eileen_stdx::thread::spawn_named_with_stack` (which `spawn_named` calls) and
  `eileen_stdx::thread::sleep`, `eileen_rt::task::spawn_raw` and `eileen_rt::task::spawn_blocking`,
  `eileen_supervisor::child::base_command`, and `eileen_buildguard::clock::instant_now`,
  `eileen_buildguard::clock::system_now`, `eileen_buildguard::clock::RealClock::sleep` (its
  `Clock` impl) and `eileen_buildguard::spawn::spawn_cargo`.

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

- Base crates: Windows file locks are mandatory, so no other process can read a record written
  inside a locked file. Each build slot's holder record (PID, worktree, command, start) lives
  beside its lock, `build-slot-0.holder` next to `build-slot-0.lock`, and is removed on release;
  `slots --kill-stale` removes the record of a slot whose lock the OS released when its holder
  died. Section 6.1 rule 1 said the lock file records the holder, so the one-line amendment
  `wp0/holder-records-amendment` (pull request #22, merged 2026-10-21) moved the record beside
  its lock and named `samples::slot_held_now` in the samples interface (below).
- Base crates: the `windows` union of section 4.1 lacked two features process-wrap turns on:
  `Win32_System_IO` from its `job-object` feature, and `std` from its `windows` dependency,
  which keeps `windows`' default features. Without them a build with the supervisor and a build
  without it would have compiled `windows` twice; both joined the union. `windows-sys` 0.61.2
  had the same shape: tokio (`net`, `process`, `signal`), mio and socket2 turn on
  `Wdk_Foundation`, `Wdk_Storage_FileSystem`, `Wdk_System_IO`, `Win32_Networking_WinSock`,
  `Win32_System_IO`, `Win32_System_Pipes`, `Win32_System_SystemServices`,
  `Win32_System_WindowsProgramming` and `Win32_System_Console`, and the test tools (insta's
  console, tempfile, rustix, errno, winapi-util) `Win32_System_Diagnostics_Debug` and
  `Win32_UI_Input_KeyboardAndMouse`. All of them joined the `windows-sys` union, so
  `cargo xtask` (the build guard alone), a `-p` build and the workspace build compile one set.
- Base crates: `tracing` reuses a span's id once it closes, so the flight recorder and the test
  capture give every span a record id of their own, and a remote parent is matched by span id
  among the spans open at the child's start.
- Base crates: in `tracing`'s event macros a dotted field name (`pii.title = ...`) straight after
  `target:` is ambiguous, and a quoted one in that place is read as the message. Put another
  field first.
- Base crates: tokio's `test-util` (paused time) is not in the tokio union, so the supervisor's
  heartbeat and backoff logic takes explicit instants and the deadline test waits 20 ms of real
  time. WP2's concurrency tests will want `test-util`; adding it is a one-line amendment. Since
  pull request #35, `eileen-dbus-linux` turns `test-util` on in its Linux dev-dependencies, so on
  Linux a workspace test build compiles tokio with it and a `-p` build of another crate without
  it; that amendment would make the union say so.
- Base crates: the first lock of the base crates' dependencies took `cc` 1.9.0 and
  `find-msvc-tools` 0.1.15 (2026-10-19, through `blake3`'s build script), `deranged` 0.5.9
  (2026-10-13, through `tracing-appender`'s `time`) and `smallvec` 1.16.3 (2026-10-10), all
  inside the 14-day window. They are held back with `cargo update --precise` to `cc` 1.6.0
  (2026-10-03), `find-msvc-tools` 0.1.14 (2026-09-25), `deranged` 0.5.8 (2026-02-21) and
  `smallvec` 1.16.2 (2026-09-25). `tokio` 1.54.0 (2026-10-19) was young too but was then section
  4.1's pin; it was yanked afterwards; pull request #27 moved the pin to 1.53.2, and pull request
  #59 to 1.53.3 for RUSTSEC-2026-0386, which `main` locks. The other 82 new entries were
  published on or before 2026-10-07.
- Base crates: the Linux lane's first look at this branch came from WP10, not CI: an Ubuntu 24.04
  run over `backup/wp0-base-crates` at `ca336be` (docs/LINUX.md section 10) passed 305 of 305
  tests and found three things the Windows jobs cannot show. A test's `drop` of an `AudioBoost`,
  which holds nothing off Windows, fails `clippy::drop_non_drop` there; the yanked `tokio` 1.54.0
  fails `cargo deny`; and the guard read Linux commit headroom as `CommitLimit` minus
  `Committed_AS`, which the kernel enforces only under strict overcommit, so on a default desktop
  it can read 0 with gigabytes free and refuse every build. All three are fixed on the rebased
  branch.
- Base crates: clippy 1.99 wants `Duration::from_mins` and `from_hours` and the renamed
  `AtomicU64::try_update`, and rustfmt can leave a trailing comma in a macro call it joins onto
  one line, which clippy then refuses.
- Base crates: the review fixes were committed by an agent cut off by a usage limit before it
  had built them; the resume found one parse error, two lints, a rustdoc error and nine
  unformatted files. A fix batch is committed only after the build, clippy and the tests pass.
- Base crates: the squash merge of #73 dropped the shared history WP10's drafts were built on
  (they sat on WP10's merge 3fde1ab of the shared branch), so each draft then conflicted with
  `main` in this file, which none of them touches. WP10 rebases its stack with
  `git rebase --onto origin/main 3fde1ab`, which replays only its own commits; WP0 changes
  nothing on those branches.

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

- Base crates, platform: the handle and value types only the traits use are `eileen-platform`'s, by
  the contracts rule above: `PlatformError`, `Deadline` (a budget, since the crate may depend on
  `eileen-contracts` only), `Subscription`, `ThreadTicket`, `PowerRequest`, `RgbaImage`, the
  element, input, insertion, tray and draw-list values, and `AppOp` (the contracts' Office, Explorer
  and Photoshop ops). `ForegroundOutcome::Taken` carries the `RestoreToken` that `Input::restore`
  takes. An owning package moves a type into the contracts by amendment when it starts crossing a
  process. `ThreadTicket` is not `Send` on any OS, as the supervisor's `AudioBoost` is not, and its
  revert need not be `Send`; the fakes call their subscribers after letting go of their lock, so a
  subscriber may unsubscribe from inside its own callback.
- Base crates, platform: `machine_hash` normalises its OS id itself, so an empty, malformed,
  all-zero or `uninitialized` id gives the install-id-only hash. The normalised id enters the
  hash as its 32 ASCII characters and the install id as its 16 bytes, each after an 8-byte
  little-endian length. The golden vectors were computed with a from-scratch BLAKE3 before the
  code ran. `FileMachineIdentity` treats a missing, non-UTF-8 or unusable file as absent and
  reports any other read error, so a transient failure never changes the hash silently.
- Base crates, stdx: soft assertions panic in debug builds and report in release builds, and
  `eileen_trace::init` switches every product process to reporting (its hook logs
  `soft_assert.failed` and asks for a ring dump); `soft_assert::with_mode` sets a per-thread
  mode, which the simulator and `test_support::capture` use. `buggify!` activates a site for a
  run with probability 1/4 from a hash of the seed and the site, so the choice does not depend on
  which site runs first, and an active site fires with probability 1/4 from the run's
  `CoreRng`. `CoreRng` is xoshiro256** seeded through SplitMix64, with Lemire's unbiased range.
- Base crates, stdx: a fault site is a name written at it, `buggify!("store.write")`, not its
  file, line and column. `file!()` is spelt with backslashes on Windows and slashes on Linux and
  moves with unrelated edits, so one seed would have switched on different sites on the owner's
  PC and on the Linux lane, and a `SimRegression` written on one would not replay on the other.
  The name feeds the activation hash, is what `buggify::fired` reports and is a
  `FaultRecord.site`; a test pins the activation for one seed and name, computed with an
  independent implementation, so both lanes check the same answer.
- Base crates, buildguard: paging is judged as the 30 s average, because the baseline's limits are
  percentiles of 30 s averages; the headroom rule uses the lowest headroom in the window; slot 1
  applies the headroom rule as well as its memory requirement; exclusive work takes both slots under
  slot 0's rules, slot 1's requirement and the 8-job ceiling. Slot 1's rule is the second-build
  rule: it applies whenever another build holds a slot, whichever index is free (slot 0's build
  often ends while slot 1's runs), with the peak measured from the running holder's start; slot 0's
  rule applies only when no slot is held. Admission decides from the slots as they stand and takes a
  lock only when the rules admit, so a waiting command never holds a slot or writes a record between
  its 5 s attempts; a take that loses a race, or a held lock whose record is not written yet,
  refuses for that tick and the next decides again. `slots --kill-stale` reads and removes a stale
  record only while it holds that slot's lock. Every lock the guard takes for a moment (a probe,
  `--kill-stale`, the samples lock) or for a build is unlocked explicitly before its handle closes,
  because Windows releases a lock left on a closed handle only when the system gets to it. Samples
  cover the last 30 s when the oldest in the window is at least 25 s old (one 5 s interval of slack)
  and the newest is under 10 s old. `SystemMemory` primes its paging counter when it opens, and a
  reading taken less than a second after the prime or the previous reading first waits for the rest,
  so a guarded command that samples straight after opening it never adds a rate over a few
  microseconds to the shared file; off Windows it primes from `/proc/vmstat` the same way. It is not
  `Send` on any OS, so code that moves one to another thread fails on Linux as on Windows.
  `samples::slot_held_now` (and `Store::slot_held`) probes both slot locks, so the runtime fills
  `slot_held` from the samples module alone, as section 3.1 requires. The second-build rule's code
  says so (`decide_second_build`, `Reason::SecondBuildMemory` and `Reason::SecondBuildBeforePeak`),
  and a held lock whose record is not written yet refuses as `Reason::HolderUnknown` rather than as
  "before the peak". Every wait runs on the caller's `Clock`, a real reading's one-second minimum
  interval and the samples lock included, so a test on a `ManualClock` waits for nothing real;
  `MemorySource::sample` and `SystemMemory::new` take the clock, and `Store::append` and
  `append_paging` take it for the lock wait, while the module functions the runtime calls pass the
  real one. One constant per quantity: `samples::DEFAULT_PAGING_LIMIT_KB_S` is admission's slot-0
  default, the `auto` limit's 24-hour rule is `samples::AUTO_LIMIT_COVERAGE`, and the baseline
  groups by `admission::WINDOW_MS`. Percentiles are nearest-rank; "plus 25 percent" rounds to the
  nearest KB/s. `EILEEN_BUILD_SLOT_HELD` is `<slots>:<jobs>:<holder pid>` and counts only while that
  holder's record names the PID and its lock is held, so a stale variable inherited by a shell
  cannot bypass the guard. Low disk refuses at once with exit 75. `procs::process_alive` waits on
  the process with a zero timeout rather than reading its exit code, because a process may exit with
  259, which is `STILL_ACTIVE`. The machine root comes from `LOCALAPPDATA` alone on Windows: a
  process whose environment lacks it gets an error rather than a second, always-free set of slot
  locks under `HOME`. Off Windows, when it is unset, the root is `$XDG_STATE_HOME/eileen`, else
  `$HOME/.local/state/eileen`, beside WP10's `machine/` files (docs/LINUX.md section 5), and the
  sampler reads `/proc`: `MemAvailable`, major faults times 4 KB, and commit headroom by
  `vm.overcommit_memory` as `eileen-shell-linux` reads it (`CommitLimit` minus `Committed_AS` under
  strict overcommit, else `MemAvailable` plus `SwapFree`), so the guard's samples and the runtime's
  agree.
- Base crates, buildguard: `spawn_cargo` creates cargo suspended, puts it in a fresh
  kill-on-close Job Object, then resumes its threads (process-wrap's technique), so nothing it
  starts escapes the job; the guard's process holds the only handle. Its process tests run the
  test binary again as stand-ins for xtask, cargo and rustc, which block on a lock the test holds
  instead of sleeping. The stand-in cargo starts its stand-in rustc with
  `CargoInvocation::in_caller_job`, a plain child with no job of its own, as real cargo starts
  rustc, so killing xtask proves that xtask's job takes cargo's ordinary children. Nothing in the
  process tests sleeps or polls (check 16): a helper says `ready ...` on a piped stdout that the
  test reads, exits are waited on through process handles (`procs::Watched`, Windows), and a dead
  holder's lock release is waited on with a blocking lock.
- Base crates, trace: the JSON file layer is Eileen's own rather than `fmt`'s JSON, because
  `fmt` formats an event's fields itself and the `pii.` policy could not apply to them; the line
  keeps `fmt`'s field names (`timestamp`, `level`, `target`, `filename`, `line_number`,
  `threadName`, `fields`, `span`, `spans`) and adds `process`. A span's `traceparent` field
  records a parent in another process. The Redactor's card rule takes 15, 16 or 19 digits
  starting with 2 to 6 that pass the Luhn check (so timestamps and other IDs are left alone),
  and replaces a card standing as a bare JSON number with `0`, because a run of zeros is not
  valid JSON. A literal secret is registered with its JSON-escaped, `Debug`-escaped and `Debug`
  then JSON-escaped forms, the file layer redacts each field value before it serializes the line
  (the line pass stays as a backstop), and the console layer writes through the Redactor;
  the canary covers a password with a quote, a backslash and a tab, the console, and checks that
  every file and dump line is JSON. `ring::parse_lines` counts the lines it skips. The `pii.`
  key is `Option`al and `TraceConfig::new` leaves it out: with none, `pii.` fields keep their
  length alone, never a hash under a constant key, and the runtime passes the per-install key it
  derives. The PII scan accepts the reserved example domains and generic user folders
  (`Public`, `Default`, the CI runner's); a scenario's text files are capped at 256 KiB,
  consented audio at 5 MiB and the folder at 8 MiB.
- Base crates, trace, second review: a literal secret stays registered while the `LiteralGuard` that
  `Redactor::add_literal` returns lives (the guard holds the redactor weakly), at most 64 at once
  (`MAX_LITERALS`, refusals counted), with each form counted so two registrations of one secret need
  two drops; a secret under 6 bytes is refused as an error rather than ignored. The `Debug` of a
  `Redactor` shows how many literals it holds, and the per-install key is a `PiiKey` whose `Debug`
  is `PiiKey(..)`. A log line's `filename`, and a ring record's default event name
  (`event <file>:<line>`), keep a workspace or standard-library path as it is and cut an absolute
  one to the part from its crate's `<name>-<version>` folder in Cargo's registry (else to its file
  name), so a dependency built on the owner's PC never writes the account name into a log or an
  incident. The span-parent table follows the observability report's section 2.5: `speech.segment`,
  `turn.detect`, `tts.synth`, `desktop.capture`, `desktop.action` and `apps.call` take only their
  remote parent, so a lost `traceparent` fails the check; `speak` alone keeps `Root`, for an
  announcement with neither turn nor job. `test_support` sits behind the `test-support` feature,
  which only dev-dependencies turn on. The PII scan reports each kind once per line, in plain words.
- Base crates, trace: the scenario privacy rules parse `scenario.toml` into the contracts'
  `Scenario` (so `eileen-trace` depends on `toml`). Audio passes only when the origin is
  `contributed-consented-audio`, `intent.audio.file` names it and the record at
  `intent.audio.consent` is in the folder. "Only placeholders in personal fields" is checked
  here, for every origin as section 4.8 states: a walk of `scenario.toml` and of every replay
  input line alongside the contracts' generated schema finds each `Personal` position (the
  `x-eileen-text` extension), and its text must be built only from the placeholders. A
  `scenario.toml` or replay line that does not parse fails as unreadable; problems name the
  file, the line and a JSON pointer, never the text.
- Base crates, rt: a spawned task also carries the caller's subscriber (and a blocking task its
  dispatcher), so a scoped subscriber in a test or a tool sees what the task logs. A panic
  message is logged whole as `pii.message`, which the files reduce to its length and keyed hash
  while the ring and the console keep it; the first cut replaced backtick-quoted text instead,
  which left an unwrapped error's `Debug`, an `expect` message and an assertion's values in the
  files. A blocking task is listed in `task::dump` from when a pool thread takes it, so a queued
  task never reads as blocking and a call outside a runtime leaves no entry.
- Base crates, supervisor: it uses `windows` rather than `windows-sys`, because process-wrap's
  `CreationFlags` takes `windows`' flag type; the root manifest's comment on `windows-sys` now names
  only the build guard and the dependencies whose features it lists. Children are of class
  `Sidecar`, `ClaudeSession` (below normal priority) or `Tool`; stderr lines are forwarded at INFO
  as `pii.line` (a Claude session's stderr quotes paths and prompt text), cut at 4 KB; restarts
  default to 250 ms doubling to 30 s, at most 5 in 10 minutes, reset after 2 stable minutes; the
  first delay never exceeds the maximum, doubling saturates, and a heartbeat whose deadline lies
  past the clock's range has none, so no policy value panics. A `ChildSpec`'s `Debug` lists its
  environment's names, never the values. The detached spawn falls back to a plain spawn only on
  access denied, and a supervised child's own detached spawn cannot leave its job (the test checks
  `broke_away` is false there). Stderr is read as bytes with at most 4 KB of a line kept and decoded
  lossily, so a tool writing in the ANSI code page or a megabyte with no newline neither stops the
  forwarding nor grows memory. `AudioBoost` is not `Send` on any OS. The off-Windows bodies of
  `opt_out_of_ecoqos` and `boost_audio_thread` are constants, so they carry a `cfg_attr` `expect` of
  `missing_const_for_fn` and `unnecessary_wraps` there; `free_disk_gb` keeps one body and splits the
  platform into a helper.

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

Locally for `wp0/base-crates` on 2026-10-21 (the owner's PC with 2.8 to 4.5 GB free, 2 to 4
jobs, below-normal priority): the warm `cargo nextest run --workspace --profile ci` ran 288 tests
in 4.7 to 7.0 s, the process tests included; clippy with `-D warnings`, the doc tests, `cargo doc`
with `-D warnings`, cargo-deny and cargo-machete passed on Windows. The Linux lane is CI's.
After the review fixes, on the same PC with 2.5 to 3.1 GB free and 2 jobs: 310 tests in 5.5 s
warm, and the same checks pass. This host has no Linux target, so the off-Windows code of the
build guard and of the supervisor's `qos` module was linted by compiling it here with
`cfg(windows)` and `cfg(not(windows))` swapped, under the workspace's lint table and
`clippy.toml`; the supervisor's `cfg(unix)` process code uses Unix-only APIs and waits for
`logic-linux`.

## Outcome

Open.
