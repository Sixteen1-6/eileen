# Fuzzing

Every parser of untrusted input in Eileen has a fuzz target here (BUILD.md section 4.7). This
folder is a workspace of its own, as cargo-fuzz lays it out, so the root workspace never builds
it.

## How a target is put together

- `fuzz_targets/<target>.rs` calls `fuzz_entry::<target>` in the neutral crate that owns the
  parser, and fails when that function returns `Err` naming a broken invariant. The entry
  never panics itself, because every Eileen binary links the crate.
- `corpus/<target>/` holds a small, size-capped seed corpus, committed. The owning crate's
  `tests/it/fuzz_corpus.rs` replays it through the same `fuzz_entry` function as an ordinary
  test on every OS (on Linux only for the `-linux` crates' targets).
- `regressions/<target>/` holds minimised crashes. A crash counts as fixed only once its input
  is here and a focused regression test sits beside the fix.

Most targets start as stubs that accept every input: WP0 declared them so each owning package
fills in its parser, its invariants and its corpus. `uia_text` and `desktop_state`
(`eileen-contracts`) are real from the start, and so are the three WP10 added with its `-linux`
crates: `linux_mountinfo` and `linux_proc_status` (`eileen-shell-linux`) and `desktop_entry`
(`eileen-apps-linux`).

## Running

cargo-fuzz runs only on Unix-like systems and needs the nightly toolchain pinned in
`rust-toolchain.toml` here, so fuzzing runs on Linux (the CI runners, or WSL on a Windows PC).
From this folder:

```sh
cargo fuzz run uia_text -- -max_total_time=60
```

`cargo xtask fuzz` and `cargo xtask fuzz-replay` (section 4b.5) wrap this once they exist. The
nightly lane fuzzes every target, and `long.yml` runs one for longer on demand (the `fuzz-10m`
label).
