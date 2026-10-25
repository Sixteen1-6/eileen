# fuzz

The cargo-fuzz workspace: one target for every parser of untrusted input (BUILD.md section 4.7).
It is a workspace of its own, which the root workspace excludes. Read `fuzz/README.md` first; this
file adds the rules only this folder has.

## Why this folder has its own rules

Fuzzing needs a nightly toolchain and cargo-fuzz, which runs only on Unix-like systems, so this
folder builds with another toolchain, on Linux only, outside the root workspace and its gates.

## Rules

- The nightly is pinned by date in `fuzz/rust-toolchain.toml`. Build from a copy of the root
  lockfile (`cp Cargo.lock fuzz/`), so the targets build the versions the root workspace tests;
  the copy is not committed.
- Each target calls `fuzz_entry::<target>` in the neutral crate that owns the parser and fails only
  when that function reports a broken invariant. The entry never panics itself, because every
  Eileen binary links the crate.
- `corpus/<target>/` holds a small, size-capped seed corpus. The owning crate's
  `tests/it/fuzz_corpus.rs` replays it on every OS (on Linux only for the `-linux` crates'
  targets).
- A crash counts as fixed only once its minimised input is in `regressions/<target>/` and a
  focused regression test sits beside the fix.
- A target belongs to the package that owns its parser. WP10 adds the three Linux targets with
  its crates; WP0 does not declare them.
- `cargo xtask fuzz` and `cargo xtask fuzz-replay` wrap this once they exist; until then run
  `cargo fuzz` from this folder on Linux.
