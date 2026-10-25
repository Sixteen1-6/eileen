# Contributing to Eileen

Eileen is a voice-first AI layer for Windows 11, with Linux following, MIT licensed and public.
This file covers how to get set up, how to pick work, how to send a change and the rules that do
not bend. Coding agents read [AGENTS.md](AGENTS.md), the short form for them; this file has the full rules.

## Two ways to help

- **Test it on your PC.** Use it for real, and when something works or breaks, say "save that as a
  test". You see exactly what would leave your PC, and you share it from your own GitHub account
  as a scenario pull request. The testers' guide, `TESTING.md`, lands with the first live slice.
- **Build it.** Set up with [docs/DEV-SETUP.md](docs/DEV-SETUP.md), then read
  [ARCHITECTURE.md](ARCHITECTURE.md), [docs/BUILD.md](docs/BUILD.md) section 0 and the work
  package you take in section 6, and the crate doc of every crate you change.

## Picking work

The v0.1 build is split into work packages, WP0 to WP10, in [docs/BUILD.md](docs/BUILD.md) section
6. Each has a living file in [docs/wp/](docs/wp/) with its goal, its acceptance list and its
progress. Work is also tracked as GitHub issues; comment on one to claim it, and say so if you
stop. A package's files are listed under "Owns" in its section; a change to another package's
merged files goes through an amendment pull request.

## Sending a change

1. Branch from `main` as `wp<n>/<topic>` (or `amend/<topic>` for an amendment). Open a draft pull
   request from the first push; CI runs on drafts.
2. Lead with evidence. The pull request template opens with `## Evidence`: the failing test or
   scenario that now passes, numbers before and after, or a replay. For a feature, fix or
   refactor, list at least one test or scenario on a `tests:` or `scenarios:` line, and every one
   you list must exist at the head. A fix adds or changes a regression.
3. Fill in `## Type` with exactly one of `feature`, `fix`, `refactor`, `docs`, `scenario`,
   `amendment` and `deps`. The type must fit the files: `docs` only for Markdown and `docs/`,
   `scenario` only under `scenarios/`.
4. The title is the squash commit's subject: imperative, capitalised, under 72 characters, no
   trailing period.
5. Run `cargo xtask pre-push` before you push, and `cargo xtask pr-meta --body-file <file>` on the
   description. Keep logic changes under about 500 lines and one topic per pull request.
6. A pull request merges only when `all-green` and `pr-meta` pass on a branch that is up to date
   with `main`. Merges are squashed, and history on `main` is linear.

Documents no package owns, [docs/BUILD.md](docs/BUILD.md), [docs/DECISIONS.md](docs/DECISIONS.md)
and [docs/OWNER-DIRECTION.md](docs/OWNER-DIRECTION.md), change only through an `amendment` pull
request that touches only them, and the owner reviews it. A change to the decision log also
updates [docs/PLAYBOOK.md](docs/PLAYBOOK.md), or says under "Playbook" why it is unaffected.

## The checks

Every convention is enforced by a check, and every check runs locally with one command:
`cargo xtask ci` runs what the pull request lane runs, and `cargo xtask lint-repo` runs the fast
repository checks in seconds. Each failure names its check, the file and line, the fix and the
command that reproduces it. The thirty checks are listed in [docs/BUILD.md](docs/BUILD.md) section
4b.3.

<!-- checks:start -->
WP0 phase B's `cargo xtask codegen` renders the check registry here (number, convention, how it
is checked, the local command and the CI job) and into `docs/reference/checks.md`. Until then, the
table in docs/BUILD.md section 4b.3 is the list.
<!-- checks:end -->

## Code conventions

| Topic | Rule |
|---|---|
| Language | Rust 1.99.0, edition 2024, pinned in `rust-toolchain.toml` |
| Layout | One workspace, flat `crates/*`; every third-party dependency declared once in the root `Cargo.toml` |
| Guidance | Each crate's rules are its crate doc, the `//!` comment at the top of `src/lib.rs`, with the headings of BUILD.md section 4b.4 |
| Formatting | `rustfmt.toml`; `cargo fmt --all` |
| Lints | The workspace lint table and `clippy.toml`; CI denies warnings. Suppress only with `#[expect(..., reason = "...")]` |
| Errors | Typed error enums in libraries; no `unwrap`, `expect` or `panic!` outside tests |
| Unsafe | Only in a crate's private `ffi` module, one operation per block, each with a safety comment |
| Cross-process types | Only `eileen-contracts` types cross a process, pipe, file or socket |
| Platform code | Windows code in `-win` crates and Windows target tables; neutral crates build and test on Linux |
| Tests | nextest; unit tests beside the code, one `tests/it` binary per crate, the simulator for core behaviour, insta snapshots, fuzz targets for untrusted parsers |

## Writing style for docs

Plain, editorial, confident. Short paragraphs and sentences. Use a table when it beats prose. No
em or en dashes: use commas, periods or parentheses. No emoji, and no marketing words. The product
is only ever "Eileen"; workers are named by route (Claude, Codex, Grok, DeepSeek). Relative links
must resolve. Summarise a research report and link to it rather than pasting it.

## Hard rules

These are not open for review discussion. A pull request that breaks one is closed.

1. Never commit or log credentials, tokens, API keys, personal paths or anything from a `.env`
   file. No API keys except DeepSeek's, and never in the repository.
2. Never add code that reads vendor credential files, proxies tokens or runs a login flow for the
   user. Users sign in to the official CLIs themselves, and Eileen spawns them unmodified.
3. Never modify, patch or rebrand a vendor binary or app.
4. Never bundle code under a license that conflicts with MIT; cargo-deny enforces the list. Talk
   to such tools over IPC or spawn them.
5. Never weaken a check, a lint or `deny.toml` without a decision record.

The reasons are in [docs/DECISIONS.md](docs/DECISIONS.md) and [docs/RESEARCH.md](docs/RESEARCH.md).
