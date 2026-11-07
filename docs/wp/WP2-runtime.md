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
  `crates/eileen-scenarios`, the `ipc` module of `crates/eileen-supervisor` with decision D86's
  changes to its `spawn`, `ChildSpec` and `Child` (WP0 reviews them), and the `PathProbe` trait
  with its fake in `crates/eileen-platform` (decision D86), `TESTING.md`, `docs/DEBUGGING.md`,
  `scenarios/runtime/`, the fuzz targets `mcp_client_message`, `mcp_tool_arguments`,
  `dispatch_line`, `worker_hook_payload`, `http_request` and `scenario_file` with their corpora
  and regressions, and this file.

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
   the session role works only from outside every Eileen job, and a pipe instance a worker adds
   receives no token from any shim.
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
    `eileen doctor machine`, the machine link carrying a benchmark to a test audio side and back,
    and the probe's readings on the owner's PC.
15. The speech transport, with stand-in speech processes: only the spawned process with the
    right token is handed over and reported `Ready` once, a squatter on the name makes the
    transport fail closed, nothing restarts until the core says so, and the pipe's DACL grants
    only the current user.

Every pull request also meets the rules section 6.1 sets for every package.

## Progress

- `amend/wp2-ipc-home` (#171, amendment, decision D86): settles where the Win32 calls of the pipes,
  caller checks and path classification live, since `eileen-runtime` forbids unsafe code, stays
  neutral and may not depend on a `-win` crate. `eileen-supervisor` gains an `ipc` module that
  WP2 builds, with types on every OS (`PipeServer` and `PipeConn` with both ends' process ids,
  `Peer`, which opens the other end once and refuses a process created after the connection,
  `PeerFacts`, a `Jobs` registry the runtime owns, the owner of a loopback connection, user-only
  token files); the runtime decides each role with a pure function over `PeerFacts`. Every
  client of a core pipe, the shim in every role included, checks that the server is the core
  before it sends anything. `spawn` makes each child's Job Object itself, so `ChildSpec` gains
  `jobs`, `job_key`, `breakaway_ok` and `job_memory_limit_mb` and `Child` gains
  `memory_limit_hit`, a cross-package change WP0 reviews; WP3's driver sets each session's key,
  its memory limit and the core's process id (section 6.5). `eileen-platform` gains `PathProbe`
  with a fake, implemented in `eileen-shell-win`, which opens only drive and volume paths and
  asks `share` about a UNC path before opening anything. `CoreConfig.local_time` comes from
  `eileen-shell-win`'s zone reader through the composition root; `getrandom` 0.4.3 mints the
  tokens; the `windows` union gains `Win32_System_Time`, `Win32_NetworkManagement_NetManagement`,
  `Win32_System_SystemServices` and `Win32_System_Ioctl`. BUILD.md sections 2.1, 2.12, 3.1,
  3.3.3, 3.3.7, 3.3.9, 3.4, 4.1, 6.2, 6.4, 6.5 and 7 and the root `Cargo.toml` say so;
  `Cargo.lock` does not change.
- `amend/wp2-session-wiring` (amendment, docs only): reconciles BUILD.md with what WP3's #142
  and WP1's #130 merged and suggested. Each session's hooks post to
  `<HookEndpoint.path>/<event in kebab case>` with the path `v1/hooks/claude/<session>`, the
  layout `eileen_harness::session` writes, so the endpoint knows the session from the path
  (sections 2.5 and 6.4). The shim takes `--pipe`, `--role` and, for a worker, `--session`, with
  its token in `EILEEN_SHIM_TOKEN` and the core's process id in `EILEEN_CORE_PID` (section 2.5).
  For every Claude session it starts, a worker or a brain instance, the harness takes the
  endpoints through `eileen_harness::SessionEndpoints::open` with the role `shim_role` gives
  (`librarian` for a librarian job), and calls `close` after every end of that process;
  `eileen-cli` implements the trait over the new session registry, `eileen_runtime::sessions`,
  and the driver loop routes the core's `WorkerCmd::Start` unchanged (section 3.3.8). The trait
  is WP2's proposal, which WP3's `wp3/worker-brain` confirms or changes (section 6.5). Every
  session shares the runtime's one dispatcher pipe, made at start, as sections 2.12 and 3.3.9
  had it. The endpoint answers a brain-role session's hooks itself, since the core knows no
  brain instance's `ws_` id. WP2 pins `tools` and `tools/brain` in W1 and its other four
  provisional modules with their W2 producers (section 3.3). The machine link's test moved from
  acceptance 15 to 14, so `wp2/speech-transport` does not wait for `wp0/machine-contracts`; until
  `eileen-guard` lands, `HangDumpThenKill` kills the speech process without a dump, with a
  warning. Section 6.4 lists the W1 order below.
- W1 order (BUILD.md section 6.4, "W1 order"), each pull request once those it names have merged:
  1. `amend/wp2-ipc-home` (#171, merged).
  2. `amend/wp2-session-wiring` (this amendment).
  3. `wp2/supervisor-pipes`: `PipeServer` and `PipeConn` with the pipe ends' process ids, and
     the incident when a next instance cannot be made.
  4. `wp2/supervisor-callers` (after 3): `Peer`, `PeerFacts` and the `Jobs` registry with the
     changes to `spawn`, `ChildSpec` and `Child` that WP0 reviews, `tcp_client_pid` and
     `write_user_only`.
  5. `wp2/path-probe-trait`: the trait and `FakePathProbe` in `eileen-platform`, then
     `WinPathProbe` in `eileen-shell-win`. If #77 is still held, it builds on `main`'s
     `Deadline` and says so.
  6. `wp2/shell-local-time`: the zone reader and its wiring into `CoreConfig.local_time`.
  7. `wp2/speech-transport` (after 3 and 4, since its caller check needs `Peer` and the
     registry): acceptance 15, with the interim kill without a dump. `wp1/speech-life` (#141)
     and WP4's `SpeechTransport` (#140) have merged.
  8. `wp2/driver-loop`: the driver loop, effects routed by family, the bus with explicit
     `Lagged`, and `eileen run --console`'s view.
  9. `wp2/hook-endpoint` (after 4 and 8): the session registry with `open` and `close`, and the
     loopback server with the session hooks routed by role (a brain-role session's answered by
     the endpoint), the guard, the tripwire, the client mapping and job check, and `bus.json`.
     It implements no trait, since `SessionEndpoints` arrives with `wp3/worker-brain`.
  10. `wp2/dispatcher` (after 9): the one dispatcher pipe, the pure caller decision and
      `eileen-shim`, pinning `tools` and `tools/brain` and rewording the provisional doc line
      and its test for every owner, with WP0 reviewing.
  11. `wp2/path-classes` (after 5 and 9): the `PathFact`s of each `PreToolUse` request, which
      slice S1's job needs; the `GitFact` and `UrlFact` rows follow in W2.
  12. `wp2/shell-tray`: the tray's mute item with the physical-click check, and the mute hotkey.
  13. `wp2/s1-live` (after 6 to 12, `wp3/worker-brain`, `wp4/s1-audio` and `wp4/s1-speech`),
      with the `SessionEndpoints` implementation in `eileen-cli`.

  WP3's #159 (`ClaudeWorker`, merged) spawns sessions with no job key and refuses a memory
  limit; its key, limit and the core's process id follow `wp2/supervisor-callers` in a WP3 pull
  request.

## Surprises

- process-wrap 10.0.1 keeps the handle of the Job Object it makes private (`JobObjectChild`
  exposes none), so `IsProcessInJob`, section 2.1's breakaway limit and WP3's job memory limit
  cannot reach it; the supervisor makes each child's Job Object itself under D86, keeping the
  same limits.
- The first-instance flag protects a pipe's name only until the core starts. A DACL that lets
  the core add instances grants `FILE_CREATE_PIPE_INSTANCE` (4, inside `FILE_GENERIC_WRITE`),
  which the user cannot be denied, so any process of the user can add an instance later and
  catch new clients; every client now checks the server first.
- WP3's generated MCP config holds no token: it names `${EILEEN_SHIM_TOKEN}`, and the tokens
  travel in the session's environment, so the token files are only `bus.json` and
  `session.json`.
- `Cargo.lock` holds three `getrandom` versions. 0.3.4 arrives only as a build dependency of
  `zip` 2.6.1 on Windows, while 0.4.3 is already in every binary through `ulid`'s `rand`, so
  the token pin is 0.4.3.
- The six provisional modules WP2 owns each carry the doc line "Provisional: WP2 pins this
  module in its first pull request", whose wording `eileen-contracts`' `it::modules` test
  checks. Section 3.3 now pins each with its first producer, so `wp2/dispatcher` rewords the
  line, the test, the `modules.toml` header and the crate doc to `<owner> pins this module`,
  with WP0 reviewing, and each later pinning pull request removes its module's line.
- The brain's settings file carries a blocking `PreToolUse` hook with no matcher, so every
  `mcp__eileen__*` call the brain makes reaches the endpoint, while `Core::hook` on `main` denies
  a session with no live job. Forwarding the brain's hooks to the core would have denied its
  `jobs_dispatch`, so the endpoint answers them by role.
- #142's test fixtures name the dispatcher pipe `eileen-dispatch-ws_...`. That is fixture data:
  the harness writes whatever pipe the endpoints give, and the plan keeps one pipe per runtime.
- `FakePathProbe` should keep simulated time as #77 gives the other fakes; #77 is open and held
  for the owner, so `wp2/path-probe-trait` builds on `main`'s `Deadline` if #77 has not merged
  by then, and says so.

## Decisions

- D86: pipes and caller checks live in `eileen-supervisor`'s `ipc` module, path probes behind
  the `PathProbe` platform trait; every pipe client checks the server; `spawn` makes each child's
  Job Object itself; Linux forms are WP10's, and until then `ipc` answers `Unsupported` on
  Linux.

## Outcome

Open.
