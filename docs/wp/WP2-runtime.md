# WP2: Runtime, binaries, shell and the first slice

The living plan for work package 2 of [BUILD.md](../BUILD.md) section 6.4: the runtime around the
core, the binaries, the Windows shell and slice S1. The agent doing the work keeps it current.

## Goal

The runtime and the binaries around the core: the driver loop, the SQLite store, the bus, the
memory sampler, the loopback HTTP server, path, git and URL classification into `PathFact`s,
`GitFact`s and `UrlFact`s, the dispatcher pipe and `eileen-shim`, the sidecar MCP clients, the
flight recorder, `eileen-crash` and `eileen-guard`, the Windows shell (tray, hotkeys, Raw Input,
`SystemMemory`, `MachineIdentity`, `SecretStore`), `eileen-cli` as the composition root, the
install id and the machine hash, the `session` and `dev` toolsets, `eileen-scenarios`,
`TESTING.md` and `docs/DEBUGGING.md`. The machine profile's runtime part waits for WP0's
`wp0/machine-contracts`.

It also brings up slice S1, the gate for wave W3. `wp2/s1-live` adds
`eileen_runtime::bootstrap::SliceResolver`, a minimal pure resolver that WP7 phase A replaces,
and runs the slice live on the owner's PC once `wp3/worker-brain`, `wp4/s1-audio` and
`wp4/s1-speech` have merged: one fast-path command, one barge-in, one brain answer and one Claude
job, with the mute hotkey and tray mute, and latencies recorded. The same flow runs in CI with a
WAV source, the transcript stub and the stub harness.

## Branch, dependencies and files

- Branch: `wp2/<topic>`; the slice is `wp2/s1-live`.
- Depends on: WP0 phase A and WP1's first pull request, the minimal core. `wp2/s1-live` also waits
  for `wp3/worker-brain`, `wp4/s1-audio` and `wp4/s1-speech`, not for all of WP3 and WP4.
- Wave: W1 from WP1's minimal core, the rest in W2.
- Desktop: no for the automated tests, whose few tray and hotkey tests take the desktop lock.
  Slice S1 runs on the owner's PC and holds `desktop.lock` for the run
  (`cargo xtask desktop-lock --hold`).
- Owns: `crates/eileen-runtime`, `crates/eileen-mcp`, `crates/eileen-cli`, `crates/eileen-shim`,
  `crates/eileen-crash`, `crates/eileen-guard`, `crates/eileen-shell-win`,
  `crates/eileen-scenarios`, the `ipc` module of `crates/eileen-supervisor` and the `PathProbe`
  trait with its fake in `crates/eileen-platform` (decision D86), `TESTING.md`,
  `docs/DEBUGGING.md`, `scenarios/runtime/`, the fuzz targets `mcp_client_message`,
  `mcp_tool_arguments`, `dispatch_line`, `worker_hook_payload`, `http_request` and
  `scenario_file` with their corpora and regressions, and this file.

## Done when

The acceptance list of BUILD.md section 6.4, items 1 to 15, each recorded under Progress with its
evidence when it holds. The lines below are reminders; the full text in BUILD.md is the bar.

1. Slice S1 passes live on the owner's PC, with latencies and the `desktop.lock` hold recorded,
   and its stub variant passes in CI.
2. A killed stand-in sidecar gives the caller a typed error, an incident with its hang dump and a
   restart within the backoff bound, while the core keeps capturing.
3. The `PreToolUse` endpoint never answers `allow`; a held `approve` call allows only after a
   `UserInput` approval; the `PermissionRequest` tripwire answers `deny`.
4. Caller verification: token files inside a worker's Job Object open only that session's hooks,
   and the session role works only from outside every Eileen job.
5. The idle footprint of the thin release `eileen.exe`, measured with `cargo xtask measure-idle`,
   is under 60 MB working set and within section 4.5's CPU table.
6. `eileen trace last` stitches one turn across processes by its `turn_` ID, and
   `eileen incident show last` summarises a captured incident.
7. The redaction canary covers every sink this package writes, greyed partial transcripts never
   leave the core, and `Record::PurgeJobs` leaves no trace of the purged jobs.
8. `eileen scenario save`, `share` and `submit --dry-run` give a body that passes `pr-meta` and a
   redacted folder that passes `scenarios check` and still replays, both with the machine hash.
9. The shuttle tests pass, and the six fuzz targets run clean for 10 minutes each, with their
   corpora replaying on Windows.
10. Path classification: table tests over real files and links in a temporary folder give the
    classes, flags, `GitFact`s and `UrlFact`s of section 2.12.
11. `eileen scenario bless` and `stale` behave as specified, and `eileen scenario check` agrees
    with check 18 on expiry.
12. The machine-wide memory files: only a runtime reading real memory writes them, with no
    duplicate windows, and the `auto` paging limit passes its table test.
13. Machine identity: the install id and `fleet\machine.toml` lifecycle, and no raw MachineGuid or
    install id in any other file Eileen writes.
14. The machine profile's runtime: the facts, hourly and plan files, change events within 1 s,
    `eileen doctor machine`, and the probe's readings on the owner's PC.
15. The speech transport, with stand-in speech processes: only the spawned process with the
    right token is handed over and reported `Ready` once, a squatter on the name makes the
    transport fail closed, nothing restarts until the core says so, and the pipe's DACL grants
    only the current user.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

- `amend/wp2-ipc-home` (#171, amendment, decision D86): settles where the Win32 calls of the pipes,
  caller checks and path classification live, since `eileen-runtime` forbids unsafe code, stays
  neutral and may not depend on a `-win` crate. `eileen-supervisor` gains an `ipc` module that
  WP2 builds (pipe servers made as the first instance with remote clients rejected and a
  user-only DACL, both ends' process ids, the image path, a registry of the Job Objects `spawn`
  made with `in_job` and `in_any_job`, the owner of a loopback connection, user-only token
  files); `eileen-platform` gains `PathProbe` with a fake, implemented in `eileen-shell-win`;
  `CoreConfig.local_time` comes from `eileen-shell-win`'s zone reader through the composition
  root; `getrandom` 0.4.3 mints the tokens; the `windows` union gains `Win32_System_Time` and
  `Win32_NetworkManagement_NetManagement`. BUILD.md sections 2.12, 3.1, 3.3.3, 3.3.7, 3.4, 4.1
  and 6.4 and the root `Cargo.toml` say so; `Cargo.lock` does not change.
- Next, in this order once the amendment merges: `wp2/supervisor-pipes` (the `ipc` pipe server
  and the pipe ends' process ids, with `server_pid` for the speech process),
  `wp2/supervisor-callers` (the Job Object registry, `image_path`, `tcp_client_pid` and
  `write_user_only`), `wp2/path-probe-trait` (the trait and `FakePathProbe` in
  `eileen-platform`, then `WinPathProbe` in `eileen-shell-win`) and `wp2/shell-local-time`
  (the zone reader and its wiring into `CoreConfig.local_time`). `wp2/speech-transport` now
  waits for the `ipc` pipes too; `wp1/speech-life` (#141) and WP4's `SpeechTransport` (#140)
  have merged.

## Surprises

- process-wrap 10.0.1 keeps the handle of the Job Object it makes private (`JobObjectChild`
  exposes none), so `IsProcessInJob` and section 2.1's breakaway limit cannot reach it; the
  supervisor makes each child's Job Object itself under D86, keeping the same limits.
- `Cargo.lock` holds three `getrandom` versions. 0.3.4 arrives only as a build dependency of
  `zip` 2.6.1 on Windows, while 0.4.3 is already in every binary through `ulid`'s `rand`, so
  the token pin is 0.4.3.
- `FakePathProbe` should keep simulated time as #77 gives the other fakes; #77 is open and held
  for the owner, so `wp2/path-probe-trait` builds on `main`'s `Deadline` if #77 has not merged
  by then, and says so.

## Decisions

- D86: pipes and caller checks live in `eileen-supervisor`'s `ipc` module, path probes behind
  the `PathProbe` platform trait; Linux forms are WP10's, and until then `ipc` answers
  `Unsupported` on Linux.

## Outcome

Open.
