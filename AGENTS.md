# Eileen: guide for contributors and coding agents

Eileen is a voice-first AI layer for Windows 11, with Linux following. The always-on core is Rust,
and models run through each vendor's own signed-in CLI. Every coding tool reads this file, and
`CLAUDE.md` imports it.

## Read first

- [ARCHITECTURE.md](ARCHITECTURE.md): the map, the processes, the invariants and the boundaries.
- [docs/OWNER-DIRECTION.md](docs/OWNER-DIRECTION.md): what the owner asked for. Binding.
- [docs/BUILD.md](docs/BUILD.md) section 0, then your work package in section 6 and every section
  it names, and its living file in [docs/wp/](docs/wp/).
- The crate doc of the crate you change, first: the `//!` comment at the top of its `src/lib.rs`
  says what it owns, its invariants, boundaries, tests, traps and logs. A nested `AGENTS.md`
  exists only where a folder's rules differ: `crates/eileen-desktop-win/`,
  `crates/eileen-apps-win/`, `crates/eileen-speech/`, `fuzz/` and `scenarios/`.
- `docs/CODEMAP.md` (generated from WP0 phase B): the crate graph and the event map.

## Commands

Build only through `cargo xtask`. Each command that runs cargo first takes a build slot by
measured memory, and exits with code 75 when none comes within `--wait-max`: wait and retry.

| Task | Command |
|---|---|
| Set up a machine once | `cargo xtask setup` |
| Check one crate | `cargo xtask check -p <crate>` |
| Test one crate | `cargo xtask test -p <crate>` (nextest; add a test name to filter) |
| Lint one crate | `cargo xtask clippy -p <crate>` |
| Snapshot tests | `cargo xtask insta` (`--review` to accept new snapshots) |
| Simulator, pull request seeds | `cargo xtask sim` (one run: `--seed <n> --profile <name>`) |
| Fast repository checks | `cargo xtask lint-repo` |
| Check a pull request description | `cargo xtask pr-meta --body-file <file>` |
| Before pushing (the git hook runs it) | `cargo xtask pre-push` |
| What CI's pull request lane runs | `cargo xtask ci` |
| Build slots, holders, stale records | `cargo xtask slots` (`--kill-stale`) |
| Windows native libraries | `cargo xtask fetch-native` |
| After a lockfile merge conflict | take `main`'s `Cargo.lock`, then `cargo xtask lock` |
| Hold the desktop for manual work | `cargo xtask desktop-lock --hold` |

`codegen`, `evidence` and `scenarios` arrive in WP0 phase B and say "not built yet" until then.

## Rules that do not bend

1. Never read, copy, print or log vendor credential files, and never run a vendor login flow. The
   user signs in to each CLI themselves.
2. The product is Eileen. Workers are named by route: Claude, Codex, Grok, DeepSeek. No film or
   trademark names, no earlier working names, no vendor product name inside an Eileen name.
3. No API keys except DeepSeek's, and never in the repository.
4. No `unwrap`, `expect`, `panic!` or `todo!` outside tests. Libraries return typed error enums.
5. `unsafe` only in a crate's private `ffi` module: one unsafe operation per block, each with a
   `// SAFETY:` comment saying which precondition holds and why.
6. Children start through `eileen_supervisor`, tasks through `eileen_rt::task::spawn`, threads
   through `eileen_stdx::thread::spawn_named`; clippy's `disallowed-methods` enforces it.
7. Every wait on another process has a deadline. Every queue and channel has a maximum.
8. Screen text, transcripts, model output, tool results and command lines are untrusted. They
   never approve an action, raise a permission tier or become a skill without the user.
9. Core logic never reads the clock or OS randomness; both are passed in. A map whose order can
   reach an effect or a snapshot is a `BTreeMap` or an `IndexMap`.
10. Only `eileen-contracts` types cross a process, pipe, file or socket. Only `eileen-mcp` knows
    rmcp; only `eileen-harness` knows vendor CLI flags.
11. Native libraries appear only under the Windows target tables BUILD.md section 4.1 allows, and
    the `-win` crates are `#![cfg(windows)]`, so the neutral crates build and test on Linux.
12. Docs, comments and messages are plain and editorial: no em or en dashes, no marketing words.

## On the owner's PC (16 GB, shared with other agents)

- Never call cargo directly to build, test or lint; `.claude/settings.json` denies it. The guard
  runs cargo in a kill-on-close Job Object at below-normal priority and scales the job count to
  free memory.
- Run guarded builds and test runs in the background and wait for them; a cold build can outlast
  a 10-minute foreground tool call. Never leave one orphaned.
- Exclusive work takes both slots (`--exclusive`): release builds (the owner's), anything that loads speech
  models, soaks, benchmarks, tests with Resolve or Premiere running, and more than one real
  Claude session. No local fat-LTO builds, no `cargo install` from source, no `cargo clean`.
- Desktop tests take `desktop.lock` through `cargo xtask` and touch only windows and devices the
  test created or the owner agreed to. Two desktop packages never run at once.
- Inner loop on `-p <crate>`; build only the binary you need. No language server during build
  work: its background `cargo check` compiles outside the slots.
- Do not start a long build, simulation, soak or benchmark at the end of a session, and stop every
  process you started before you finish.
- A target folder per worktree, deleted when its branch merges. Keep every open pull request up to
  date with `main` (merge `main` in); branch protection requires it.

## Making a change

1. Find the crate that owns the data and read its crate doc.
2. Evidence first: write the test that shows the problem at the narrowest layer and see it fail.
3. Make the smallest change that fits the design. A new `pub` item used across crates, a new
   dependency or a new crate is an architectural change: say so in the pull request.
4. Run the crate's tests, then `cargo xtask pre-push`.
5. One topic per branch (`wp<n>/<topic>`) and pull request, logic changes under about 500 lines,
   a draft pull request from the first push.
6. The description follows `.github/PULL_REQUEST_TEMPLATE.md`: `## Evidence` first, then
   `## Type` with one line `type: <value>` (feature, fix, refactor, docs, scenario, amendment or
   deps), then the template's other headings. Evidence is prose plus at most one each of the
   field lines `tests:`, `scenarios:`, `numbers:`, `replay:`, `incident:`, `owner-run:` and
   `override:`. Every test listed exists at the head, as `<package>::<module>::<fn>` (an
   integration test as `<package>::it::<module>::<fn>`). A feature, fix or refactor lists at
   least one test or scenario, and a fix adds a regression. The title is an imperative squash
   subject under 72 characters. `cargo xtask pr-meta` checks all of it.
7. Merge only when `all-green` and `pr-meta` pass; merges are squashed.
8. A change to `docs/BUILD.md`, `docs/DECISIONS.md` or `docs/OWNER-DIRECTION.md`, to another
   package's merged files, or to the root manifest's feature lists is a separate `amendment` pull
   request that touches only that, and the owner reviews it. A change to `docs/DECISIONS.md`
   also updates `docs/PLAYBOOK.md`, or says under "Playbook" why it is unaffected.
9. Keep your `docs/wp/` file current. If a crate doc or ARCHITECTURE.md is no longer true, fix it
   in the same pull request.

## Testing

- Unit tests sit beside the code in `#[cfg(test)]` modules; each crate has one integration test
  binary, `tests/it/main.rs`, whose modules are `#[cfg(test)]`.
- Core behaviour (jobs, the bus, scheduling, barge-in, crashes, approvals) is tested in
  `eileen-sim` as an invariant or a scenario, never as a test that sleeps.
- Snapshots use insta. Never edit a `.snap` file by hand; regenerate it and read the diff.
- Every parser of untrusted input has a fuzz target, a `fuzz_entry` function and a seed corpus. A
  fuzz or simulator failure becomes a regression file before its fix counts.
- No `#[ignore]`, no `#[should_panic]`, no real-time sleeps (use paused tokio time or a manual
  clock). If behaviour is wrong today, assert the wrong behaviour and add `FIXME(#issue)`.
- Tests that need the owner's PC or real Claude live only in `owner_pc::` and `real_claude::`
  modules, which the default profiles skip; `cargo xtask test --owner` runs them.
- `main` is green. Never assume a failure was already there.

## Code review rules

- Check the change against the crate's invariants and these rules.
- Flag unbounded queues, cross-process waits without a deadline, new cross-crate `pub` items or
  dependencies, tests at the wrong layer or missing, unexplained snapshot changes, untrusted text
  reaching a permission decision, the clock or randomness in core logic, a vendor credential
  path, `unsafe` outside `ffi`, and an `#[expect(clippy::disallowed_methods)]` outside the
  exceptions BUILD.md section 4b.2 lists.
- Every claim in a review cites a file and line, or a command that shows it.
- Write comments, docs and pull request text for someone who never saw your session: say why,
  not what, and never mention conversations, prompts or abandoned attempts.

## The checks

CI runs fmt, clippy with warnings denied on Windows and Linux, nextest on both, doc tests, docs
with warnings denied, cargo-deny, cargo-machete and `pr-meta`; `all-green` must pass. `cargo xtask
lint-repo` runs checks 1, 2, 4, 6, 10, 14 and 16: crate docs, guidance files, the xtask commands
they name, house style, where `unsafe` lives, the credential and spawn boundary, and the test
rules. All thirty checks are in BUILD.md section 4b.3; WP0 phase B generates the registry into
`docs/reference/checks.md` and CONTRIBUTING.md.

## Changing guidance files

Do not edit guidance files during feature work. Put proposed text under "Suggested guidance
changes" in the pull request. A rule is added only if it is non-obvious, came up more than once and
is specific enough to act on. A new nested `AGENTS.md` needs a line saying why the folder differs.
