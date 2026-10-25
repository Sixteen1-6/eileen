# Developer setup

This takes you from a fresh PC to a passing `cargo xtask ci`. Windows 11 is the full target; on
Linux the neutral crates and the `-linux` crates build and test, and the `-win` crates compile to
empty libraries. The versions here come from [BUILD.md](BUILD.md) section 4.1; when the two
disagree, BUILD.md wins.

## Prerequisites

| Tool | Version | Needed for |
|---|---|---|
| Windows 11, or Ubuntu 24.04 or later | current | Windows for everything; Linux for the neutral crates, the `-linux` crates and fuzzing |
| git | recent | Cloning, pull requests, the pre-push hook |
| rustup | current | The toolchain itself is pinned to 1.99.0 in `rust-toolchain.toml`; rustup installs it on first use |
| Visual Studio 2022 Build Tools, "Desktop development with C++" (MSVC 14.4x, Windows SDK 10.0.26100) | 17.14 | Windows: the MSVC target and the vendored C in SQLite, zstd and ring |
| A C compiler (`build-essential`) | distribution's | Linux: the same vendored C |
| cargo-binstall | 1.26.0, prebuilt | Installing the other tools prebuilt. GPL-3.0-only, a tool we run and never link |
| sccache | 0.18.0, prebuilt | Optional local compile cache |
| curl and tar | ship with Windows 11 | `cargo xtask fetch-native` |

Install cargo-binstall and sccache from their release pages as prebuilt binaries. Never `cargo
install` a tool from source: four tools built that way took 59 minutes on the owner's PC
(BUILD.md section 6.1 rule 10). If you use sccache, set `RUSTC_WRAPPER=sccache`,
`SCCACHE_DIR=%LOCALAPPDATA%\sccache` and `SCCACHE_CACHE_SIZE=8G` in your user environment; without
it, builds work the same, only slower. No Dev Drive is needed.

## Clone and set up

```powershell
git clone https://github.com/Sixteen1-6/eileen.git
cd eileen
cargo xtask setup
```

`cargo xtask setup` installs the pinned toolchain, installs cargo-nextest 0.9.148, cargo-deny
0.20.2, cargo-machete 0.9.2, cargo-insta 1.49.0 and typos 1.51.1 with cargo-binstall (each
project's own prebuilt release only, with no token lookup in credential files and no telemetry),
points git's `core.hooksPath` at `.githooks` so `cargo xtask pre-push` runs before every
push, and on Windows fetches and verifies the native libraries (below). It is safe to run again.

Then check the install:

```powershell
cargo xtask check -p eileen-contracts
cargo xtask test -p eileen-core
cargo xtask ci
```

`cargo xtask ci` runs what the pull request lane runs and lists, at the end, the CI-only steps it
skipped and why.

## The build guard

Every `cargo xtask` command that runs cargo takes a build slot first (BUILD.md section 6.1 rule
1). It admits a build by available memory, commit headroom and the paging rate, sets
`CARGO_BUILD_JOBS` from free memory, and runs cargo in a kill-on-close Job Object at below-normal
priority, so a killed command leaves no compiler running. When no slot comes within `--wait-max`
(30 minutes by default) it exits with code 75: wait and try again.

- `cargo xtask slots` lists the holders; `cargo xtask slots --kill-stale` frees the records of
  holders that are gone.
- Release builds, speech-model tests, soaks and benchmarks take both slots with `--exclusive`.
- A cold build can outlast a 10-minute foreground tool call. Agents start guarded builds in the
  background and wait for them.
- On the owner's PC, `cargo xtask slots --baseline` samples a typical working day and writes the
  thresholds the guard uses; until then the code defaults apply.

## Windows notes

- The workspace builds for `x86_64-pc-windows-msvc` with the static C runtime
  (`.cargo/config.toml`).
- `cargo xtask fetch-native` downloads the pinned sherpa-onnx archive into the machine folder,
  checks its size and SHA256, and unpacks it. Every cargo run through `cargo xtask` then gets
  `SHERPA_ONNX_ARCHIVE_DIR`, `ORT_LIB_LOCATION` and `ORT_PREFER_DYNAMIC_LINK`, so nothing
  downloads unchecked binaries, and test binaries load the right `onnxruntime.dll` from beside
  themselves (`System32` holds an older copy the loader would find first).
- Each worktree keeps its own target folder; delete it when the branch merges.
- Developer Mode is optional; symlink tests need it, and you turn it on yourself.

## Linux notes

- Linux never builds the Windows native libraries: they sit under Windows target tables.
- Fuzzing runs here, with the nightly pinned in `fuzz/rust-toolchain.toml` and cargo-fuzz 0.13.2
  (see `fuzz/README.md`).
- What the `-linux` crates' tests need (a D-Bus session, the desktop's services) is in
  [LINUX.md](LINUX.md) section 6.

## Signing in to Claude

Install the claude CLI from Anthropic and sign in yourself with `claude auth login`. Eileen
spawns the unmodified CLI in your environment; it never reads credential files, never runs a login
flow and never proxies tokens. The only API key Eileen accepts is DeepSeek's, kept in the
operating system's credential store, never in a file in the repository.

## Troubleshooting

**`cargo xtask` exits with code 75.** No build slot came within `--wait-max`. Run
`cargo xtask slots` to see who holds them, wait, and try again; `--kill-stale` frees a slot whose
holder died.

**cargo says a workspace member has no manifest.** Every folder under `crates/` is a member; a
folder there without a `Cargo.toml` breaks the workspace.

**A test binary loads the wrong `onnxruntime.dll`.** Run `cargo xtask fetch-native` and build
through `cargo xtask`, which points `ort` at the verified copy.

**A slow or hung build on a busy PC.** Close heavy apps, and if you use WSL, `wsl --shutdown`
frees about 4 GB; the guard then admits a second slot on its own.
