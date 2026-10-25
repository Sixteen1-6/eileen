# Engineering practice: how the best Rust projects are organised, documented and tested, and what Eileen copies

Research pass, written 2026-10-20. Question: how do the best-engineered open-source projects lay out their code, write their guidance for people and coding agents, test long-running concurrent software, and split fast pull request checks from slow suites, and exactly what should Eileen copy?

This is for whoever writes `docs/BUILD.md` and WP0, and for every engineer agent after that. It serves the 2026-10-20 owner direction entry "Engineered extremely well from the ground up" in [OWNER-DIRECTION.md](../OWNER-DIRECTION.md). It builds on [2026-10-rust-architecture.md](2026-10-rust-architecture.md), which already settled the workspace lints, clippy pedantic, cargo-deny, cargo-nextest, `cargo xtask ci`, the build guard and CI on `windows-latest`; this report does not repeat those. Where it proposes a change to [ARCHITECTURE.md](../../ARCHITECTURE.md) or [DECISIONS.md](../DECISIONS.md), that change needs a decision record.

## How to read this

- **Nothing was built or run for this report.** The PC had about 2 GB of free memory, so the work was reading.
- **[read]**: read from the project's own files on GitHub, its documentation or its registry entry on 2026-10-20. Downloaded copies are in `%USERPROFILE%\AppData\Local\Temp\claude\C--Users-USER-Projects--claude\8b2a7b12-9d74-4cce-a7ac-9d5f3ba7299c\scratchpad\eng\raw\`.
- **[secondary]**: from a search result or third-party write-up, not the project itself.
- **[prior]**: carried from an earlier Eileen report, which says how it was measured.
- **[inferred]**: design reasoning that has not been checked against anything. Verify before relying on it.

## 1. Verdict

1. **Write the core as a deterministic state machine and test it with Eileen's own simulator.** The best-tested systems (TigerBeetle's VOPR, Turso's simulator, FoundationDB) stub out every source of nondeterminism and replay any failure from a seed and a commit [read]. For Eileen that means: the bus, job registry, scheduler, turn-taking and barge-in, and supervisor policy sit behind one closed `Input` enum and one `Effect` enum, with time, randomness and child processes injected; a new dev-only crate `eileen-sim` drives them on one thread, injects faults at `buggify!` points, checks invariants after every step and prints the replay command on failure (section 3.3). The ready-made simulation runtimes do not fit as the main tool: madsim needs `[patch.crates-io]` git overrides of crates such as `getrandom`, which `deny.toml` forbids, and turmoil simulates TCP and UDP hosts, while Eileen's processes talk over stdio and named pipes [read, inferred].
2. **Seeds on pull requests are fixed; fresh random seeds run on `main` and nightly.** Turso's CI states the rule plainly: a fixed seed set makes a green PR stay green and a red one reproduce exactly, while new seeds are a lead to investigate, not a gate on an unrelated change [read]. A nightly failure is minimised and stored as a decoded input script, not only a seed, because seeds change meaning when the code changes (Turso warns about exactly this for its yield points) [read].
3. **Generate simulation scenarios from bytes.** If the scenario generator reads from an `arbitrary::Unstructured` buffer (matklad's "finite random number generator"), the same code is a seeded random test, a coverage-guided cargo-fuzz target and a minimiser [read, inferred].
4. **One event log format for everything.** The flight recorder, the simulator trace, the end-to-end expectations, the session replay and the learning loop's job record are the same newline-delimited event stream defined in `eileen-contracts`. Recording a session then gives a bug reproduction, a test and a learning example at once [inferred].
5. **End-to-end tests inject recorded WAV files at the audio boundary**, not through a virtual microphone driver, and use a stub harness that replays recorded stream-json on CI. Real Claude and real pro apps run only in a local lane on the owner's PC. The owner's PC must never be registered as a self-hosted runner for this public repository: GitHub's own documentation warns that a fork's pull request can run code on such a machine [read, secondary].
6. **The owner's benchmark requests are timed scenarios compared A/B**, base binary against head binary on the same machine, with a non-parametric test (Firecracker's method), and every result is appended as one JSON line to a results store (TigerBeetle's devhub). Fixed time thresholds on a loaded 16 GB PC would only produce noise [read, prior].
7. **Every parser of untrusted input gets a fuzz target** (16 targets in section 3.7). cargo-fuzz runs only on Unix-like systems, so fuzzing runs on Linux runners against platform-neutral parser crates, and each target's corpus is replayed as an ordinary test on Windows [read].
8. **Agent guidance: `AGENTS.md` is the canonical text, at the root and in every crate, and every `CLAUDE.md` imports it with `@AGENTS.md`.** Claude Code expands `@` imports; Codex has no import syntax and reads only `AGENTS.md`; committed symlinks turn into one-line text files on Windows checkouts [read]. So the owner gets a `CLAUDE.md` everywhere he asked for one, and other tools get the same words. Files stay short and hold invariants, traps, test commands and log locations; the map lives in `ARCHITECTURE.md` and a generated `docs/CODEMAP.md`; `cargo xtask tidy` keeps all of it honest (section 4).
9. **A new, short `ARCHITECTURE.md` at the repository root in rust-analyzer's shape** (bird's eye view, entry points, code map with invariants and API boundaries, cross-cutting concerns). The existing 34 KB `docs/ARCHITECTURE.md` is a Python-era design specification; move it to `docs/design/` (section 5).
10. **CI: a plan job, Linux jobs for everything platform-neutral, Windows clippy and Windows tests in parallel, and one aggregate required check.** Actions pinned by commit SHA, zizmor on the workflows, caches saved only from `main`, the Windows target directory on the runner's `D:` drive. Target wall time under 15 minutes with a warm cache [read, inferred]. GitHub's merge queue is only available to organisation-owned repositories and `Sixteen1-6/eileen` is user-owned, so the owner has a decision to make (section 6.8) [read].
11. **The style guide (section 7) adapts TigerStyle**: assertions on both the expected and the excluded space, limits on every queue and wait, time and randomness injected, no detached tasks, units last in names, comments that say why.

## 2. What the best-run repositories do

### 2.1 Project by project

| Project | What is notable (read 2026-10-20) | What Eileen copies |
|---|---|---|
| [rust-analyzer](https://github.com/rust-lang/rust-analyzer) | `docs/book/src/contributing/architecture.md` (moved from `docs/dev/`): bird's eye view, entry points, a code map with bold "Architecture Invariant" and "API Boundary" callouts, then cross-cutting concerns (testing, error handling, observability, configurability, serialization). Tests concentrate on three system boundaries, are data driven through one `check` helper, use `expect-test` snapshots and no external resources; formatting and tidy checks are ordinary `#[test]`s; heavy tests run only with `RUN_SLOW_TESTS`. `xtask` holds codegen, dist, release, metrics and tidy. CI ends in a `conclusion` job that fails unless every needed job succeeded or was skipped. A 60-line `CLAUDE.md` (with `AGENTS.md` a symlink to it) points to the guides, states key invariants and says to start with the narrowest test. Swatinem/rust-cache was disabled on 2025-04-01 because it did not work with merge groups | The `ARCHITECTURE.md` shape; tidy checks as tests; `check`-style data-driven tests; the aggregate job; slow tests behind an environment variable; a short root guide that points elsewhere |
| [matklad's essays](https://matklad.github.io) | *ARCHITECTURE.md* (2021): short, only things that rarely change, name files and types but do not link them, call out invariants that are expressed as an absence, revisit twice a year. *How to Test* (2021): keep code sans-IO, test features not code, data-driven tests, never spawn work a test cannot wait for. *Delete Cargo Integration Tests*: one integration test binary per crate, because each file in `tests/` is linked separately. *Large Rust Workspaces*: flat `crates/*`. *Catch Flakes On Main* (2026-05): keep running the full suite on `main` and keep a list of its failures, which are flakes by definition. *Finding Bugs* (2026-09): give every system an oracle; a bug that dodged the fuzzer is first a bug in the fuzzer. *Test Case Minimization* (2026-04): a finite random number generator over a byte buffer makes minimisation a search over bytes. *Benchmark In Milliseconds* (2026-10): size micro benchmarks to about 300 ms | All of these, directly |
| [TigerBeetle](https://github.com/tigerbeetle/tigerbeetle) | `docs/TIGER_STYLE.md`: assertions are a force multiplier for fuzzing, at least two per function on average, paired across code paths, on both positive and negative space, split rather than compound, compile-time where possible; put a limit on everything; 70-line functions; units last in names; always say why. VOPR (`docs/internals/vopr.md`): every nondeterministic part stubbed, faults injected from a seed, replay from seed plus commit, specific cases written on the same infrastructure. `src/scripts/devhub.zig`: on every merge to `main`, macro benchmarks run and one JSON object per run is appended to a file in a separate git repository that a dashboard reads. One CI entry point (`zig build ci`). The GitHub `ci.yml` was retired on 2026-07-08 (commit `1116a844`); only release workflows remain on GitHub Actions | The assertion discipline and limits (section 7); seed plus commit replay; the NDJSON results store; one command that CI and people both run |
| [FoundationDB](https://apple.github.io/foundationdb/testing.html) | Whole cluster simulated in one thread; tens of thousands of simulations every night. `flow/include/flow/Buggify.h`: each `BUGGIFY` site is activated for a whole run with probability 0.25 and, once active, fires with probability 0.25, only when simulation enables it | `buggify!` with per-run activation, which gives swarm testing for free |
| [Turso](https://github.com/tursodatabase/turso) | `testing/simulator`: random workload profiles (JSON, with a generated JSON Schema), interaction plans of queries plus properties with assertions, `--seed`, `--doublecheck` (run twice and compare), a bug base of stored plans, differential mode against SQLite, Miri. `testing/concurrent-simulator` (Whopper): CI runs ten seeds at 100,000 steps per profile. Yield injection: test-only yield and failure points in resumable state machines, identified by source-order ordinals that must never be reordered because stored plans would silently retarget. CI: fixed seeds on PRs, random seeds only on `main`; shuttle tests; the Windows simulator job is commented out, beside a note about runs blowing the stack. `AGENTS.md`: every change must have a test that was run failing and passing; every minimised differential-fuzzer failure becomes a permanent regression before the fix counts; plain words over invented terms. `docs/agent-guides/*.md` carry skill front matter | Most of the simulator ergonomics; the fixed-seed PR rule; the permanent-regression rule; a larger stack for the simulator on Windows |
| [tokio](https://github.com/tokio-rs/tokio) and [loom](https://github.com/tokio-rs/loom) | loom permutes executions under the C11 memory model. tokio's `loom.yml` runs every loom suite on `master` and on PRs only when a label such as `R-loom-sync` is set, with `LOOM_MAX_PREEMPTIONS=2`. The runtime builder has `start_paused` (stable, `test-util`) and `rng_seed` (only with `--cfg tokio_unstable`), which makes `select!` branch order deterministic for a fixed tokio and compiler version | loom for the audio ring buffer, label-gated on PRs; paused-time tests for the tokio drivers |
| [shuttle](https://github.com/awslabs/shuttle) | Randomised scheduler with probabilistic guarantees (PCT), deterministic replay of a failing schedule; scales to larger tests than loom but proves nothing. Wrapper crates replace dependencies under a `shuttle` feature; `shuttle-tokio` 1.0.0 (2026-09-04) replaces tokio | Randomised schedule tests of the tokio glue (section 3.4) |
| [turmoil](https://github.com/tokio-rs/turmoil) | Many hosts in one thread with a simulated network and filesystem, seeded hardship (latency, drops, partitions, crashes, torn writes); 0.7.2 | Later, for network connectors (Home Assistant WebSocket, phone bridge); not v0.1 |
| [madsim](https://github.com/madsim-rs/madsim) | A tokio-like runtime with deterministic simulation, enabled by `--cfg madsim`; needs git patches of `getrandom`, `quanta` and others; last release 0.2.34 (2025-10-11) | Not used |
| [uv](https://github.com/astral-sh/uv) | `AGENTS.md` is 46 lines of terse ALWAYS, NEVER and PREFER rules (match nearby tests, prefer snapshots, never release builds unless asked, `cargo update --precise` only, write for readers who have not seen the conversation). `STYLE.md` covers user-facing text. CI: a `plan` job, one reusable workflow per check, a `required-checks-passed` aggregator, zizmor on the workflows, automated security and code-quality review jobs; Windows tests split by hash into three partitions with 15-minute timeouts on 16-core larger runners, pending insta snapshots uploaded as artifacts; `setup-dev-drive.ps1` notes that standard runners' `D:` drive performs as well as or better than a ReFS Dev Drive | The plan job and aggregator; `D:` for build output on Windows; zizmor; the tone of `AGENTS.md` |
| [ruff](https://github.com/astral-sh/ruff) | `CLAUDE.md` is one line, `@AGENTS.md`. `AGENTS.md` (230 lines) has review rules, threat models, "write for human readers", test commands. `.claude/settings.json` registers `.agents/` as a local plugin marketplace for its skills and runs a SessionStart hook. CI: `determine_changes` from a git diff; `cargo insta test --unreferenced reject --test-runner nextest`; `cargo fuzz build` on every PR, parser fuzzing when the parser changes, and a daily fuzz cron that writes a GitHub issue on failure; `cargo hawk` (Astral, created 2026-05, experimental) reports `pub` items that could be private across the workspace | `@AGENTS.md`; rejecting orphaned snapshots; fuzz build on PR and a daily fuzz issue; hawk as an advisory report |
| [Zed](https://github.com/zed-industries/zed) | `AGENTS.md` and `CLAUDE.md` are symlinks to `.rules`. Its "rules hygiene" section: rules are traps to avoid, not maps to follow, because architecture descriptions go stale and agents can read the code; a new rule must be non-obvious, seen more than once and specific; crate-only rules go in that crate; agents propose additions in the PR description instead of editing rules during feature work. Rust rules: no `unwrap`, never `let _ =` on fallible calls, no `mod.rs`, full-word names, tasks must be awaited, detached or stored | Rules hygiene, verbatim in spirit (section 4.9) |
| [codex](https://github.com/openai/codex) | Two per-directory `AGENTS.md` files remain, each a scope and review contract for one subsystem (`exec-server/src/sandbox_integrity`, `tui/src/bottom_pane`). The 320-line root `AGENTS.md` and `.codex/skills/` were deleted on 2026-09-30 (#49713) with no reason in the commit; its `GitOrigin-RevId` line suggests the public repository is exported from an internal one [inferred]. The deleted file had: modules under 500 lines, new modules past 800; changes under 800 lines (500 for complex logic); every UI change carries an insta snapshot; integration tests against a mock model server built from `ev_*` event constructors; a "Code Review Rules" section; divan for benchmarks | Per-directory contracts where a subsystem has one; module and change size limits; a stub harness built from event constructors |
| [Deno](https://github.com/denoland/deno) | `CLAUDE.md` as a full development guide; "spec" tests: one directory per test with a `__test__.jsonc` validated by a JSON Schema, expected output with `[WILDCARD]`, `[WILDLINE]` and `[UNORDERED_START]` markers | Scenario directories with a schema and a wildcard matcher for the end-to-end harness |
| [Biome](https://github.com/biomejs/biome) | `AGENTS.md`: no claim about the code without a file and line or a command; bug fixes need a failing case; review the finished change with a fresh subagent given only the scope and requirements; a skills catalogue with trigger boundaries and a 500-line cap per skill | The evidence rule and the fresh reviewer |
| [nushell](https://github.com/nushell/nushell) | Short `AGENTS.md`: exact semver workspace dependencies, no git dependencies, wait for one `cargo` command to finish before starting another, run the CI clippy alias before handing off | Exact versions; one cargo at a time |
| [Helix](https://github.com/helix-editor/helix) | `docs/architecture.md` opens with a crate table, then explains the core data structures | A crate table at the top of the code map |
| [Firecracker](https://github.com/firecracker-microvm/firecracker) | `tests/README.md`: performance tests emit data series and assert nothing; `tools/ab_test.py` runs each test against an A and a B binary and applies a non-parametric statistical test per series; functional A/B tests (for example, a security audit that fails only if the PR adds an advisory); performance A/B runs after merge, outside the PR pipeline | A/B timing of the benchmark scenarios |
| [wezterm](https://github.com/wezterm/wezterm) | Workflows generated by `ci/generate-workflows.py`; a weekly job runs `cargo cooldown update` with a 14-day minimum publish age (`cooldown.toml`) and opens a PR only if `Cargo.lock` changed | A cooling-off period for new crate versions |

### 2.2 Architecture documents

rust-analyzer's document and matklad's essay agree on the essentials [read]:

- **Short, and only what changes rarely.** The cost of reading it is paid by every recurring contributor, and the shorter it is the less it rots. It is revisited a few times a year, not kept in sync with every change.
- **Bird's eye view first**: what goes in, what comes out, what state is kept.
- **A code map answers "where is the thing that does X" and "what does the thing I am looking at do"**: one short entry per crate, naming key files and types without linking them (links go stale; symbol search does not).
- **Invariants are called out explicitly, especially those that are an absence** ("nothing in `ide` is serializable", "only the binary crate knows the protocol"), because an absence cannot be learned by reading code.
- **API boundaries are marked**, since rules at a boundary differ from rules inside it.
- **Cross-cutting concerns get their own section**: testing, error handling, cancellation, observability, configuration, serialization, code generation.

Helix opens with a crate table [read]. TigerBeetle's `docs/ARCHITECTURE.md` is the opposite pole, a long essay on design decisions (determinism, static allocation, single thread); Eileen has `DECISIONS.md` for that role.

### 2.3 Agent guidance files in 2026

**How the tools load them** [read, Claude Code memory docs and the Codex AGENTS.md guide, 2026-10-20]:

| | Claude Code (owner has 2.1.304 [prior]) | Codex |
|---|---|---|
| Root and ancestors | `CLAUDE.md`, `.claude/CLAUDE.md`, `CLAUDE.local.md` from the working directory and every directory above, concatenated, at launch | Global `~/.codex/AGENTS.md`, then one file per directory from the project root down to the working directory, at start |
| Subdirectories | A subdirectory's `CLAUDE.md` loads when Claude reads, writes or edits a file in it | Not loaded unless the working directory is inside it |
| Imports | `@path`, relative to the importing file, up to four hops, expanded at launch | None documented |
| `AGENTS.md` | Read directly since 2.1.277, by default only when no `CLAUDE.md` exists in the working directory or above; a `CLAUDE.md` with `@AGENTS.md` is never read twice | The native file; `AGENTS.override.md` replaces it per directory; extra names via `project_doc_fallback_filenames` in the user's own config |
| Size | Under 200 lines per file recommended | `project_doc_max_bytes`, 32 KiB combined by default |
| Path-scoped rules | `.claude/rules/*.md` with a `paths:` list of globs, loaded when a matching file is touched | None |
| Symlinks | Work, but on Windows Git checks a committed symlink out as a one-line text file unless `core.symlinks` is on, so the docs recommend the `@AGENTS.md` import there | n/a |
| Review | `/doctor prompt-audit` (2.1.283 and later) reports stale or conflicting instructions | A `## Code Review Rules` section in the nearest `AGENTS.md` is used by Codex code review on GitHub |

AGENTS.md is now stewarded by the Agentic AI Foundation under the Linux Foundation, launched on 2025-12-09 [secondary]. Its FAQ says the closest file wins, but tools differ: Codex concatenates root to working directory, Claude Code concatenates everything it finds [secondary].

**What the studied repositories actually do** [read]:

- Root file layouts: rust-analyzer (`AGENTS.md` links to `CLAUDE.md`), Turso and nushell (`CLAUDE.md` links to `AGENTS.md`), Zed (both link to `.rules`), ruff (`CLAUDE.md` contains `@AGENTS.md`), uv and Biome (`AGENTS.md` only), Deno (`CLAUDE.md` plus `.github/copilot-instructions.md`), codex (none at the root since 2026-09-30). Every one of them is developed mainly on Unix-like systems, which is why symlinks are common; Eileen is Windows-first.
- **Nobody keeps a guidance file in every crate.** Per-directory files appear where a subsystem has a contract worth stating: codex's sandbox integrity checks and TUI state machines, ruff's and Biome's per-crate `CONTRIBUTING.md` files. The owner asked for one per crate; that works if each file is short and holds what code cannot show (invariants, traps, test commands, log locations), and if a test checks that the files exist and stay within limits (section 4.9).
- **Common content**: the exact commands; a few hard rules in ALWAYS and NEVER form (uv); test first, and show it failing and then passing (Turso, Biome); never claim something about the code without a file and line (Biome); write for a reader who never saw the session (uv, ruff, Biome); plain words, no invented terms (Turso); review rules (ruff, codex); proposed rule changes in the PR description, not inline edits (Zed).
- **Skills** sit in `.claude/skills/` (Deno, Turso) or `.agents/skills/` (Zed, Biome, ruff; ruff registers that folder as a local Claude plugin marketplace in `.claude/settings.json`). Whether `.agents/skills/` is a shared discovery path for several tools could not be confirmed [secondary]; Claude's documented project location is `.claude/skills/`.
- **Mixed signal on comments**: TigerStyle says always say why; Turso's guide says do not add comments at all; ruff, uv and Biome say comments explain invariants and reasons, never narrate. Eileen takes the middle (section 7).
- **Codex dropped its root guidance file without explanation.** Read it as a reminder that guidance files are a convenience: the tests, lints and tidy checks are the guardrail, and the files should mostly point at them.

### 2.4 Simulation and concurrency testing

There are three ways to make concurrent software reproducible, and the strongest projects use the first [read, inferred]:

1. **Own the event loop.** The system is a state machine driven by explicit inputs, with time, randomness and IO injected. A simulator feeds inputs on one thread, from a seed, faster than real time. TigerBeetle, FoundationDB and Turso do this. rust-analyzer's observability section makes the same point from another angle: its main loop accepts a closed `enum` of events, so everything that can trigger work is visible in one place [read].
2. **Control the scheduler of real async code.** shuttle (randomised, scales well) and loom (exhaustive within a preemption bound, for small lock-free structures) replace the synchronisation primitives; tokio's `start_paused` and, unstably, `rng_seed` make one runtime repeatable.
3. **Replace the runtime.** madsim and turmoil swap tokio or its network for simulated versions.

**Tools judged for Eileen:**

| Tool | Version (2026-10-20) | Fit for Eileen | Use |
|---|---|---|---|
| Own simulator over a sans-IO core | n/a | Exact: Eileen's nondeterminism is microphone timing, model output, child processes and UIA, none of which a generic runtime models | Primary, in `eileen-sim` |
| `buggify!` (FoundationDB style) | n/a | Fault points inside core logic, active only under the simulator | Yes |
| shuttle and `shuttle-tokio` | 0.9.8 (released 2026-10-20), 1.0.0 (2026-09-04) | Randomised interleavings of the real tokio driver tasks | A handful of driver tests (section 3.4); wait out a cooling-off period before taking 0.9.8 |
| loom | 0.7.2 (2024-04) | Exhaustive checks of a lock-free structure | The audio ring buffer between the WASAPI thread and the core |
| tokio `start_paused` | tokio 1.54.0 [prior] | Deterministic timers in driver tests | Yes |
| tokio `rng_seed` | unstable | Needs `--cfg tokio_unstable` for the whole build, a second target directory on a 16 GB PC | No |
| proptest-state-machine | 0.9.0 | Reference model versus implementation for the job lifecycle | Yes |
| turmoil | 0.7.2 | Simulated TCP and UDP hosts | Later, for network connectors |
| madsim | 0.2.34 | Git patches of crates.io crates, against `deny.toml` | No |
| Antithesis | commercial | Turso uses it for whole-system fault injection in Linux containers | Out of scope |

### 2.5 Testing tools

| Tool | Version | What matters for Eileen [read] |
|---|---|---|
| cargo-nextest | 0.9.148 | Profiles; `[test-groups]` with `max-threads = 1` for tests that need the foreground window; `leak-timeout = { period = "500ms", result = "fail" }` marks tests that leave child processes running (0.9.95); `retries` with `flaky-result = "fail"` reports and fails flaky tests (0.9.131); `--stress-count` and `--stress-duration` loop tests (0.9.103); `--partition hash:k/n` and archives for splitting; JUnit output; record, replay and Perfetto trace export of runs (0.9.123 and 0.9.131, experimental) |
| insta | 1.49.0 | `assert_json_snapshot!` with redactions for ULIDs and timestamps, `sort_maps`, `glob!` for corpus-driven snapshot tests, `cargo insta test --unreferenced reject`; with `CI=true`, `cargo insta test` behaves as if `--check` was passed (since 1.48.0 an explicit option such as `--accept` overrides that) |
| proptest | 1.12.0 | Regression files committed next to the tests; `proptest-state-machine` for model-based lifecycle tests |
| expect-test, cov-mark | 1.5.1, 2.2.0 | rust-analyzer's inline snapshots and "this test covers that branch" marks; insta is already chosen, cov-mark is worth having for the simulator's rare paths |
| cargo-fuzz with libfuzzer-sys and arbitrary | 0.13.2, 0.4.13, 1.5.0 | libFuzzer needs LLVM sanitizers and works only on Unix-like systems, not Windows; needs nightly |
| bolero | 0.13.7 | One harness that runs as a property test under `cargo test` and under fuzz engines; an alternative if cargo-fuzz's split becomes a burden |
| cargo-mutants | 27.1.0 | `--in-diff` mutates only the lines a PR changed; `--shard k/n` splits a full run; a `mutants` profile without debug info speeds it up; works with nextest; reports in `mutants.out` |
| cargo-llvm-cov | 0.9.1 | Works on `x86_64-pc-windows-msvc`; `cargo llvm-cov nextest`; `show-env` measures coverage of arbitrary binaries, so an end-to-end run of real `eileen.exe` processes can be measured; branch coverage needs nightly |
| cargo-semver-checks | 0.51.0 | For published library crates; Eileen's crates are `publish = false`, so wire stability comes from schema snapshots instead. Revisit when a connector SDK crate is published |
| criterion, divan, CodSpeed | 0.8.2 (now in the criterion-rs organisation), 0.1.21, 5.0.3 | codex uses divan; Turso runs both through CodSpeed's compatibility crates. Micro benchmarks matter little for Eileen; its benchmarks are timed scenarios (section 3.6) |
| cargo-hawk | 0.1.15 | Closed-world report of `pub` items no other crate uses; the README says it was written by Codex and is experimental |
| cargo-modules | 0.28.0 (2026-10-18) | Module tree and module dependency graph; no prebuilt binaries, so CI only |
| rust-analyzer `scip` | rustup component | A symbol index (definitions and references) that code search tools read |
| `rust-analyzer-lsp` plugin | in `anthropics/claude-plugins-official` | Gives Claude Code go to definition, references and call hierarchy; each session starts its own rust-analyzer |
| zizmor | 1.31.1, action 0.7.1 | Static analysis of GitHub Actions workflows (dangerous triggers, credential persistence, template injection) |
| cargo-cooldown | 0.3.5 | Refuses crate versions younger than a set age; young tool |

### 2.6 How the best projects split PR checks from slow suites

| Project | On every PR | Slower, elsewhere [read] |
|---|---|---|
| rust-analyzer | Everything except heavy tests; a self-check that runs `analysis-stats` on its own source | Heavy tests only with `RUN_SLOW_TESTS`; nothing nightly in GitHub |
| ruff | Changed-path detection decides which jobs run; `cargo fuzz build`; parser fuzzing when parser code changed; ecosystem checks | Daily fuzz cron that files an issue |
| uv | A plan job, partitioned tests, smoke and integration tests | Benchmarks, ecosystem and system tests on their own workflows |
| Turso | Simulator matrix of profiles with fixed seeds; regression seeds; shuttle tests | Random seeds on `main`; nightly configuration and performance workflows; Antithesis on a schedule |
| tokio | Normal tests | loom on `master`, and on PRs only with a label |
| TigerBeetle | `zig build ci` through its merge queue | VOPR continuously on dedicated machines; devhub benchmarks on every merge |
| Firecracker | PR pipeline, plus a non-blocking pipeline | Performance A/B tests after merge; release QA pipeline |

The pattern: a PR gate that is deterministic and fast, a main-branch lane that adds randomness and catches flakes, and dedicated time for long fuzzing, simulation and performance work whose failures become issues, not red PRs.

## 3. Eileen's test pyramid

### 3.1 Layers

| Layer | What it proves | Tool | Where it runs |
|---|---|---|---|
| Unit and property | One module's logic, including invalid inputs | `#[test]`, proptest, cov-mark | Every OS |
| Snapshot | Wire formats, tool schemas, HUD event shapes, user-facing text | insta | Every OS |
| Integration per crate | A crate through its public API, one test binary in `tests/it/` | nextest | Every OS for platform-neutral crates; Windows for FFI crates |
| Exhaustive | Small state machines (turn-taking, barge-in) for every input sequence up to a length | Own enumerator (matklad's technique) | Linux |
| Deterministic simulation | The core under random inputs and faults, checked by invariants, replayable | `eileen-sim` | Linux on PRs, both OSes nightly |
| Concurrency checking | The tokio glue and lock-free structures | shuttle-tokio, loom, paused tokio time | Linux |
| Fuzzing | Parsers of untrusted input never panic, stay bounded, round-trip | cargo-fuzz, corpus replay tests | Fuzzing on Linux; replay on every OS |
| End to end | Real Eileen processes, real Windows desktop, recorded audio in, event trace out | `eileen-e2e` | `windows-latest` with the stub harness; the owner's PC with real Claude |
| Timed scenarios | The owner's benchmark requests stay fast | `eileen-e2e --bench`, A/B | The owner's PC |
| Soak | Idle and long-running behaviour (memory, handles, CPU) | `cargo xtask soak` | `windows-latest` nightly and weekly; the owner's PC weekly |
| Mutation and coverage | Tests actually check what they cover | cargo-mutants, cargo-llvm-cov | Linux; coverage also Windows |

### 3.2 What the core must look like (WP0 constraints)

Simulation is only cheap if the core is designed for it from the first commit. These are the constraints WP0 should write into `eileen-core` and `eileen-stdx` [inferred, from VOPR, FoundationDB, Turso and rust-analyzer's closed event loop]:

```rust
/// Monotonic time in nanoseconds since the core started. The simulator controls it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonoTime(u64);

/// Everything that can happen to the core. The only input of `Core::step`.
pub enum Input {
    Voice(VoiceInput),               // speech started, speech ended, end of turn, transcript partial or final
    Playback(PlaybackInput),         // samples handed to the device for an utterance, playback drained
    Worker(JobId, WorkerInput),      // one decoded stream-json event, malformed line, exited
    Sidecar(SidecarId, SidecarInput),// tool result, heartbeat, exited
    Timer(TimerId),
    User(UserInput),                 // mute, unmute, cancel, approve, deny (from the mic, hotkey or HUD only)
}

/// Everything the core asks the outside world to do. Drivers carry these out.
pub enum Effect {
    Spawn(SpawnSpec), Kill(ChildId), Send(Target, Message),
    Speak(UtteranceId, Text), Duck(Decibels), StopPlayback,
    SetTimer(TimerId, MonoTime), Emit(BusEvent), Persist(Record),
}

impl Core {
    pub fn step(&mut self, now: MonoTime, rng: &mut CoreRng, input: Input, effects: &mut Vec<Effect>);
}
```

- The production driver (tokio tasks in the core process) turns pipe lines, WASAPI callbacks, timers and hotkeys into `Input`, and `Effect` into IO. It holds no decisions.
- No `Instant::now`, `SystemTime::now` or OS random numbers in core logic; ULIDs are built from `MonoTime` plus wall-clock offset and the injected RNG. Ban the calls with `disallowed-methods` and `#[expect]` the single real implementation.
- Ordered maps (`BTreeMap`, `IndexMap`) wherever iteration order can reach an effect.
- Every queue inside the core has a maximum, and the core reports overflow as an event rather than growing.
- `buggify!(probability)` in `eileen-stdx`: activated per site per run, as in FoundationDB, enabled only when the simulator installs its context. It is a runtime switch, not a Cargo feature, because a `sim` feature on `eileen-core` would be unified into every binary built in the same `cargo build --workspace` [inferred].
- `never!` and `always!` soft assertions in `eileen-stdx` (section 7).

### 3.3 The simulator, `eileen-sim`

A dev-only crate that owns a `World` of simulated devices around one real `Core` [inferred, design]:

| Simulated part | Behaviour it can produce |
|---|---|
| Microphone and voice pipeline | Speech starts and ends at random times, half-finished phrases, pauses mid-sentence, the user talking over Eileen, background speech not addressed to Eileen, mute and unmute |
| Playback device | Hands samples to the device in 10 ms callbacks, so the core's "what was heard" mapping can be checked to the sample |
| Brain | Fast replies after a delay distribution; slow or failed turns; a deep answer arriving after the fast one |
| Workers | Scripted stream-json sequences (from the same files the stub harness uses), plus faults: malformed lines, oversized lines, a crash mid-turn, a hang, a permission request, an exit during approval |
| Sidecars | Desktop and app tool calls with latency; a hang far past the deadline (the 38.7-minute UIA hang [prior] is a scenario); a crash and restart |
| Supervisor substrate | Job Object semantics: killing a child kills its descendants; core restart with the same SQLite file |
| Store | Real rusqlite on `:memory:`; SQLite is deterministic for a given sequence of statements |

**Each run** picks a profile (fault rates, user talkativeness, number of parallel jobs, worker behaviour), seeds a PRNG, then loops: choose the next input (earliest timer or a random external event drawn from the profile), call `Core::step`, apply the effects to the world, check every invariant. A profile chosen at random per run, with buggify sites activated per run, is swarm testing.

**Invariants checked after every step** (each one a named function, so a failure names what broke):

1. The voice path is never starved: every voice input is consumed within two audio frames of virtual time, whatever the jobs and sidecars are doing.
2. Barge-in timing, from the voice report's design [prior]: after the first speech window while Eileen is speaking, playback is ducked by the next output callback and stopped by the third window (96 ms); onset to silence never exceeds 150 ms of virtual time.
3. What the brain is told the user heard equals the text mapped from samples actually handed to the device, not the text that was generated.
4. Every job ends in exactly one terminal state, within its deadline plus the supervisor's kill and restart bound; no job stays running without a live worker past one heartbeat timeout.
5. Job state changes follow the legal graph (checked against a proptest-state-machine reference model in a separate test).
6. Every bus event carries a request ID, and a job ID where it belongs; nothing is emitted for a job after its terminal event; per-job order is preserved; a lagging subscriber gets an explicit lag event, never silent loss.
7. A tool call above the reversible tier is always preceded by an approval that came from `UserInput`, never from screen text, model output or a learned skill; replaying a skill never raises a tier [prior, learning-loop report].
8. A crashed or hung child is restarted with bounded backoff and at most N times per window; killed children leave no descendants.
9. Mute stops audio reaching speech recognition within one frame; "cancel" reaches the worker as an interrupt within its bound and the job ends cancelled.
10. Two utterances never overlap; the announcement queue stays bounded; nothing is spoken while muted.
11. Two-speed answers: the deep answer either replaces the fast one or goes to the HUD; the user never hears both in full; an interruption sends the deep answer to the HUD.
12. After a simulated core restart, jobs that were running are marked interrupted and no job ID repeats.

**Ergonomics copied from Turso and TigerBeetle** [read]:

- `cargo xtask sim` runs the PR seed set; `cargo xtask sim --seed 0x5eed --profile barge-in-heavy` replays one run; the failure message prints that command with the commit SHA.
- `--doublecheck` runs the same seed twice and compares a hash of the trace, which catches nondeterminism leaking into the core.
- Output is one line per step in fixed columns (virtual time, input kind, job, voice state, queue depths, live children), so a failure can be read like TigerBeetle's VOPR log.
- `crates/eileen-sim/regressions/*.json` hold decoded input scripts of past failures, replayed on every PR. A seed alone is not stored as a regression because a generator change silently changes what it means.
- Profiles are TOML or JSON with a generated JSON Schema for editor completion, like Turso's.
- Scenarios are generated from an `arbitrary::Unstructured` byte buffer. A seeded PRNG fills the buffer for normal runs; `fuzz/fuzz_targets/sim.rs` lets libFuzzer fill it with coverage guidance; shrinking the buffer minimises a failure (matklad's finite RNG).
- Hand-written scenarios for specific cases run on the same infrastructure, as TigerBeetle does for cases that random search reaches slowly.
- The simulator runs on a thread with an explicit large stack (`std::thread::Builder::stack_size`), because Turso's simulator overflowed the stack on Windows, where the main thread's default is smaller [read, inferred].

**Exhaustive tests** for the turn-taking and barge-in state machine: enumerate every sequence of its inputs (speech start, speech end, end of turn, TTS chunk ready, playback tick, mute, unmute, brain text) up to length 6 on PRs (about 262,000 sequences) and 8 nightly, checking invariants 2, 3, 9 and 10 after each [inferred].

**Budgets** [inferred, to be measured in WP0]: PR seed set under 90 seconds on a Linux runner; nightly 30 minutes per profile.

### 3.4 Concurrency checking of the tokio glue

The drivers that sit between `Core::step` and the real world are small but concurrent [inferred]:

- **Paused-time tests** (`#[tokio::test(start_paused = true)]`, current-thread runtime) for deadlines, heartbeats, restart backoff and the bus fan-out, so no test sleeps in real time.
- **shuttle-tokio** for four to six tests where task interleaving matters (bus broadcast with a slow subscriber, a supervisor restart racing a job cancel, two workers finishing at once). Build them in their own invocation (`cargo nextest run -p eileen-core --features shuttle`), never in the same `cargo build` as a shipped binary, so feature unification cannot leak the wrapper into `eileen.exe`.
- **loom** for the single-producer single-consumer ring buffer between the WASAPI capture thread and the core, under `cfg(loom)`. On PRs it runs only when `crates/eileen-audio/**` changes (tokio's label approach, automated by the plan job); nightly always.

### 3.5 Windows end-to-end harness, `eileen-e2e`

**Audio in and out without a sound card** [inferred]:

- `eileen-audio` defines `AudioSource` and `AudioSink` traits from day one. Production uses WASAPI. Tests use `WavSource { path, pace }` (real-time or accelerated) and `CaptureSink`, which writes what Eileen "says" to a WAV and counts samples handed over.
- A `RoomMixer` sink feeds a delayed, attenuated copy of the playback back into the next input frames, so echo cancellation and barge-in are exercised with speaker bleed.
- Hidden developer flags select them: `eileen run --audio-source wav:<file> --audio-sink capture:<file>`. GitHub's hosted Windows runners have no audio device [inferred], and a virtual audio driver would need an install and a signed driver. Injecting at the boundary is deterministic and also lets the owner reproduce a bug from a recording.

**Workers without credentials** [inferred]:

- `eileen-stub-harness`, a dev-only binary that speaks the same stream-json as `claude -p`, plays back scripts in `tests/harness-scripts/*.ndjson`, and obeys fault directives (delay, crash, malformed line, permission request). codex tested its agent against a mock model server built from small event constructors [read]; this is the same idea one layer down.
- Scripts start as recordings of real sessions on the owner's PC, redacted with the learning loop's redaction module.
- A weekly contract test on the owner's PC runs the same prompts through the real `claude -p` and compares the shape of the events (types and fields, not text) with the stub scripts, so CLI changes show up as a failing check, not as a surprise. CI never holds vendor credentials.

**Scenarios as directories**, in Deno's spec-test style [read]:

```text
tests/scenarios/barge-in-while-narrating/
  scenario.toml        # steps, required environment, timing checks; validated by a JSON Schema
  expect.trace         # expected event log with [ANY], [ANYLINE] and [UNORDERED] markers
```

```toml
description = "The user interrupts narration; Eileen stops within 150 ms and continues from what was heard."
requires = []                     # for example ["resolve"], ["photoshop"], ["real-claude"]
worker = "stub:narrate-long"      # a stub script, or "claude" in the owner's PC lane

[[step]]
play = "audio/whats-the-render-status.wav"

[[step]]
wait_for = "tts.started"

[[step]]
after_ms = 800
play = "audio/stop-wait-a-second.wav"

[timing]
"voice.speech_started[2] -> playback.stopped" = { max_ms = 150, ci_factor = 3.0 }
```

- The harness starts `eileen.exe` with a throwaway data directory and profile, drives the steps, collects the event log over the debug pipe, matches it against `expect.trace`, and checks timing. On CI timing limits are multiplied by `ci_factor`, because the simulator already proves the logic and hosted runners are noisy; on the owner's PC the limits are strict.
- Desktop scenarios use apps that exist on the runner image (Notepad, Explorer). Hosted Windows runners are reported to run with an interactive desktop session, which UI Automation needs [secondary]; the first nightly run should prove it with a Notepad round trip before more scenarios depend on it. Desktop tests share one nextest test group with `max-threads = 1`, because they compete for the foreground window.
- **Recorded speech**: the owner's own recordings (16 kHz mono, about 32 KB per second, so a hundred five-second utterances are about 16 MB), plus utterances synthesised with Kokoro's voices and seeded noise for variety. Recordings of anyone else need their consent.

### 3.6 The owner's benchmark requests as timed scenarios

| Scenario | Request | Measured phases (from event log spans) | Where |
|---|---|---|---|
| B1 | "Open the video editor, go to 15:30 and make the face of the girl on the right less blurry" | End of speech to Resolve focused; to playhead at 15:30 (checked through the Lua bridge); to the effect applied; total; worker turns | Owner's PC (Resolve Studio) |
| B2 | The Photoshop beach-photo request | Same shape | Skipped until Photoshop is installed (`requires = ["photoshop"]`) |
| B3 | "Launch three agents to do X" | Spawn of each job; first narration per job; all finished; HUD shows all three | CI with stubs; owner's PC with real Claude |
| B4 | Writing code by speaking | End of speech to text in the editor; word error rate against the script | Both |
| B5 | Interrupting Eileen mid-sentence | Onset to silence; correctness of what Eileen says it was told | Both (strict on the owner's PC) |
| B6 | "What's the best place to eat near me" | Fast answer start; deep answer arrival; spoken length; HUD content | Owner's PC (network, location opt-in) |
| B7 | Dictation into any field | End of speech to text landed; filler removal accuracy | Both |
| B8 | The same hard request twice | Second run time, tokens and worker turns against the first (the learning loop's promise) | Owner's PC |

**How results are judged** [read Firecracker, TigerBeetle; inferred for Eileen]:

- Each run appends one JSON line (scenario, phase, milliseconds, commit, binary hash, Claude CLI version, free RAM, CPU load, whether the owner was active) to a results store, TigerBeetle's devhub pattern. Recommended home: an orphan `devhub` branch in this repository or a separate small repository; the owner decides.
- Regressions are found by A/B: build base and head, alternate runs (ABAB) to cancel drift, compare each phase with a non-parametric test (Mann-Whitney U) and report only differences that are both significant and larger than about 10 percent. No absolute thresholds, because every 2026-10-20 timing on this PC was taken under contention [prior].
- Runs taken with less than 2 GB free memory are recorded but excluded from comparisons, matching the build guard's limit [prior].
- Micro benchmarks (for example, the skill matcher) use divan, sized to about 300 ms per benchmark as matklad suggests [read].

### 3.7 Fuzz targets

Every target lives in `fuzz/fuzz_targets/` (its own workspace, as cargo-fuzz generates it), calls a function in a platform-neutral crate, and has a corpus in `fuzz/corpus/<target>/` that `cargo xtask fuzz-replay` runs as an ordinary test on every OS [inferred].

| Target | Untrusted input | Crate (parser lives in) | Property |
|---|---|---|---|
| `stream_json_line` | Lines from `claude -p` | `eileen-harness` | No panic; line length capped before parsing; unknown `type` becomes an `Unknown` event; known events round-trip |
| `control_message` | `control_request` and `control_response` lines | `eileen-harness` | No panic; unknown subtypes rejected with a typed error |
| `mcp_client_message` | JSON-RPC from sidecars and adapters, including image results | `eileen-mcp` | No panic; image payloads bounded; a message with both `result` and `error` rejected (rmcp 3.6.0 behaviour [prior]) |
| `mcp_tool_arguments` | Tool arguments a worker sends to Eileen's MCP servers | `eileen-mcp`, `eileen-contracts` | Schema validation never panics; invalid input never reaches a tool |
| `worker_hook_payload` | HTTP hook bodies from worker settings hooks (the tier guard) | `eileen-core` | No panic; anything unparsable is denied (fail closed) |
| `acp_message` | ACP messages (when an ACP route lands) | `eileen-harness` | No panic |
| `uia_text` | Names, values and text-pattern strings from UI Automation | `eileen-contracts::desktop` | Output is valid UTF-8, bounded, free of control and bidirectional override characters, and tagged untrusted |
| `desktop_state` | DesktopState snapshots from the desktop sidecar | `eileen-contracts` | No panic; round-trip; oversized trees truncated with a marker |
| `config_toml` | The user's configuration files | `eileen-config` | No panic; every error names a line; defaults round-trip |
| `skill_record` | Learned skill files and their front matter, later imported skill packs | `eileen-skills` | No panic; a skill that would raise a tier is rejected |
| `speech_frame` | Binary frames from `eileen-speech` | `eileen-contracts` | No panic; length fields validated before allocation |
| `hud_event` | Events the HUD decodes | `eileen-contracts` | No panic |
| `resolve_reply` | Replies from the Resolve Lua bridge | `eileen-apps` (pure parser module) | No panic |
| `command_grammar` | Transcripts into the no-model fast path | `eileen-core` | No panic; normalisation is idempotent |
| `dictation_cleanup` | Transcripts into the dictation cleaner | `eileen-speech` (pure module) | No panic; output no longer than input plus a small constant; words outside the filler list survive |
| `sim` | Simulator scenarios from bytes | `eileen-sim` | All invariants of section 3.3 |

Rules [read Turso, matklad, ruff]:

- `cargo fuzz build` runs on every PR that touches a parser crate, so targets never rot; fuzzing itself runs nightly, ten minutes per target.
- A crash is minimised, its input goes into `fuzz/regressions/<target>/`, and a focused regression test lands with the fix. A bug found any other way in a fuzzed parser is first treated as a gap in the fuzz target, which is extended until it finds the bug.
- Parsers stay out of FFI modules so they can be fuzzed on Linux; the FFI layer only converts COM strings to Rust strings.

### 3.8 Soak tests

An always-on process fails slowly. `cargo xtask soak` runs the real processes with WAV input and stub workers, plays a scripted request every few minutes, and samples once a second [inferred]:

| Measure | Why | Pass criterion |
|---|---|---|
| Working set and private bytes per process | Leaks | Slope after a 10-minute warm-up within a small bound (for example under 1 MB per hour for the core) |
| Handle count | Leaked pipes, events, Job Objects | Flat after warm-up |
| GDI and USER objects (`GetGuiResources`) | HUD and tray leaks, which Windows caps per process | Flat |
| Thread count | Leaked tasks or blocking threads | Flat |
| CPU while idle | The owner wants it fast at rest; the skeleton idled at 0.0 percent [prior] | Under about 1 percent of one core averaged over idle periods |
| Live child processes | Orphans | Exactly the expected set |
| Log, SQLite and recorder sizes | Unbounded growth | Within configured caps |
| Latency of the scripted request | Slow decay | No upward trend |

`scripts/measure-idle.ps1` from the Rust report [prior] already samples most of this; the soak runner should call the same Win32 APIs from Rust and judge slopes, not absolute numbers.

| Where | Duration | When |
|---|---|---|
| `windows-latest` | 30 minutes | Nightly |
| `windows-latest` | 4 hours (jobs may run for up to 6 hours [read]) | Weekly |
| Owner's PC, real Claude idle with occasional requests | Overnight, about 8 hours, with keep-awake | Weekly, owner opt-in |

### 3.9 Mutation testing and coverage

- `cargo mutants --in-diff` on PRs for the pure logic crates (`eileen-core` state machines, `eileen-contracts` validation, `eileen-skills` matcher and ladder), on Linux, advisory at first. Make it gating per crate once that crate's full run has no missed mutants [read, inferred].
- A full sharded run weekly (four shards), with the report uploaded.
- `cargo llvm-cov nextest` nightly on both OSes, plus `show-env` coverage of an end-to-end run. Coverage is a map for where to aim fuzzing and simulation, not a gate.

### 3.10 What runs when

| Lane | Contents | Budget |
|---|---|---|
| Every push to a PR branch | Format, typos, cargo-deny, cargo-machete, zizmor, tidy (including docs rules and the code map check); Linux: clippy and nextest of platform-neutral crates, simulator PR seeds and stored regressions, exhaustive tests to depth 6, fuzz corpus replay, `cargo fuzz build` when parsers change, loom when `eileen-audio` changes, `cargo mutants --in-diff` (advisory); Windows: clippy `-D warnings` on the workspace, nextest on the workspace, doc tests, end-to-end smoke scenarios with stubs | Under about 15 minutes wall with a warm cache |
| Merge to `main` | The PR lane again (it is the only proof that the merged result works without a merge queue), plus random simulator seeds for 10 minutes and a release smoke build | 20 minutes |
| Nightly | Simulator random seeds per profile (Linux, plus one Windows run); fuzzing 10 minutes per target; exhaustive tests to depth 8; full end-to-end scenario set; 30-minute soak; nextest with `retries = 2` and `flaky-result = "fail"` to find flakes; shuttle and loom suites; coverage; thin-LTO release build and smoke test; SCIP index artifact. Failures open or update an issue labelled `nightly` | About 1 hour per job, in parallel |
| Weekly | Full sharded mutation run; 4-hour soak; one hour per hot fuzz target; build timing report (`cargo build --timings`); cargo-hawk report; dependency update PR respecting a 14-day cooling-off period | Hours, in parallel |
| Before a release | Everything above on the release commit, the `dist` (fat LTO) build, end-to-end on a fresh runner, licence check of downloaded native libraries, and the owner's PC A/B of B1 to B8 against the previous release | Owner signs off |
| Owner's PC, nightly when idle | Benchmarks B1 to B8 with real Claude within a nightly turn budget the owner sets; stream-json contract test; results to the devhub store | Bounded by the owner's plan limits |

## 4. CLAUDE.md and AGENTS.md

### 4.1 The layout and why

| File | Content |
|---|---|
| `AGENTS.md` (root) | The canonical guide for every coding tool (section 4.2) |
| `CLAUDE.md` (root) | `@AGENTS.md`, then a short Claude-only section (section 4.3) |
| `crates/<name>/AGENTS.md` | The crate's guide from the template (section 4.4) |
| `crates/<name>/CLAUDE.md` | `@AGENTS.md` (one line) |
| `.claude/settings.json` | Permission rules that make the memory guard and the credential rule mechanical |
| `.claude/rules/*.md` | Path-scoped rules for FFI modules and fuzz targets (Claude only) |
| `.claude/skills/*/SKILL.md` | Four procedures: simulator debugging, adding a fuzz target, writing an end-to-end scenario, working a work package |
| `docs/wp/WP<n>-<topic>.md` | Living work package files (section 4.8) |

The task as written asks for `CLAUDE.md` with the content and an `AGENTS.md` that points to it. The reverse is better, for evidence in section 2.3: Claude Code expands `@AGENTS.md`, so a `CLAUDE.md` that imports the file gives Claude the full text; Codex has no import syntax, so an `AGENTS.md` that only says "read CLAUDE.md" gives Codex nothing unless the model decides to open the file. Symlinks are ruled out on Windows. The owner still gets a `CLAUDE.md` in the root and in every crate; it just borrows its words.

Two cautions from Zed's rules hygiene and the Claude docs [read]: content that describes structure goes stale fastest, and longer files are followed less. So each crate file states ownership in at most three bullets and spends the rest on invariants, traps, testing and logs, which do not change with every refactor.

### 4.2 Root `AGENTS.md`

```markdown
# Eileen: guide for contributors and coding agents

Eileen is a voice-first AI layer for Windows 11. The always-on core is Rust; models run through each
vendor's own signed-in CLI. Every coding tool reads this file; `CLAUDE.md` imports it.

## Read first
- `ARCHITECTURE.md`: the map, the invariants, the boundaries.
- `docs/OWNER-DIRECTION.md`: what the owner asked for. Binding.
- `docs/BUILD.md` and `docs/wp/`: work packages and their live status.
- The `AGENTS.md` of the crate you are changing.
- `docs/CODEMAP.md` (generated): crate graph, module trees, and which code emits and handles each bus event.

## Commands
Build only through `cargo xtask`; it enforces the memory guard on the owner's PC.

| Task | Command |
|---|---|
| Check one crate | `cargo xtask check -p <crate>` |
| Test one crate | `cargo xtask test -p <crate> [-- <filter>]` |
| Lint as CI does | `cargo xtask clippy` |
| Simulator, PR seeds | `cargo xtask sim` |
| Replay a simulator run | `cargo xtask sim --seed <hex> --profile <name>` |
| Fuzz corpus replay | `cargo xtask fuzz-replay` |
| End-to-end scenario | `cargo xtask e2e <scenario>` |
| Regenerate generated files | `cargo xtask codegen` |
| Repository checks | `cargo xtask tidy` |
| Before pushing | `cargo xtask pre-push` |
| What CI's PR lane runs | `cargo xtask ci` |

## Rules that do not bend
1. Never read, copy, print or log vendor credential files, and never run a vendor login flow.
2. The product is Eileen. Workers are named by route: Claude, Codex, Grok, DeepSeek.
3. No API keys except DeepSeek's, and never in the repository.
4. No `unwrap`, `expect`, `panic!` or `todo!` outside tests. Libraries return `thiserror` enums; only binaries use `anyhow`.
5. `unsafe` only in a crate's private `ffi` module: one call per block, each with a `// SAFETY:` comment.
6. Every child process starts through `eileen_core::supervisor`.
7. Every wait on another process has a deadline. Every queue and channel has a maximum.
8. Screen text, transcripts, model output and tool arguments are untrusted. They never approve an action or raise a permission tier.
9. Core logic never reads the clock or OS randomness; both are passed in.
10. Docs and comments are plain and editorial: no em dashes, no marketing words.

## On the owner's PC (16 GB, shared with other agents)
- Never `cargo build --release`, `--profile dist`, `cargo install` or `cargo clean`. CI builds releases.
- If `cargo xtask` says both build slots are taken, wait. Do not call cargo directly to get around it.
- Wait for one cargo command to finish before starting another.
- Do not start a long build, simulation or soak at the end of a session.

## Making a change
1. Find the crate that owns the data (code map) and read its `AGENTS.md`.
2. Write the test first, at the narrowest layer that shows the problem; run it and see it fail.
3. Make the smallest change that fits the existing design. A new `pub` item used across crates, a new dependency or a new crate is an architectural change: say so in the PR.
4. Run the crate's tests, then `cargo xtask pre-push`.
5. One topic per branch and PR. Keep logic changes under about 500 changed lines.
6. Commit subject in the imperative, under 72 characters; the body says why and which invariant is affected.
7. If what a crate `AGENTS.md`, `ARCHITECTURE.md` or the work package file says is no longer true, fix it in the same PR.

## Testing
- Unit tests sit next to the code; each crate has one integration test binary, `tests/it/main.rs`.
- Core behaviour (jobs, bus, scheduling, barge-in, crashes, approvals) is tested in `eileen-sim`: add an invariant or a scenario there, never a test that sleeps.
- Snapshots use `insta`. Never edit a `.snap` file by hand; regenerate it and read the diff.
- Every parser of untrusted input has a fuzz target and a corpus. A fuzz or simulator failure becomes a regression file before its fix is done.
- No `#[ignore]`, no `#[should_panic]`, no real-time sleeps. If behaviour is wrong today, assert the wrong behaviour and add `FIXME(#issue)`.
- `main` is green. Never assume a failure was already there.

## Code Review Rules
- Check the change against the crate's invariants and these rules.
- Flag: unbounded queues; waits across a process boundary without a deadline; new cross-crate `pub` items or dependencies; tests at the wrong layer or missing; snapshot changes without an explanation; untrusted text reaching a permission decision; clock or randomness read in core logic; any vendor credential path.
- Every claim in a review cites a file and line or a command that shows it.

## Writing for readers
Write comments, docs and PR text for someone who never saw your session. Say why, not what. Do not mention
conversations, prompts or abandoned attempts.

## Changing guidance files
Do not edit `AGENTS.md` files during feature work. Put proposed text under "Suggested AGENTS.md changes" in
the PR description. A rule is added only if it is non-obvious, came up more than once, and is specific
enough to act on.
```

That is about 90 lines, well under both tools' limits.

### 4.3 Root `CLAUDE.md`

```markdown
@AGENTS.md

## Claude only
- Project skills: `sim-debug`, `add-fuzz-target`, `e2e-scenario`, `work-package` in `.claude/skills/`. Load the one that fits.
- `.claude/settings.json` denies release builds, `cargo install`, `cargo clean` and reads of vendor credential files. If a denial blocks you, stop and ask; do not work around it.
- Final review: use a fresh subagent and give it only the diff and the goal, not your own suspicions.
- The rust-analyzer LSP plugin is optional here: every session starts its own rust-analyzer, and this PC is often short of memory.
```

### 4.4 Crate `AGENTS.md` template

```markdown
# <crate-name>

<One sentence: what this crate is for.>

## Owns
- <At most three bullets: data, processes or decisions only this crate changes.>

## Does not own
- <The nearest thing people expect here, and where it actually lives.>

## Invariants
- <Each stated positively, with how it is enforced: type, assertion, lint, test or simulator invariant.>

## Boundaries
- Called by: <crates or processes>.
- May depend on: <crates>. Must not depend on: <crates or kinds of library>.
- Wire types: <module of eileen-contracts>.

## Testing
- Fast: `cargo xtask test -p <crate>`.
- <Layer specifics: simulator scenarios, fuzz targets, snapshot files, nextest test groups.>
- A bug here starts as <the kind of failing test>.

## Traps
- <Non-obvious things that broke before, each with the reason.>

## Logs and debugging
- Tracing target `<crate_name>`; process log in `%LOCALAPPDATA%\eileen\logs\<process>\`.
- More detail: `EILEEN_LOG=<crate_name>=debug`.
- <Flight recorder fields this crate adds; the replay or probe command for it.>
```

Limit: 60 lines per crate file, checked by tidy.

### 4.5 Example: `crates/eileen-core/AGENTS.md`

```markdown
# eileen-core

The always-on core's logic: event bus, job registry, scheduler, turn-taking and barge-in, supervisor policy and permission tiers.

## Owns
- Job lifecycle and the job registry (SQLite through rusqlite).
- When to speak, duck, stop and resume, and what the user heard.
- Which child processes run, their deadlines, restarts and kills.

## Does not own
- Audio devices (`eileen-audio`), speech models (`eileen-speech`), vendor CLI protocols (`eileen-harness`),
  serialized types (`eileen-contracts`), learned skills (`eileen-skills`).

## Invariants
- Logic modules are sans-IO: `Core::step(now, rng, input, effects)` is the only entry point. Drivers in
  `driver/` do IO and make no decisions. Checked by the simulator and by `disallowed-methods`.
- No UIA, COM, ONNX Runtime or WebView code is linked into this crate (`cargo xtask tidy` checks the dependency tree).
- Every queue has a maximum; overflow is an event, never growth.
- Every child starts through `supervisor::spawn`; the one `Command::new` carries an `#[expect]`.
- An action above the reversible tier needs an approval built from `UserInput`; the type cannot be made from screen text,
  model output or a skill.
- Every emitted `BusEvent` carries a request ID.

## Boundaries
- Called by: `eileen-cli` (the `eileen` binary) and `eileen-sim`.
- May depend on: `eileen-contracts`, `eileen-stdx`, `eileen-skills`, `eileen-connectors`. Must not depend on any FFI crate.

## Testing
- Fast: `cargo xtask test -p eileen-core`.
- Behaviour: `cargo xtask sim`; add invariants in `eileen-sim/src/invariants.rs`, scenarios in `eileen-sim/scenarios/`.
- Turn-taking: the exhaustive test `turn::exhaustive` (depth 6; nightly depth 8).
- Job lifecycle: `tests/it/job_model.rs` (proptest-state-machine against a reference model).
- Driver timing: paused-time tokio tests in `tests/it/driver.rs`; never real sleeps.
- A bug here starts as a simulator regression file or a failing invariant.

## Traps
- Iterating a `HashMap` where order reaches an effect breaks replay; use `BTreeMap` or `IndexMap`.
- A Cargo feature on this crate is unified into every binary built in the same invocation; use the runtime switch for `buggify!`.
- Tokio broadcast drops messages for slow receivers; handle `Lagged` explicitly.

## Logs and debugging
- Tracing targets `eileen_core::{bus,jobs,turn,supervisor,tiers}`; log in `%LOCALAPPDATA%\eileen\logs\core\`.
- The flight recorder holds the last inputs and effects of `Core::step`; `eileen debug dump` writes them, and
  `cargo xtask sim --replay <dump>` replays them.
```

An FFI crate's file differs mainly in its invariants and testing, for example for `eileen-desktop`: every UIA call runs on the crate's own STA thread with a deadline; the process may be killed at any moment and must hold no state that is not rebuilt on start; text leaving the crate goes through `eileen_contracts::desktop::sanitize`; desktop tests run in the `desktop` nextest group (one at a time) and only on Windows.

### 4.6 Crate `CLAUDE.md`

```markdown
@AGENTS.md
```

### 4.7 Claude-only extras

**`.claude/settings.json`**, committed. Claude's memory docs say instruction files are context, not enforced configuration, and point to hooks for hard blocks [read]; permission rules in settings are the other mechanism the harness enforces rather than the model [inferred]. Check the exact rule syntax against the settings reference when WP0 writes the file:

```json
{
  "permissions": {
    "allow": ["Bash(cargo xtask:*)", "Bash(git status:*)", "Bash(git diff:*)"],
    "deny": [
      "Bash(cargo install:*)",
      "Bash(cargo clean:*)",
      "Bash(cargo build --release:*)",
      "Bash(cargo build --profile dist:*)",
      "Read(~/.claude/.credentials.json)",
      "Read(~/.codex/auth.json)"
    ]
  }
}
```

**`.claude/rules/ffi.md`**:

```markdown
---
paths:
  - "crates/*/src/ffi.rs"
  - "crates/*/src/ffi/**/*.rs"
---
# FFI modules
- One unsafe call per block, each with a `// SAFETY:` comment saying which precondition holds and why.
- Expose only safe functions; convert every HRESULT or BOOL into the crate's error type here.
- COM calls run on the crate's own STA thread with a deadline, never on a tokio worker.
```

**`.claude/skills/sim-debug/SKILL.md`** (the shape of all four skills):

```markdown
---
name: sim-debug
description: Use when an eileen-sim run fails, a simulator regression file fails, or a doublecheck reports a trace mismatch. Not for writing new invariants.
---
1. Copy the replay command from the failure output; run it once to confirm it reproduces.
2. Read the last 40 lines of the step log; find the first line where an invariant's inputs went wrong, not the line where it fired.
3. If `--doublecheck` fails, the core read the clock, used OS randomness or iterated an unordered map. Search the diff for those first.
4. Minimise: `cargo xtask sim --seed <hex> --minimize`; save the result in `crates/eileen-sim/regressions/`.
5. Write the fix; the regression file and the original seed both pass; run the PR seed set.
```

### 4.8 Work package files

The ExecPlan pattern from OpenAI's cookbook (a living plan with progress, surprises, a decision log and outcomes, written so someone with no context can continue) [secondary] matches how Eileen's runs already hand work between sessions. One file per work package, updated by the agent doing it:

```markdown
# WP<n>: <topic>

Goal: <one paragraph, linked to BUILD.md and OWNER-DIRECTION.md entries>.

## Done when
- [ ] <behaviour> with <test layer> (unit, simulator invariant, fuzz target, scenario)
- [ ] Crate AGENTS.md files and the ARCHITECTURE.md code map entry are current
- [ ] CI green; merged

## Progress
- 2026-10-21: <what landed, PR link>

## Surprises
- <what was not as expected, with evidence>

## Decisions
- <decision, reason; a DECISIONS.md record if it changes the design>

## Outcome
- <filled in when done>
```

### 4.9 Keeping guidance true

`cargo xtask tidy` (a std-only `#[test]` suite in `xtask`, rust-analyzer's approach) checks [inferred]:

- Every `crates/*` has `AGENTS.md` with the template's headings and a `CLAUDE.md` whose first line is `@AGENTS.md`; crate files at most 60 lines; root `AGENTS.md` at most 150 lines; root plus any one crate file under 24 KiB (Codex's limit is 32 KiB).
- Every `cargo xtask <command>` named in any `AGENTS.md` exists.
- Every crate has an entry in the `ARCHITECTURE.md` code map, and every entry names an existing crate.
- No em dash (U+2014) in Markdown or Rust doc comments; no old working names of the product; no user-facing string starting with a vendor tool's product name; no vendor credential path in any crate source (the CI grep firewall from the Rust report moves here so it also runs locally).
- No `#[ignore]` or `#[should_panic]` in tests; modules over 800 lines appear in `tidy-allow.toml` with a reason.
- Every fuzz target has a corpus directory and a replay entry; every simulator regression file parses.

Also: run `/doctor prompt-audit` monthly and on every change to the guidance files; follow Zed's "suggested additions in the PR description" rule; and add the guidance check to the PR template's checklist.

## 5. ARCHITECTURE.md

### 5.1 Placement and size

Put `ARCHITECTURE.md` at the repository root, next to `README.md` and `CONTRIBUTING.md`, as matklad recommends. Aim for 300 to 400 lines. The current `docs/ARCHITECTURE.md` (34 KB) is a detailed specification of the Python-era design (DesktopState schema, routing policy, permission tiers, channels). Move it to `docs/design/2026-09-design-spec.md`, keep its still-valid parts referenced from BUILD.md, and record the move in DECISIONS.md.

### 5.2 Skeleton

```markdown
# Architecture

This is the map of Eileen. Read it before changing code. It describes what changes rarely; details live in
each crate's AGENTS.md and in the code. Revisited every few months; last revisited <date>.

## Bird's eye view

Eileen listens whenever the microphone is not muted, decides what the user wants, and either answers at once or
starts jobs that run in parallel while the conversation continues. Input: audio, the hotkey and HUD clicks,
what is on screen, events from connectors. Output: speech, the HUD, actions on the PC through tools, and
messages to workers (vendor CLIs on the owner's plans). State: the job registry, the event log, learned
skills and the user model, all on the user's PC.

One core process owns the microphone loop, the event bus, jobs, permissions, skills and the supervisor.
Everything that can hang or crash (UI Automation, app COM, speech models, the HUD, vendor CLIs) runs in a
child process the supervisor can kill.

## Entry points

- `crates/eileen-cli/src/main.rs`: the `eileen` binary; starts the runtime, the drivers and the core.
- `Core::step` in `eileen-core`: every decision starts here; read its `Input` and `Effect` enums first.
- Each sidecar's `main.rs` (`eileen-speech`, `eileen-desktop`, `eileen-apps`, `eileen-hud`).
- `eileen-sim`: the fastest way to see the core run.

## Code map

| Crate | Kind | One line |
|---|---|---|
| eileen-contracts | lib | Wire types and schemas |
| eileen-stdx | lib | Assertions, buggify, time and small helpers |
| eileen-core | lib | Core logic and its drivers |
| ... | | |

### `eileen-contracts`
Every type that crosses a process, pipe, file or socket, with serde and schemars; snapshot tests pin every shape.
**Architecture Invariant:** this is the only crate whose types are serializable. Other crates convert to and from
these types at their edge, so internal types can change freely.
**API Boundary:** wire formats, the event log format, the skill file format and the config schema.

### `eileen-stdx`
### `eileen-core`
**Architecture Invariant:** sans-IO logic behind `Core::step`; no clock, no OS randomness, no IO in logic modules.
**Architecture Invariant:** links nothing that can hang (UIA, COM, ONNX Runtime, WebView).
### `eileen-skills`
### `eileen-connectors`
**API Boundary:** the `Connector` trait; phone, home and other devices are new implementations, not a redesign.
### `eileen-mcp`
**Architecture Invariant:** the only crate that knows rmcp.
### `eileen-harness`
**Architecture Invariant:** the only crate that knows vendor CLI flags and protocols (stream-json, ACP, app-server).
### `eileen-config`
### `eileen-audio`
### `eileen-shell`
### `eileen-cli`
### `eileen-speech`, `eileen-desktop`, `eileen-apps`, `eileen-hud`
**Architecture Invariant:** may crash or be killed at any time; hold no state that is not rebuilt on start.
### `eileen-sim`, `eileen-e2e`, `eileen-stub-harness` (development only)
### `xtask`, `tools/codemap`, `fuzz/`

## Cross-cutting concerns

### Process isolation and supervision
### Events, request IDs and the event log
### Time, randomness and determinism
### Cancellation, interruption and barge-in
### Permissions and untrusted input
### Error handling and assertions
### Testing
### Observability and debugging
### Configuration
### Serialization and stability
### Generated files
### Performance budgets
### Platform boundaries
```

What each cross-cutting section should say, in two or three sentences each when WP0 fills it [inferred unless marked]:

- **Process isolation and supervision**: the process table from the Rust report [prior]; every child in a kill-on-close Job Object; every call has a deadline and every sidecar a heartbeat.
- **Events, request IDs and the event log**: a request ID (ULID) is created when an utterance, hotkey or HUD action starts a request; every bus event, tracing span and persisted record carries it. Across MCP it travels as W3C `traceparent` in `_meta`, the convention MCP's SEP-414 made final [read]; the per-job worker MCP configuration also passes the job ID, since Claude does not yet forward trace context into the tool calls it makes [secondary]. The event log format in `eileen-contracts` is shared by the flight recorder, the simulator, the end-to-end harness and the learning loop.
- **Time, randomness and determinism**: `MonoTime` and the injected RNG; what the simulator controls; ordered maps.
- **Cancellation, interruption and barge-in**: the interrupt path from speech onset to `StopPlayback` and to a worker `interrupt` control message; the timing budget.
- **Permissions and untrusted input**: tiers; which inputs are untrusted; the approval type that only `UserInput` can build.
- **Error handling and assertions**: operating errors are values; programmer errors are `never!` in the core and hard asserts in sidecars; partial results stay partial (rust-analyzer's `(T, Vec<Error>)`).
- **Testing**: the pyramid of section 3, the system boundaries tests concentrate on (`Core::step`, each sidecar's MCP interface, the end-to-end event trace), and the slow-test variable (`EILEEN_SLOW_TESTS=1`).
- **Observability and debugging**: JSON log lines per process with file and line; the flight recorder (last minutes of inputs, effects and spans, dumped on panic, on a `never!`, or on request); `eileen debug dump` and replay; Perfetto-compatible trace export of one request's spans (the same viewer nextest uses for test runs [read]).
- **Configuration**: TOML files with comments; a JSON Schema generated from the config types for editor completion and a commented default file generated from their doc comments (the pattern codex and ruff use for their own config [read]); `eileen config explain` shows each effective value and where it came from.
- **Serialization and stability**: rust-analyzer's lesson that a format becomes stable when its first user appears [read]: the skill file format and the event log carry a version from the first commit; snapshot tests make any change a reviewed diff.
- **Generated files**: committed, regenerated by `cargo xtask codegen`, checked fresh by a test (rust-analyzer's approach [read]): `CODEMAP.md`, config schema and default file, contract schemas.
- **Performance budgets**: the latency budget from the voice report [prior]; idle CPU and memory targets; where they are measured.
- **Platform boundaries**: which crates are platform-neutral (built and tested on Linux in CI) and which are Windows-only.

### 5.3 Generated code map and event map

`docs/CODEMAP.md`, written by `cargo xtask codegen` through a separate `tools/codemap` package (so `xtask` itself stays std-only and quick to build) and checked fresh by a test in the Linux hygiene job [inferred]:

- **Crate graph** from `cargo metadata --no-deps` as a Mermaid diagram, with platform-neutral and Windows-only crates marked.
- **Module tree per crate** with line counts and the location of every `ffi` module, parsed with `syn` (no build needed, and it sees `cfg(windows)` modules on Linux, which cargo-modules would not).
- **Event map**: for every `BusEvent` and `Input` variant, the functions that construct it and the match arms that handle it, found with a `syn` visitor. In an event-driven core this table is the call graph that matters: it answers "who sends this, who reacts to it" in one place, for people and for agents.
- **Binaries and their processes**, with the transport each uses.

Two heavier aids are optional: a nightly `rust-analyzer scip` index uploaded as an artifact for code search tools, and the official `rust-analyzer-lsp` Claude Code plugin for definition, references and call hierarchy, enabled for one agent at a time on the owner's PC because of memory [read, inferred].

### 5.4 Tracing a bug from symptom to code

The playbook `ARCHITECTURE.md` should end with [inferred]:

1. The user says "Eileen, that went wrong", presses the dump hotkey, or a `never!` fires: the flight recorder writes `%LOCALAPPDATA%\eileen\dumps\<time>-<request id>\` (event log, spans, process table, versions, config hash). Dumps stay on the PC; a dump attached to an issue goes through the redaction module first.
2. Open the request's spans in Perfetto or filter the event log by request ID; every line has its source file and line.
3. The event map in `CODEMAP.md` names the producer and handler of the first wrong event; the owning crate's `AGENTS.md` names its invariants and tests.
4. `cargo xtask sim --replay <dump>` reproduces logic bugs from the recorded inputs; `cargo xtask e2e --from-dump <dump>` rebuilds an end-to-end scenario for the rest.
5. The reproduction becomes a regression file or scenario before the fix.

## 6. CI on GitHub Actions

### 6.1 Workflows

| File | Trigger | Purpose |
|---|---|---|
| `ci.yml` | `pull_request`, `push` to `main` | The PR lane (section 6.2) |
| `nightly.yml` | `schedule` daily, `workflow_dispatch` | Long simulation, fuzzing, full end-to-end, soak, coverage, flake hunt |
| `weekly.yml` | `schedule` weekly | Mutation run, long soak, timings, hawk report, dependency update with cooling-off |
| `release.yml` | tag `v*`, `workflow_dispatch` | Release build and checks; a draft GitHub release the owner publishes |

All four use `permissions: contents: read` at the top (write scopes only where a job opens an issue or a release), `persist-credentials: false` on checkout, and actions pinned to commit SHAs with the version in a comment, as rust-analyzer, ruff, uv and TigerBeetle do [read]. zizmor checks every workflow change. Dependabot keeps the pinned SHAs current.

### 6.2 PR lane

| Job | Runner | Contents | Target | Timeout |
|---|---|---|---|---|
| `plan` | ubuntu | Changed paths decide which optional steps run (speech, audio, parsers, docs only) | under 1 min | 5 |
| `hygiene` | ubuntu | `cargo fmt --check`, typos, cargo-deny, cargo-machete, zizmor, `cargo test -p xtask` (tidy), code map freshness | 3 min | 10 |
| `logic-linux` | ubuntu | clippy and nextest for platform-neutral crates; simulator PR seeds and regressions; exhaustive depth 6; fuzz corpus replay; `cargo fuzz build` if parsers changed; loom if `eileen-audio` changed | 8 min | 20 |
| `windows-clippy` | windows | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 6 min | 20 |
| `windows-test` | windows | `cargo nextest run --workspace --locked --profile ci`, doc tests, `cargo xtask e2e --suite smoke` with stubs; JUnit and pending snapshots uploaded on failure | 12 min | 25 |
| `mutants-diff` | ubuntu | `cargo mutants --in-diff` on logic crates, `continue-on-error` at first | 10 min | 20 |
| `all-green` | ubuntu | Fails unless every needed job succeeded or was skipped | under 1 min | 5 |

The `all-green` job is the single required status check, the pattern rust-analyzer calls `conclusion` and uv calls `required-checks-passed`: a skipped job counts as success to GitHub, so the aggregate job has to run with `if: always()` and inspect the results itself [read].

Skeleton (to be completed and run by WP0):

```yaml
name: CI
on:
  pull_request:
  push:
    branches: [main]
permissions:
  contents: read
concurrency:
  group: ci-${{ github.event.pull_request.number || github.sha }}
  cancel-in-progress: true
env:
  CARGO_INCREMENTAL: 0
  CARGO_NET_RETRY: 10
  CARGO_TERM_COLOR: always
  CARGO_PROFILE_DEV_DEBUG: line-tables-only
  RUST_BACKTRACE: 1
  NEXTEST_PROFILE: ci
defaults:
  run:
    shell: bash
jobs:
  plan:
    runs-on: ubuntu-latest
    timeout-minutes: 5
    outputs:
      parsers: ${{ steps.filter.outputs.parsers }}
      audio: ${{ steps.filter.outputs.audio }}
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: dorny/paths-filter@fbd0ab8f3e69293af611ebaee6363fc25e6d187d # v4.0.1
        id: filter
        with:
          filters: |
            parsers: ['crates/eileen-contracts/**', 'crates/eileen-harness/**', 'crates/eileen-mcp/**', 'crates/eileen-config/**', 'fuzz/**', 'Cargo.lock']
            audio: ['crates/eileen-audio/**']

  windows-test:
    needs: plan
    runs-on: windows-latest
    timeout-minutes: 25
    env:
      CARGO_TARGET_DIR: D:\target    # uv: D: on standard runners is as fast as a Dev Drive; measure it here
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - run: rustup show               # the toolchain pinned in rust-toolchain.toml, as ruff's CI does
      - uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3
        with:
          shared-key: windows-test
          save-if: ${{ github.ref == 'refs/heads/main' }}
      - uses: taiki-e/install-action@8cca0918f8471c33515202b73f56ab27f2bed283 # v2.87.29
        with:
          tool: cargo-nextest@0.9.148
      - run: cargo nextest run --workspace --locked
      - run: cargo test --workspace --doc --locked
      - run: cargo xtask e2e --suite smoke
      - if: failure()
        uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: windows-test-failure
          path: |
            ${{ env.CARGO_TARGET_DIR }}/nextest/ci/junit.xml
            **/*.snap.new

  all-green:
    needs: [plan, hygiene, logic-linux, windows-clippy, windows-test]
    if: always()
    runs-on: ubuntu-latest
    timeout-minutes: 5
    env:
      NEEDS_JSON: ${{ toJson(needs) }}   # through the environment, so zizmor sees no template in the script
    steps:
      - run: jq --exit-status 'all(.result == "success" or .result == "skipped")' <<< "$NEEDS_JSON"
```

Other pinned actions resolved on 2026-10-20 [read]: `EmbarkStudios/cargo-deny-action` v2.1.1 is commit `3c6349835b2b7b196a839186cb8b78e02f7b5f25`, `crate-ci/typos` v1.51.1 is `7f9d0819ca1c008ade73f4b41fa9d258d48f605b`, `zizmorcore/zizmor-action` v0.7.1 is `85b2b9fb8e3325e7a23d7d0a9d7ba5ae0f999e3e`. Check `rust-cache`'s handling of a target directory outside the workspace (its `workspaces` input) when trying `D:`.

`.config/nextest.toml` additions on top of the Rust report's profiles:

```toml
[test-groups]
desktop = { max-threads = 1 }      # tests that need the foreground window or real UIA

[[profile.default.overrides]]
filter = "test(/^desktop::/) | package(eileen-desktop) | package(eileen-apps)"
test-group = "desktop"

[profile.default]
leak-timeout = { period = "500ms", result = "fail" }   # a test that leaves a child process running fails

[profile.nightly]
retries = 2
flaky-result = "fail"              # nightly lists flaky tests instead of hiding them
fail-fast = false
```

### 6.3 Caching and supply chain

- Swatinem/rust-cache with a `shared-key` per job type (clippy artifacts and test artifacts differ, so they cannot share), saved only from `main` (`save-if`), as Turso and ruff do: pull requests read `main`'s caches and do not spend the repository's 10 GB cache allowance [read].
- No sccache in CI; rust-cache covers dependencies. sccache remains the local recommendation from the Rust report [prior].
- Tools installed prebuilt with `taiki-e/install-action` at exact versions; never `cargo install` from source in CI either.
- New crate versions wait 14 days before adoption unless they fix a bug Eileen hits (wezterm's `cargo cooldown`, and the policy this repository's own tooling already follows for scripts). The weekly dependency job opens one PR; protocol crates (rmcp, agent-client-protocol) are updated by hand with the probe subcommands as smoke tests, since they ship weekly [read, prior].
- Release artifacts get checksums; signing is a later decision.

### 6.4 Keeping Windows under about 15 minutes

No Rust CI run of the new workspace has happened yet, so these are estimates to measure in WP0 [inferred]. A clean dev build of the skeleton took 242 seconds on the owner's 22-thread PC under load [prior]; a 4-vCPU runner is slower, so a cold cache probably costs 10 or more minutes per Windows job, and a warm one a few minutes. In order of effect:

1. **Move everything platform-neutral to Linux.** Simulation, fuzzing, exhaustive tests, mutation testing and most unit tests run on `ubuntu-latest`; Windows runs clippy, tests and smoke scenarios. This also enforces the Rust report's rule that logic crates carry no Win32 types.
2. **Clippy and tests in parallel jobs.** Public repositories get standard runners free and without a minute limit [read], so a second runner costs nothing.
3. **Warm caches from `main`**, keyed on `Cargo.lock` and the toolchain. A run after a lockfile change is the exception that may take 20 minutes; the timeouts allow it.
4. **One integration test binary per crate** (`tests/it/main.rs`), because each test file is a separate link and MSVC linking is slow [read matklad, inferred].
5. **Small debug info**: `line-tables-only` for our crates and none for dependencies in CI.
6. **Build target on `D:`**, uv's finding for standard runners; keep it only if it saves a minute or more.
7. **Doc tests only where they document**: each doc test is its own binary; run them on Linux for platform-neutral crates if Windows time is tight (cargo-mutants' guide makes the same point about their cost) [read].
8. **Skip the speech crate's native downloads on PRs that do not touch it**, through the plan job, while `main` and nightly always build it.
9. **Partition tests only when they need it**: nextest `--partition hash:1/2` and `2/2` once the test run alone passes five minutes; an archive (`cargo nextest archive`) avoids building twice.
10. **Measure before tuning**: the weekly job uploads `cargo build --timings` from Windows; `rust-lld` as the MSVC linker is worth one measured try.

### 6.5 Nightly and weekly lanes

| Job | Runner | Content | Timeout |
|---|---|---|---|
| `sim-random` | ubuntu, matrix of 4 profiles | New random seeds, 30 minutes each; failures minimised and uploaded | 45 |
| `sim-windows` | windows | One profile, 20 minutes, to catch platform differences | 30 |
| `fuzz` | ubuntu, matrix by target group | 10 minutes per target; new corpus entries uploaded; crashes minimised | 60 |
| `e2e-full` | windows | Every scenario not marked `requires` | 45 |
| `soak` | windows | 30 minutes (4 hours weekly) | 60 (300 weekly) |
| `flake-hunt` | windows, ubuntu | nextest `--profile nightly`; `--stress-count 20` on tests tagged as timing-sensitive | 45 |
| `concurrency` | ubuntu | shuttle and loom suites in full | 30 |
| `coverage` | ubuntu, windows | `cargo llvm-cov nextest`, HTML artifact | 45 |
| `release-smoke` | windows | Thin-LTO `release` build, `eileen --version`, an MCP probe round trip | 30 |
| `report` | ubuntu | Opens or updates one issue per failing job, labelled `nightly`; main-branch failures of ordinary tests go to a `flake` issue, matklad's list of flakes on main | 10 |

Ruff's daily fuzz job writes its issue body from the fuzzer's output; Eileen's report job does the same with the replay command at the top [read].

### 6.6 Release lane

On a `v*` tag: the PR and nightly lanes on the tagged commit; the `dist` profile (fat LTO, one codegen unit) built on `windows-latest`, which has the memory the owner's PC lacks for it [prior]; end-to-end smoke from a fresh runner using the built artifacts; a licence listing from cargo-deny plus the licences of downloaded native libraries (ONNX Runtime), which cargo-deny cannot see [prior]; `.pdb` files uploaded beside the binaries; a draft release. The owner runs the local A/B of B1 to B8 against the previous release and publishes the release himself, since publishing is a consequential action.

### 6.7 The owner's PC lane

`cargo xtask nightly-local`, started by Windows Task Scheduler in the small hours if the owner opts in, with keep-awake [inferred]:

- Skips if free memory is under 2 GB or the owner used the PC in the last 30 minutes; takes a build slot like any build.
- Runs B1 to B8 with real Claude within a nightly turn budget the owner sets, since workers share his plan limits with claude.ai and the desktop app [prior]; runs the stream-json contract test; once a week, the overnight soak.
- Appends results to the devhub store and opens an issue on regressions.
- Never a GitHub self-hosted runner: on a public repository a fork's pull request could run code on the owner's PC [read, secondary]. The lane pulls `main`; nothing pushes code to it.

### 6.8 Branch protection and continuous pushing

The owner direction asks that every finished piece land on its own branch and pull request and merge only with green checks. Today `main` has no protection, no rulesets, and auto-merge and branch deletion are off [read, GitHub API]. Settings changes are the owner's to make:

1. A ruleset on `main`: pull request required, `all-green` required, no force pushes, squash merges.
2. Allow auto-merge, so an agent can open a PR, enable auto-merge and move on; delete branches after merge.
3. **Merge queue**: GitHub offers it only for organisation-owned public repositories [read]. Without it, two PRs can each pass alone and break `main` together. The choices:
   - Move the repository to a free GitHub organisation (old URLs redirect [inferred]) and enable the queue, adding `merge_group` to `ci.yml`. TigerBeetle relies on its queue so that every commit on `main` passes the tests [read]. Note that rust-analyzer found rust-cache unhelpful under merge groups [read].
   - Stay user-owned and require branches to be up to date before merging: always correct, but parallel agents rebase each other repeatedly.
   - Stay user-owned without that requirement, and rely on the `main` push run plus nightly to catch the rare semantic conflict quickly.
   Recommendation: the organisation move, because the build plan runs several agents in parallel; until then, the third option with a prompt revert rule [inferred].
4. Agents push early as draft PRs (CI runs on drafts), mark them ready when green, and keep PRs small enough to review.

## 7. Engineering style guide

Short rules; each names where it comes from. These complement, not repeat, the lint table of the Rust report.

**Assertions**

1. Assert preconditions, postconditions and invariants of the core state machines, both what must hold and what must never happen. Pair them across a boundary: assert a record before writing it and after reading it back. (TigerStyle)
2. One condition per assertion, always with a message. (TigerStyle; `missing_assert_message` is already on)
3. Relationships between constants are compile-time assertions, for example `const _: () = assert!(FRAME_SAMPLES == SAMPLE_RATE_HZ / 1000 * FRAME_MS);`. (TigerStyle)
4. Two strengths. In the always-on core, `never!` and `always!` log at error level, count, dump the flight recorder and return a safe fallback, and panic under tests and the simulator; the microphone loop must survive. In sidecars, the simulator and tests, plain `assert!`: a crash there is cheap, because the supervisor restarts the process. (rust-analyzer, TigerStyle)
5. Types before assertions: make illegal states unrepresentable with enums and newtypes, then assert what types cannot say. (Turso, ruff)

**Limits**

6. Every queue, channel, buffer, retry loop and message read has a stated maximum; every wait on another process has a deadline. Add `tokio::sync::mpsc::unbounded_channel` to `disallowed-methods`. Line-protocol readers cap line length before parsing. (TigerStyle)
7. A loop that is meant never to end says so, and asserts its exit path. (TigerStyle)

**Time, randomness and concurrency**

8. Core logic gets time and randomness as arguments. `std::time::Instant::now`, `std::time::SystemTime::now` and the `rand` thread generator are in `disallowed-methods`; the one real clock and RNG carry an `#[expect]` with a reason. (VOPR, FoundationDB)
9. No detached tasks. Every spawned task belongs to a `JoinSet` or `TaskTracker` that someone awaits or cancels; a function that starts work returns its handle. Put `tokio::spawn` and `tokio::task::spawn` in `disallowed-methods`. (matklad, Zed)
10. Ordered collections wherever iteration order can reach output; `HashMap` only for lookup. (determinism)
11. Push `if`s up and `for`s down: one function holds a state machine's control flow; helpers compute and do not decide. (TigerStyle, matklad)

**Errors**

12. Operating errors (a worker exits, a pipe closes, UIA times out) are values and are handled; programmer errors are assertions. Never `let _ =` on a `Result`. (TigerStyle, Zed)
13. A builder that can fail per item returns its result with a list of diagnostics, so one unreadable window does not lose the whole DesktopState. (rust-analyzer)
14. Error enums carry what a reader needs to act: which job, which child, which deadline. (Rust report)

**Naming**

15. Full words (`version`, not `ver`). Units and qualifiers last, most significant first: `barge_in_stop_ms_max`, `queue_depth_max`. In code prefer `Duration`; in config keys and wire fields use the `_ms` suffix. (TigerStyle, uv)
16. Nouns for things and state; events in the past tense (`JobStarted`), commands in the imperative (`StartJob`). A helper used by one function carries its name (`spawn_worker`, `spawn_worker_exited`). Related names of equal length line up (`source`, `target`). (TigerStyle)
17. One name per concept across code, logs, HUD and docs; the glossary lives in `ARCHITECTURE.md`. (TigerStyle)

**Size**

18. Functions at most 80 lines (clippy's `too_many_lines`, already set; TigerBeetle uses 70). Modules aim under 500 lines without tests; past 800, new code goes in a new module unless tidy's allow list says why. (codex, TigerStyle)
19. At most six arguments (already set); two arguments of the same type become an options struct or newtypes; no bare `bool` arguments at call sites, use enums. (TigerStyle, codex)

**Comments and docs**

20. Comments say why and state invariants; they never narrate the code or its history. Every module starts with a `//!` paragraph that explains it top down. Comments are sentences. (TigerStyle, ruff, uv, Biome, codex)
21. Write for a reader at the current commit who never saw the session. (uv, ruff, Biome)
22. Docs are plain and editorial, with no em dashes and no marketing words; the product is only ever Eileen; tidy enforces the mechanical parts. One sentence per line in Markdown keeps diffs readable. (owner, rust-analyzer)

**Tests**

23. Test features at a boundary through a `check` helper, so the test body is data and refactors touch one function. No `#[ignore]`, no `#[should_panic]`, no sleeps. (rust-analyzer, matklad)
24. A fix starts with a test that fails without it. (Turso, Biome)
25. Snapshot changes are reviewed as behaviour changes; orphaned snapshots are rejected. (ruff, Biome)

**Dependencies, visibility, commits**

26. A new dependency, a new cross-crate `pub` item or a new crate is an architectural change and says so. Prefer std and existing dependencies; exact versions; lockfile changes with `cargo update --precise`. (rust-analyzer, uv, nushell)
27. `pub(crate)` by default; `unreachable_pub` warns; the weekly cargo-hawk report lists `pub` items nobody uses. (ruff)
28. Commit messages say why; a PR description is not in `git blame` and is no substitute. Logic PRs stay under about 500 changed lines. (TigerStyle, codex)

## 8. Changes for BUILD.md and the work packages

1. **WP0 builds the test and guidance scaffolding, not only the workspace**: `eileen-stdx` (`MonoTime`, `never!`, `always!`, `buggify!`), the sans-IO `Core::step` with closed `Input` and `Effect` enums, the injected clock and RNG, the `eileen-sim` skeleton with three invariants and the PR seed set, `xtask` subcommands (`check`, `test`, `clippy`, `ci`, `pre-push`, `sim`, `fuzz-replay`, `e2e`, `codegen`, `tidy`, `soak`, `bench`, `nightly-local`, `setup`), the tidy suite, `fuzz/` with the first two targets, and `.config/nextest.toml` with the `desktop` group, leak failures and a nightly profile.
2. **WP0 writes the guidance files**: root `AGENTS.md` and `CLAUDE.md` (section 4), an `AGENTS.md` and `CLAUDE.md` pair in every crate it creates, `.claude/settings.json`, `.claude/rules/ffi.md`, the four skills, and `docs/wp/` files for every work package.
3. **WP0 writes the root `ARCHITECTURE.md`** from section 5.2 and moves `docs/ARCHITECTURE.md` to `docs/design/2026-09-design-spec.md`, with a decision record.
4. **WP0 replaces CI**: `ci.yml` per section 6.2, plus `nightly.yml`, `weekly.yml` and `release.yml` skeletons, actions pinned by SHA, zizmor, Dependabot for actions. The Python jobs go when BUILD.md retires or keeps the scaffold.
5. **Add to `clippy.toml`'s `disallowed-methods`**: `Instant::now`, `SystemTime::now`, the `rand` thread generator, `tokio::sync::mpsc::unbounded_channel`, `tokio::spawn`, `tokio::task::spawn`.
6. **Define one event log format** in `eileen-contracts` (versioned, newline-delimited), used by the flight recorder, the simulator, the end-to-end harness and the learning loop's job record; the learning-loop work package records jobs in it rather than a separate schema.
7. **Request IDs from the first commit**: created at the start of each request, on every bus event, span and record; `traceparent` in MCP `_meta`; the job ID in each worker's MCP configuration.
8. **The audio work package defines `AudioSource` and `AudioSink`** with WAV and capture implementations and the room mixer, and the hidden `--audio-source` and `--audio-sink` flags.
9. **The harness work package ships `eileen-stub-harness`** and the first recorded scripts, and the weekly contract test against the real CLI.
10. **Every work package's definition of done** includes: tests at the right layer; a fuzz target for any new parser of untrusted input; a simulator invariant or scenario for any new core behaviour; the crate's `AGENTS.md` and the code map current; the work package file updated.
11. **A new work package for the end-to-end harness and timed scenarios** (`eileen-e2e`, the scenario format and schema, B1 to B8, the devhub store, A/B comparison, the owner's PC lane).
12. **`tools/codemap`** generates `docs/CODEMAP.md` with the crate graph, module trees and the event map; a freshness test runs in the hygiene job.
13. **Configuration generation**: JSON Schema and a commented default config generated from the config types, checked fresh by a test; `eileen config explain`.
14. **Rewrite `CONTRIBUTING.md`, `docs/DEV-SETUP.md` and the PR template for Rust**: they still describe pytest, ruff and the Python layout. The PR template gains "Tests at which layer", "Seen failing before the fix" and "Suggested AGENTS.md changes".
15. **Reconcile the learning crate's name**: the Rust report calls it `eileen-skills`, the learning-loop report `eileen-learn`. BUILD.md picks one.
16. **Owner decisions to list in BUILD.md**: the `main` ruleset and auto-merge; moving the repository to an organisation for the merge queue; where the devhub store lives; opting in to the owner's PC nightly lane, its turn budget and the Task Scheduler entry; recording his own voice for the scenario corpus.
17. **Branch naming** `wp<n>/<topic>`, draft PRs from the first push, auto-merge on green.

## 9. Open risks

- **Windows CI time is unmeasured.** The 15-minute target rests on estimates; the first real runs decide whether tests need partitioning or the plan job needs to skip more.
- **Hosted Windows runners are reported, not proven, to give UI Automation an interactive desktop** [secondary], and they have no audio device. If desktop tests fail there, they move to the owner's PC lane and CI keeps only audio-injected scenarios.
- **Simulator fidelity.** Fake workers and sidecars encode today's understanding of `claude -p` and UIA. The stream-json control messages are not a documented public API [prior]; the weekly contract test is the only defence against drift.
- **Determinism leaks**: unordered maps, the clock, tokio scheduling in the drivers, and feature unification changing serialization (the skeleton already hit `serde_json/preserve_order` [prior]). `--doublecheck` catches them but someone has to fix them.
- **Seeds go stale across commits**; only decoded scripts are durable regressions, and a generator change can still make an old script meaningless. Keep the scripts close to `Input`.
- **Fuzzing is Linux-only.** Windows-specific code paths (COM string conversion, named pipe framing) are covered only through their platform-neutral parsers and the replay tests.
- **The owner's PC lanes compete with his own work and his plan limits.** The turn budget and idle checks reduce but do not remove that.
- **No merge queue while the repository is user-owned.** Parallel agent PRs can break `main` together.
- **Guidance files rot.** Zed's warning applies to the per-crate files the owner asked for; tidy and `/doctor prompt-audit` limit the damage. codex removed its root file in September 2026 without saying why.
- **Young tools**: cargo-hawk is experimental and was written by Codex; cargo-cooldown is new; nextest's record and replay and its benchmark runner are experimental. None of them should gate a merge.
- **Memory on the owner's PC.** rust-analyzer per agent session, a simulator run beside a build, or mutation testing locally can each repeat the 2026-10-20 out-of-memory session end [prior]; all heavy lanes belong in CI.
- **shuttle 0.9.8 was released the day this was written**; the cooling-off policy says to start from 0.9.7 or wait.

## 10. Sources

Read on 2026-10-20. Repository files at their default branch on that date unless a commit is named.

**Architecture and style**

- rust-analyzer architecture: https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md
- rust-analyzer style: https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/style.md
- rust-analyzer `CLAUDE.md`: https://github.com/rust-lang/rust-analyzer/blob/master/CLAUDE.md
- rust-analyzer CI: https://github.com/rust-lang/rust-analyzer/blob/master/.github/workflows/ci.yaml and tidy: https://github.com/rust-lang/rust-analyzer/blob/master/xtask/src/tidy.rs; rust-cache removal: commit `b2801f9f` (PR #19493)
- matklad, ARCHITECTURE.md: https://matklad.github.io/2021/02/06/ARCHITECTURE.md.html
- matklad, How to Test: https://matklad.github.io/2021/05/31/how-to-test.html
- matklad, Delete Cargo Integration Tests: https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html
- matklad, Large Rust Workspaces: https://matklad.github.io/2021/08/22/large-rust-workspaces.html
- matklad, Properly Testing Concurrent Data Structures: https://matklad.github.io/2024/07/05/properly-testing-concurrent-data-structures.html
- matklad, Test Case Minimization: https://matklad.github.io/2026/04/20/test-case-minimization.html
- matklad, Software Architecture: https://matklad.github.io/2026/05/12/software-architecture.html
- matklad, Catch Flakes On Main: https://matklad.github.io/2026/05/14/catch-flakes-on-main.html
- matklad, Finding Bugs: https://matklad.github.io/2026/09/19/finding-bugs.html
- matklad, Benchmark In Milliseconds: https://matklad.github.io/2026/10/05/benchmark-milliseconds.html
- matklad, CI In a Box: https://matklad.github.io/2026/02/06/ci-in-a-box.html (post sources: https://github.com/matklad/matklad.github.io/tree/master/content/posts)
- TigerStyle: https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md
- TigerBeetle architecture: https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/ARCHITECTURE.md
- TigerBeetle VOPR and testing: https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/vopr.md and https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/testing.md
- TigerBeetle hacking guide: https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/internals/HACKING.md; devhub: https://github.com/tigerbeetle/tigerbeetle/blob/main/src/scripts/devhub.zig; CI retirement: commit `1116a844`, PR #3937
- Helix architecture: https://github.com/helix-editor/helix/blob/master/docs/architecture.md

**Simulation and concurrency**

- FoundationDB testing: https://apple.github.io/foundationdb/testing.html; `Buggify.h`: https://github.com/apple/foundationdb/blob/main/flow/include/flow/Buggify.h
- Turso simulator: https://github.com/tursodatabase/turso/blob/main/testing/simulator/README.md
- Turso concurrent simulator: https://github.com/tursodatabase/turso/blob/main/testing/concurrent-simulator/README.md
- Turso yield injection: https://github.com/tursodatabase/turso/blob/main/.claude/skills/yield-injections/SKILL.md
- Turso Rust CI: https://github.com/tursodatabase/turso/blob/main/.github/workflows/rust.yml
- loom: https://github.com/tokio-rs/loom; tokio loom CI: https://github.com/tokio-rs/tokio/blob/master/.github/workflows/loom.yml; tokio runtime builder: https://github.com/tokio-rs/tokio/blob/master/tokio/src/runtime/builder.rs
- shuttle: https://github.com/awslabs/shuttle and its wrappers: https://github.com/awslabs/shuttle/blob/main/wrappers/README.md
- turmoil: https://github.com/tokio-rs/turmoil
- madsim: https://github.com/madsim-rs/madsim

**Agent guidance**

- Claude Code memory and AGENTS.md: https://code.claude.com/docs/en/memory
- Codex AGENTS.md guide: https://learn.chatgpt.com/docs/agent-configuration/agents-md (redirected from https://developers.openai.com/codex/guides/agents-md)
- AGENTS.md and the Agentic AI Foundation [secondary]: https://openai.com/index/agentic-ai-foundation/ and https://agentic-ai.readthedocs.io/en/latest/Standards/agents-md/
- OpenAI cookbook, PLANS.md [secondary]: https://developers.openai.com/cookbook/articles/codex_exec_plans
- uv `AGENTS.md`: https://github.com/astral-sh/uv/blob/main/AGENTS.md; `STYLE.md`: https://github.com/astral-sh/uv/blob/main/STYLE.md
- ruff `AGENTS.md`: https://github.com/astral-sh/ruff/blob/main/AGENTS.md; `.claude/settings.json`: https://github.com/astral-sh/ruff/blob/main/.claude/settings.json
- Zed `.rules`: https://github.com/zed-industries/zed/blob/main/.rules
- Turso `AGENTS.md`: https://github.com/tursodatabase/turso/blob/main/AGENTS.md; agent guides: https://github.com/tursodatabase/turso/tree/main/docs/agent-guides
- Biome `AGENTS.md`: https://github.com/biomejs/biome/blob/main/AGENTS.md; skills catalogue: https://github.com/biomejs/biome/blob/main/.agents/skills/README.md
- Deno `CLAUDE.md`: https://github.com/denoland/deno/blob/main/CLAUDE.md
- nushell `AGENTS.md`: https://github.com/nushell/nushell/blob/main/AGENTS.md
- codex per-directory files: https://github.com/openai/codex/blob/main/codex-rs/exec-server/src/sandbox_integrity/AGENTS.md and https://github.com/openai/codex/blob/main/codex-rs/tui/src/bottom_pane/AGENTS.md; root file removal: commit `18131270` (#49713); the removed file at its parent commit: https://github.com/openai/codex/blob/5e96aabd68dc6194f5ad928cb7f5d0bc71b72df9/AGENTS.md
- Official Claude Code LSP plugin: https://github.com/anthropics/claude-plugins-official/tree/main/plugins/rust-analyzer-lsp

**Testing tools**

- cargo-nextest docs (stress tests, record and replay, Perfetto, test groups, leaky tests, retries, partitioning, archiving, benchmarks, setup scripts): https://github.com/nextest-rs/nextest/tree/main/site/src/docs
- insta: https://github.com/mitsuhiko/insta (changelog and `insta/src/lib.rs`)
- cargo-fuzz: https://github.com/rust-fuzz/cargo-fuzz (README platform note)
- cargo-mutants book (PR diffs, sharding, performance, coverage comparison): https://github.com/sourcefrog/cargo-mutants/tree/main/book/src and the in-diff workflow: https://github.com/sourcefrog/cargo-mutants/blob/main/examples/workflows/in-diff.yml
- cargo-llvm-cov: https://github.com/taiki-e/cargo-llvm-cov
- cargo-hawk: https://github.com/astral-sh/hawk
- cargo-modules: https://github.com/regexident/cargo-modules
- rust-analyzer CLI (`scip`): https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/cli/flags.rs
- Firecracker integration tests and A/B testing: https://github.com/firecracker-microvm/firecracker/blob/main/tests/README.md
- crates.io API for versions and dates: `https://crates.io/api/v1/crates/<name>` (turmoil, shuttle, shuttle-tokio, loom, madsim, proptest, proptest-state-machine, insta, arbitrary, libfuzzer-sys, bolero, cargo-fuzz, cargo-mutants, cargo-llvm-cov, cargo-semver-checks, criterion, divan, codspeed-criterion-compat, cargo-nextest, expect-test, cov-mark, cargo-hawk, cargo-modules, cargo-cooldown)

**CI**

- ruff CI and daily fuzz: https://github.com/astral-sh/ruff/blob/main/.github/workflows/ci.yaml and https://github.com/astral-sh/ruff/blob/main/.github/workflows/daily_fuzz.yaml
- uv CI, tests and Dev Drive script: https://github.com/astral-sh/uv/blob/main/.github/workflows/ci.yml, https://github.com/astral-sh/uv/blob/main/.github/workflows/test.yml, https://github.com/astral-sh/uv/blob/main/.github/workflows/setup-dev-drive.ps1, https://github.com/astral-sh/uv/blob/main/.github/workflows/check-zizmor.yml
- wezterm cooldown: https://github.com/wezterm/wezterm/blob/main/.github/workflows/cargo-cooldown.yml and https://github.com/wezterm/wezterm/blob/main/cooldown.toml
- GitHub-hosted runners: https://docs.github.com/en/actions/reference/runners/github-hosted-runners
- GitHub Actions limits: https://docs.github.com/en/actions/reference/limits
- Merge queue: https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue and its availability note: https://github.com/github/docs/blob/main/data/reusables/gated-features/merge-queue.md
- Self-hosted runners on public repositories [secondary]: https://docs.github.com/en/enterprise-server@3.5/admin/github-actions/getting-started-with-github-actions-for-your-enterprise/getting-started-with-self-hosted-runners-for-your-enterprise
- Windows runners and UI Automation [secondary]: https://tessl.io/registry/testland/flaui-tests/files/references/flaui-api.md
- MCP SEP-414, trace context in `_meta`: https://modelcontextprotocol.io/seps/414-request-meta; Claude trace forwarding request [secondary]: https://claudeissues.com/issue/76391-feature-propagate-cowork-claude-session-trace-context-prompt-id-into-mcp-tool-ca
- zizmor: https://github.com/zizmorcore/zizmor and https://github.com/zizmorcore/zizmor-action
- Eileen repository state (owner type, protection, merge settings): GitHub REST API, `repos/Sixteen1-6/eileen` and `branches/main/protection`
