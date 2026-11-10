# WP10: Linux, Ubuntu first

The living plan for work package 10, Linux support with Ubuntu first, which
[LINUX.md](../LINUX.md) section 8 proposed and pull request #31 added to
[BUILD.md](../BUILD.md) as section 6.12. The agent doing the work keeps this file current.

## Goal

Eileen runs on Linux, Ubuntu first, through the platform traits of BUILD.md section 3.4, in
`-linux` crates that mirror the `-win` crates, and the Windows design stays as it is.
[LINUX.md section 8](../LINUX.md#8-proposed-work-package-wp10-linux-ubuntu-first) proposes the
package and [section 7](../LINUX.md#7-phases-and-acceptance) its four phases:

- L0, now, beside W0 to W2: memory, machine facts, identity and the Linux lane;
- L1, during W2 to W4: the D-Bus services;
- L2, toward v0.2: audio, accessibility, input and the HUD, ending in slice S1 on Linux;
- L3: packaging and the Linux fleet.

WP10 runs on a Linux machine of its own and holds none of the owner PC's build slots. Whether L2
ships with v0.1 or v0.2 is the owner's call. The owner asked for Linux in the plan on 2026-10-21,
and the entry that records it is proposed below.

## Done when

The acceptance of each phase in [LINUX.md section 7](../LINUX.md#7-phases-and-acceptance), each
recorded here with its evidence when it holds:

- **L0.** Both PR lanes of [LINUX.md section 6](../LINUX.md#6-testing-on-linux) green on
  `ubuntu-24.04` and `ubuntu-26.04`; a `SystemMemory` sample that matches recorded `/proc`
  fixtures exactly and a live reading within the kernel's own figures; machine facts read in
  under 200 ms, BUILD.md section 2.16's budget, on a real Ubuntu desktop.
- **L1.** Each D-Bus backend passes its trait's conformance suite against a fake service in CI
  and against the real service on Ubuntu 26.04. No conformance suite exists yet; LINUX.md section
  6 proposes one per trait.
- **L2.** S1-L on Ubuntu 26.04: the real microphone and speakers, mute and tray, one fast-path
  command, one barge-in, one brain answer and one Claude job, with latencies recorded.
- **L3.** LINUX.md section 7 names its builds (`.deb`, then Flatpak; the Linux fleet; the Linux
  part of `TESTING.md`) and no acceptance yet.

Every pull request also meets the rules BUILD.md section 6.1 sets for every package, among them:
small (logic changes under about 500 lines), the Evidence section first, merged only with
`all-green` and `pr-meta`, this file and the crate docs current, and a fuzz target for every new
parser of untrusted input. WP10's acceptance is BUILD.md section 6.12 (from #31), and this file follows that text.

## Progress

Each pull request records itself here (AGENTS.md, "Making a change", rule 9). The table holds
what merged through 2026-10-27, in merge order, with each squash commit, and is closed: nothing
is added to it. Since then a pull request adds one line of its own to the list under the table,
at its end, and edits no other pull request's line. The line gives the branch, the number once
the pull request has one, its type, and what it brings, written as what `main` holds once it
merges, so the line stays true when it merges and no later edit is needed; the merge date and
commit are git's (`git log --first-parent origin/main`). When two pull requests add their lines
at once, the second to merge keeps both in the conflict. A pull request that narrows or closes a
bullet under "Still open", or adds to Surprises, Findings for other packages or Decisions to
record, edits only those lines. A record pull request, such as `wp10/record-wave-6`, brings the
rest of this file up to date with `main`: the open pull requests, the plans, the measurements.

| Order | Pull request | Branch | State |
|---|---|---|---|
| 1 | #26 | `Ubuntu-Compatible` | Merged on 2026-10-22 as e4aff64: `docs/LINUX.md`, `eileen-shell-linux` and `eileen-apps-linux` |
| 2 | #27 | `wp10/pins-amendment` | Merged on 2026-10-22 as 8aa499f: BUILD.md section 4.1's `tokio` 1.53.2 and the Linux pins (`rustix`, `zbus`, `quick-xml`) |
| 3 | #28 | `wp10/parser-fixes` | Merged on 2026-10-22 as db428f3: two parser fixes the fuzz invariants found |
| 4 | #30 | `wp10/codeowners-linux` | Merged on 2026-10-22 as 39e389e: Ayham beside the owner on the Linux paths in `.github/CODEOWNERS` |
| 5 | #31 | `wp10/build-amendment` | Merged on 2026-10-22 as 9c6a6f3: WP10 into BUILD.md as section 6.12, with the three `-linux` crates in section 3.1 |
| 6 | #32 | `wp10/parser-fuzzing` | Merged on 2026-10-22 as 8bdf1f4: fuzz entries, seed corpora and property tests for the Linux parsers |
| 7 | #33 | `docs/wp10-linux` | Merged on 2026-10-22 as a5dfa2f: this file, and LINUX.md's pointers to it and to #31 |
| 8 | #35, #36, #37 | `wp10/dbus-errors`, `wp10/dbus-names`, `wp10/dbus-portal` | Merged on 2026-10-22 as 7e971a2, 5f74241 and 437c4d8: `eileen-dbus-linux`, the D-Bus helpers the Linux crates share |
| 9 | #38 to #45 | `wp10/session-autostart`, `-lock`, `-keep-awake`; `wp10/notifications`, `wp10/notification-events`; `wp10/power-upower`, `-profiles`, `-changes` | Merged on 2026-10-22, in order, in `eileen-shell-linux`: session #38 to #40 (bcfa948, cd587dd, 6ec27e5), notifications #41 and #42 (157d9f8, d2f3277), power #43 to #45 (017aa3f, fa4b7ea, c12af36) |
| 9 | #46 to #53 | `wp10/open-portal`, `-launch`; `wp10/recent-values`, `-xbel`, `-files`; `wp10/mpris-values`, `-sessions`, `-watch` | Merged on 2026-10-22, in order, in `eileen-apps-linux`: open #46 and #47 (e499cf6, 2aa324e), recent #48 to #50 (739e69d, c01f5de, f7e7dd2), MPRIS #51 to #53 (64a8167, d247798, 8734897) |
| 10 | #54 | `docs/wp10-progress` | Merged on 2026-10-22 as 2cd4eeb: this file's progress, and LINUX.md's L1 rows and status as built |
| 11 | #55, #56, #57 | `wp10/recent-fifo`, `wp10/autostart-refused`, `wp10/dbus-plumbing` | Merged on 2026-10-22 as bbc5426, d7af417 and 7fc7518: `RecentlyUsed` refuses a FIFO or a device in its file's place, `AutostartError` maps a refusal and a missing folder as `FileError` does, and the D-Bus plumbing the backends repeated moved into `eileen-dbus-linux` |
| 12 | #58 | `docs/wp10-followups` | Merged on 2026-10-23 as 8df0a20: this file's follow-ups, and LINUX.md's line on the shared helpers |
| 13 | #60, #63 | `wp10/base-crates-lock`, `wp10/base-crates-sync` | Merged on 2026-10-24 and 2026-10-25 into WP0's `backup/wp0-base-crates`, which reached `main` as #73 (a67d096): quick-xml and the Linux crates' edges in its lockfile, then `main`'s tokio 1.53.3 (#59) and #61 merged into it |
| 14 | #61 | `wp10/dbus-bridge-amendment` | Merged on 2026-10-24 as d94f348: BUILD.md section 3.1 lets `eileen-dbus-linux` depend on `eileen-platform` and `eileen-stdx` |
| 15 | #67 to #70 | `wp10/index-fifo`, `wp10/mpris-concurrent-reads`, `wp10/index-walk-links`, `wp10/recent-read-limit` | Merged on 2026-10-25 as 29faabc, 08ec419, cdc98ac and b94d9de: the index opens each entry without waiting on a FIFO and skips one over 1 MiB, the MPRIS listing reads every player at once, the index walks each folder once and at most 1024 under each `applications` folder, and `RecentlyUsed` refuses a file over 16 MiB |
| 16 | #62 | `wp10/dbus-bridge` | Merged on 2026-10-25 as edbc939: the `Bridge` from the synchronous traits to the async D-Bus clients, and `From<DbusError> for PlatformError` |
| 17 | #64, #65, #66 | `wp10/shell-traits`, `wp10/media-traits`, `wp10/apps-traits` | Merged on 2026-10-25 as dff9ee4, 859b08e and cc2f908: `ShellHost` and `SystemMemory` (`LinuxShell`, `LinuxSystemMemory`), `MediaSessions` (`LinuxMedia`), and `Apps` and `RecentSources` (`LinuxApps`, `LinuxRecent`) |
| 18 | #85 | `wp10/shell-keep-awake-connects` | Merged on 2026-10-25 as 2d5acb8: the keep-awake holder connects both buses within half the call's deadline, and a bus that hangs is a late holder, not an absent one |
| 19 | #93 | `wp10/shell-global-shortcuts` | Merged on 2026-10-25 as c52d68c: `ShortcutsSession`, a client of the GlobalShortcuts portal, and `Request::answer` with the portal's results |
| 20 | #79, #81, #82, #88, #89 | `wp10/trace-retention`, `wp10/trace-schema-parents`, `wp10/trace-ring-dump`, `wp10/trace-card-rule`, `wp10/trace-privacy-paths` | Merged on 2026-10-25 as 5e6810b, 863ef08, 15dfb9a, ac9433d and b0d9320: amendments to WP0's `eileen-trace` for the review of #73 (Findings for other packages) |
| 21 | #84 | `wp10/amend-spawn-bans` | Merged on 2026-10-25 as 546d308: amendment of BUILD.md section 4b.2 and both `clippy.toml` files, which ban the spawn forms that skip `eileen_rt::task` |
| 22 | #114 | `wp10/deny-zstd-advisories` | Merged on 2026-10-26 as 1013e1c: amendment of section 4b.3, `deny.toml` ignores four build-time `zstd` advisories with three bans, under decision D80 |
| 23 | #78, #80 | `wp10/desktop-crate-row`, `wp10/audio-crate-amendment` | Merged on 2026-10-26 as 7792c0d and d4945f2: amendments of section 3.1 with the rows of `eileen-desktop-linux` and `eileen-audio-linux`, and their CODEOWNERS lines |
| 24 | #106, #115, #117 | `wp10/supervisor-child-tree`, `wp10/supervisor-class-stderr`, `wp10/supervisor-detached-reap` | Merged on 2026-10-26 as 7bded52, af6a39a and a84c198: amendments to WP0's `eileen-supervisor`, a supervised child's whole tree killed and reaped on Linux, a Claude session's nice value and stderr drained in `wait`, and the detached guard reaped on Unix (Findings for other packages) |
| 25 | #86, #92 | `wp10/trace-spantrace-pii`, `wp10/trace-console-nonblocking` | Merged on 2026-10-26 as 5a0741e and 6c8f078: amendments to WP0's `eileen-trace`, no `pii.` span fields in a logged `SpanTrace`, and the console on a thread of its own |
| 26 | #105, #108 | `wp10/rt-thread-names`, `wp10/buildguard-cargo-tree` | Merged on 2026-10-26 as 2502a63 and 1499ed6: amendments to WP0's `eileen-rt` (thread names within Linux's 15 bytes) and `eileen-buildguard` (a stopped build kills cargo's whole tree, at nice 10) |
| 27 | #109 | `wp10/linux-fuzz-targets` | Merged on 2026-10-26 as a9399fa: the three Linux fuzz targets in `fuzz/`, declared with their crates as dependencies; built by CI's fuzz job once it exists |
| 28 | #104 | `wp10/linux-crate-docs-owns` | Merged on 2026-10-26 as 84822d2: the Linux crates' `# Owns` sections kept to three bullets, which leaves `lint-repo` the two check 14 problems in `eileen-dbus-linux`'s `testing.rs` |
| 29 | #107, #110 | `wp10/shell-shortcuts-holder`, `wp10/shell-tray` | Merged on 2026-10-26 as d94057a and e4139be: `register_hotkey` and `watch_input` through the GlobalShortcuts portal, and `set_tray` as a StatusNotifierItem with a dbusmenu menu |
| 30 | #83 | `wp10/linux-desktop-lane` | Merged on 2026-10-26 as d446972: amendment of section 4.6 with the nightly Linux desktop lane, its `session` job and the first four `linux_session` tests |
| 31 | #87 | `wp10/desktop-atspi-client` | Merged on 2026-10-26 as 4729302: `eileen-desktop-linux` with a bounded AT-SPI2 client, the first of `eileen-desktop-linux`'s L2 pull requests, which #94 and #112 build on |
| 32 | #90 | `wp10/audio-rtkit` | Merged on 2026-10-26 as aac5055: `eileen-audio-linux` with `register_audio_thread` through RealtimeKit |
| 33 | #103 | `wp10/docs-linux-status` | Merged on 2026-10-27 as 7de0357: this file and LINUX.md brought up to date with `main` at a84c198 |
| 34 | #91 | `wp10/linux-session-tests` | Merged on 2026-10-27 as b13e05c: five more `linux_session` tests for the shell's clients (the power facts and their stream, notifications and their `Closed` event, the Inhibit portal's GTK backend), and the lane's session unit without `CAP_WAKE_ALARM` in its bounding set, so that bwrap starts |
| 35 | #118 | `wp10/supervisor-stderr-forwarding` | Merged on 2026-10-27 as 71b8dad: amendment to WP0's `eileen-supervisor` and `eileen-cli`, a child's stderr forwarded from `spawn` on every OS, with `ChildSpec::raw_stderr` for a caller that wants the raw pipe (SU5, Findings for other packages) |
| 36 | #121 | `wp10/desktop-lane-apt` | Merged on 2026-10-27 as 56ea109: amendment of section 4.6, the desktop lane's packages from an Actions cache keyed by the runner image and the package list, and its install step within 10 minutes of its own |
| 37 | #123, #122, #124 | `wp10/rtkit-fallback`, `wp10/tray-watcher-bounds`, `wp10/mpris-parallel-reads` | Merged on 2026-10-27 as 2fda2e9, fb82ae3 and 7f493bf: the RealtimeKit fallback never lowers a thread's nice level, and a request whose connection failed is reverted over a new one (#90's follow-ups); the tray answers within its deadline when the watcher stalls, and asks a watcher that refused again (#110's); MPRIS players are read concurrently in the watch, each within one bound (#65's and #68's) |
| none | #29 | `Ubuntu-Compatible-L1` | Closed: the first D-Bus work as one pull request, replaced by orders 8 and 9 |

Since 2026-10-28, one line per pull request, each added by the pull request itself:

- `wp10/record-wave-6` (docs): this file and LINUX.md brought up to date with `main` at 18f71fd:
  #91, #118 and #121 to #124 recorded, what decision D86 asks of WP10 planned, the open pull
  requests' status, and this list in place of new table rows.
- `wp10/ipc-linux-plan` (docs): LINUX.md section 5's plan for the Linux forms of decision D86's
  `ipc` module and `PathProbe`, with each Windows behaviour's Linux mechanism, what it
  guarantees and does not, and its kernel floor on Ubuntu 24.04 and 26.04; `run/` under
  `$XDG_RUNTIME_DIR/eileen/`; and here the pull requests that build them, what WP2's interfaces
  can keep neutral for Linux, and the decision to record.

Open on 2026-11-10, with what each waits for:

- The owner's #144 (`amend/linux-desktop-toolchain`, draft, into `main`, which it now conflicts
  with) amends WP10's `linux-desktop.yml`: the lane's `session` job gets the step
  `ci.yml`'s `logic-linux` has, which keeps only the pinned toolchain before rust-cache so that
  the cache key does not follow the runner image, and xtask's workflow tests lose their one
  exception, `WITHOUT_THE_TOOLCHAIN_STEP` (the lane's `session` job, on `main`). It asks for
  WP10's review, still pending. Meanwhile the lane's rust-cache step finds no cache in its
  scheduled runs on `main` (42364908679 of 2026-11-07 to 42809906168 of 2026-11-10).
- Dependabot's #167 (`dependabot/cargo/zbus-6.0.0`, into `main`) moves `zbus` from 5.19.0 to 6.x
  in the root manifest. Its `logic-linux` fails to compile `eileen-dbus-linux` (run 42405365074:
  `zbus::object_server` is not found from `testing.rs`, and `portal.rs` fails too). BUILD.md
  section 4.1 pins 5.19.0, so a move to 6 needs that section's amendment and a port of the
  `-linux` crates; whether and when is the owner's call.
- The owner's #191 (`wp0/machine-contracts`, draft, into `main`): the `MachineProbe` trait
  (`facts` with the caller's `Deadline`, `load` and `watch`), `FakeMachineProbe` and the
  `machine` contracts, which WP10's Linux probe builds on ("Not started yet" below). It also
  gives `AudioEndpoints` `endpoints` and `watch_endpoints` with default bodies that answer
  `Unavailable`, so `eileen-audio-linux` needs no edit.
- #94 (`wp10/desktop-grounding`, draft, into `main`): `Grounding` on the AT-SPI2 client, as
  `LinuxDesktop`. It stays a draft for its own two `lint-repo` check 14 problems (its test starts
  a copy of the test binary with `Command::new`), which wait for the owner's answer on #113.
- #112 (`wp10/desktop-automation`, draft, on #94): `Automation` on the same client. It waits for
  #94.
- #111 (`wp10/shell-secret-service`, draft, into `main`, which it now conflicts with): WP10's
  proposal for the owner, `SecretStore` over the Secret Service with a client of Eileen's own, a
  `plain` session and no prompts. It waits for the owner's answer.
- #113 (`wp10/private-bus-spawn-route`, draft, into `main`, which it now conflicts with): WP10's
  proposal for the owner, an amendment of section 3.1 that lets `eileen-dbus-linux` depend on
  `eileen-supervisor` under its `private-bus` feature, so the private test bus starts
  `dbus-daemon` through `eileen_supervisor::spawn`. It waits for the owner's answer; a WP10 pull
  request then clears the two check 14 problems at `crates/eileen-dbus-linux/src/testing.rs`.
- The owner's #77 (`wp0/platform-file-deadlines`, into `main`, which it now conflicts with) gives
  the file-reading trait methods the caller's `Deadline`, as BUILD.md section 3.4 says since #76,
  and WP10's #116 (`wp10/platform-deadlines-linux`, ready, into #77) moves `LinuxApps`,
  `LinuxRecent` and the Windows stubs to its signatures, so #77 compiles once merged with `main`.
  When they merge is the owner's call.
- The owner's #119 (`wp11/gaze-proposal`, draft, into `main`) proposes `docs/GAZE.md`, a work
  package WP11 for gaze through a hardware eye tracker, and WP10's #120 (`wp10/gaze-webcam`,
  ready, into #119) folds a webcam route into it, crediting Aaron's prototype. Both change only
  `docs/GAZE.md`, and what becomes of them is the owner's call.

Planned, not yet opened:

- What decision D86 gives WP10, each after the WP2 pull request it builds on, in BUILD.md section
  6.4's W1 order, none of which is on `main` yet:
  - the Linux forms of `eileen-supervisor`'s `ipc` module, the pipes as Unix sockets and the
    caller checks that LINUX.md section 5 plans, after `wp2/supervisor-pipes` and
    `wp2/supervisor-callers` (items 3 and 4) give the module its types. Until then every `ipc`
    call on Linux answers `Unsupported`, so no Linux build accepts a caller it could not check
    (BUILD.md section 2.12), and `wp2/dispatcher`'s tests through a real pipe are Windows-only
    (item 10). They land in this order, each reviewed by WP2, whose module it is:
    - `wp10/ipc-sockets`, after item 3: `PipeServer` and `PipeConn` over Unix sockets in a
      checked 0700 folder under `$XDG_RUNTIME_DIR/eileen/`, the stale-socket rule, both ends'
      process ids from `SO_PEERCRED`, and `since` on the boot clock;
    - `wp10/ipc-peers`, after item 4: `Peer` on a `/proc/<pid>` descriptor and a pidfd,
      `PeerFacts` with the image and the jobs by session, the `Jobs` registry's Linux entries,
      `write_user_only` and `grants_only_user` with `run/` under `$XDG_RUNTIME_DIR/eileen/`, and
      the `/proc/<pid>/stat` parser with a fuzz target of its own;
    - `wp10/ipc-tcp-owner`, after item 4: `tcp_client_pid` through sock_diag and a walk of
      `/proc/<pid>/fd`, with L1's measurement of the walk's cost;
    - an amendment of BUILD.md section 4.1 that pins `libc` for the one `getsockopt` of
      `SO_PEERPIDFD` in the supervisor's `ffi` module, unless a rustix release that wraps it
      comes first; until then `Peer` opens `/proc/<pid>` by `SO_PEERCRED`'s process id and
      checks the start time, to 10 ms;
  - `PathProbe` in `eileen-shell-linux` (BUILD.md section 3.4), after `wp2/path-probe-trait`
    (item 5) adds the trait and its fake, as LINUX.md section 5 plans it, with the desktop-entry
    parser moved into `eileen-dbus-linux`, which `eileen-shell-linux` may use (or section 3.1's
    row amended), and new readers of `mimeapps.list` and shared-mime-info's `globs2`, the
    latter with a fuzz target. Until then a path that needs a probe is `Unknown` on Linux
    (D86);
  - `LocalTime` from `/etc/localtime`, after `wp2/shell-local-time` (item 6) wires Windows' zone
    reader into `CoreConfig.local_time`. Until then Linux logs a warning at start and uses UTC
    (BUILD.md section 3.3.3).
- After the owner's answer on #113: `PrivateBus` starting its `dbus-daemon` through
  `eileen_supervisor::spawn`, with tests that the daemon dies on drop and on panic.
- The rest of PL7, an amendment to WP0's `eileen-stdx`: `spawn_named` states Linux's 15-byte
  limit and checks it, and its test's thread gets a name that fits.
- The cgroup scope of LINUX.md section 5, which kills what outlives a supervised child in a
  crash or leaves its group, and sets a Claude session's CPU weight. It also gives each `Jobs`
  entry its scope: the job memory limit and `memory_limit_hit`, and `in_any_job` for a process
  that left its session.
- `TextInsertion` on the AT-SPI2 client, then input and capture through the portals.

Pull request #26 was one large change, 5,192 lines in 56 files, more than BUILD.md section 6.1's
small pull requests allow. Since then each pull request has one purpose, and a plan change ships
as an amendment before the code that needs it, as pull request #22 did for `wp0/base-crates`. A
branch that needs an unmerged pull request sits on its head. Through 2026-10-25, when the pull
request below it merged, the branch moved with `git rebase --onto origin/main <old parent>`. Now
a branch is kept current by merging `main` in, as AGENTS.md asks, never by rebasing a branch that
is already pushed, and a branch built on a pull request that merged merges `main` in and then
builds on `main`.

The D-Bus work was first built as one branch per backend on `Ubuntu-Compatible-L1` (5559c92 and
92191c8, offered as #29). A critic of the six backends found the same helpers written several
times, with different deadlines, so they moved into one crate, `eileen-dbus-linux`: `DbusError`
and its mappings, the buses with a method timeout, name owners and their signals, the
signal-stream conventions, the portal `Request` client, and a private bus for tests behind its
`private-bus` feature. Each backend was then ported onto it as a stack of small pull requests,
reviewed and fixed. Stacks in the same crate edit the same `lib.rs`, so each crate's stacks merge
as one chain, in the critic's order: session, notifications, then power in `eileen-shell-linux`;
open, recent, then MPRIS in `eileen-apps-linux`. The two chains do not touch each other's files.
They merged on 2026-10-22. The repository squash-merges, and GitHub's "Update branch" cannot
replay a stacked branch whose lower commits a squash replaced, so after each merge the next
branch was rebased onto `main` with `git rebase --onto origin/main <old parent>`, its tree checked
against the tested one, and pushed with `--force-with-lease`.

The follow-ups after the chains merged as #55 to #57, and the trait implementations' own (#64 to
#66, #68 and #84) since, and the L2 pull requests' (#87, #90, #107, #110); #118 finished SU5,
and #122, #123 and #124 fixed what this list named for the tray, RealtimeKit and MPRIS on
2026-10-26. Still open:

- The callers' deadlines for the file reads and writes, under the file-deadline record below.
  WP0 chose both a deadline parameter and a named helper thread: BUILD.md section 3.4 says so
  since #76, and the owner's #77 (open) changes the signatures. On `main`, `LinuxApps::index` and
  `LinuxRecent::recent` read on a named thread within `DEFAULT_DEADLINE` (`bounded`, #66), and
  `set_run_at_sign_in` writes on the caller's thread with no bound. #116 (into #77) moves the
  adapters to the caller's deadline; once both merge, which is the owner's call,
  `set_run_at_sign_in` takes the deadline and still writes on the caller's thread, and moves to
  a named helper thread, abandoned past the deadline. `open_url` and `open_uri` take no deadline
  in the trait and keep `DEFAULT_DEADLINE`. `bounded::within` could move to `eileen-dbus-linux`
  and serve both crates.
- `SecretStore`, which waits for the owner's answer on #111 (the SecretStore record below).
- `lint-repo` check 14 for tests that start a process: the private bus's `dbus-daemon` in
  `crates/eileen-dbus-linux/src/testing.rs` (the 2 problems `main` reports) and #94's copy of its
  test binary wait for the owner's answer on #113.
- The GlobalShortcuts holder (#107): section 3's fallback on Ubuntu 24.04 (GNOME custom shortcuts
  that run `eileen ctl`); following a portal that restarts, which loses the session without a
  word, so the shortcuts stay dead until a changed set is bound and `watch_input` hears nothing
  more; a change the user makes later in the desktop's settings, which does not reach the sink;
  and `PhysicalActivity`, which `watch_input` never sends, since no source of it is chosen yet.
- The tray (#110, #122): a watcher that never answers keeps the holder's registration under way
  for its connection's method timeout, and each `set_tray` meanwhile spends half its deadline
  before it answers `Unavailable`; and a watcher that never answered, or refused the first
  registration and the three retries the next calls make (`RETRIES`), is not asked again until
  its name gets a new owner, so until then no host shows the item (`host.rs`'s tray section and
  Traps).
- For the owner, from #107 and #110: a hotkey's press and a tray choice carry no device on Linux
  (any program on the session bus can send dbusmenu an `Event`), so they cannot meet BUILD.md
  sections 2.5 and 2.9's rule for approve, deny and unmute; and `ShellHost` has no way to return
  a press or a tray click to the core on any OS (section 3.4).
- The AT-SPI2 client (#87): its connect is unbounded. `accessibility_bus` connects through
  `eileen_dbus_linux::bus_at_within(&address, BUS, call_timeout)`, which refuses an address that
  is not a Unix socket's and gives the connection `call_timeout` as its method timeout, but
  zbus 5.19 bounds no connect, its handshake and `Hello` included (Surprises), so a socket that
  accepts and never answers holds `accessibility_bus` until the caller's deadline (the crate
  doc's Traps);
  the first reads of a just-launched app get 800 ms where libatspi allows up to 15 s (the AT-SPI2
  record below); and whether its rule for an error name an app chose (`APPS_OWN_ERROR`) moves into
  `DbusError::from_error_name` for every service, MPRIS players being apps too, is
  `eileen-dbus-linux`'s next pull request's decision.
- RealtimeKit (#90, #123): a thread that is already real-time when it registers (registered a
  second time, or made so by another library through rtkit) is taken off real-time. The
  fallback compares nice levels only, so when rtkit refuses, it moves such a thread to
  `SCHED_OTHER`, and so does the revert of a grant, since rtkit has no call that puts a
  real-time policy back (`eileen-audio-linux`'s crate doc, Invariants and Traps).
- The supervisor (#106, #115, #118): the cgroup scope (Planned above), and idle I/O priority for
  Claude sessions and builds, which needs an `ioprio_set` call (LINUX.md section 5).
- The three Linux fuzz targets (#109) have never been built: no workflow runs `cargo fuzz build`
  yet. Until WP0 phase B's fuzz job, the owner or CI builds them from `fuzz/` on Linux after
  copying the root `Cargo.lock` there.
- The desktop lane (#83): tests that need input or a clipboard need a compositor with a seat; and
  the owner profile's `all()` filter takes the `linux_session` tests, so it must not run on a
  Linux desktop (it is harmless on the owner's Windows PC, where that binary is empty). Since
  #121 the first run on a new runner image misses the package cache and downloads from the
  mirror, so on a slow mirror that run fails at the install step's 10-minute limit, and the next
  run tries again: so it went on 2026-11-06 (run 42210445439, the first on image
  20261101.186.1), and the next night's run saved the key. Its rust-cache step finds no cache on
  `main` (the open #144 above amends that step's key).
- `notifications.rs`'s own mapping of the server's error names (`already_gone`) moves into
  `DbusError`'s mappings.
- A read-only file system (`EROFS`) is `Io` in both `FileError` and `AutostartError`; if it should
  be `Refused`, both crates change together.
- The blocking-pool spawn forms: #84's follow-up is an amendment that bans them too, once it
  settles the `-linux` crates' calls, the pool seams `LinuxApps::reading_files_on` and
  `Opener::checking_files_on` and the test helpers that hold blocking threads
  (`stall_file_checks`, `take_blocking_threads`, `hold_the_blocking_thread`). The answer #84's
  entries name is a thread from `eileen_stdx::thread::spawn_named`.
- MPRIS (#65, #68, #124): each player's read is bounded as a whole (`READ_BOUND`, one method
  timeout), and the listing and the watch read their players at once, but the bus itself can
  still hold `LinuxMedia::sessions` past its deadline: connecting has no timeout, and the listing
  asks for the players' names and owners one call at a time before it reads them (`LinuxMedia`'s
  doc). The watch reports in the order its reads started, so a player that never answers holds
  back the reports of the players behind it, whose reads go on, until its one bound.
- From #66: the index's app names are a desktop entry's text, unsanitized; the threads that hung
  reads and checks keep are capped only where the caller gives a pool, which `LinuxRecent` and the
  opener `LinuxApps` makes per call do not take; the opener's calls before its wait each keep the
  method timeout, so a slow one can still let the bridge's deadline end the call with the portal's
  request left open; and which process ID a Flatpak app's `Activate` reply carries is yet to be
  measured on Ubuntu 26.04.

Not started yet:

- `MachineProbe` on Linux, which waits for WP0 phase B's `wp0/machine-contracts`, the owner's
  draft #191; until then `LinuxMachineProbe`'s readers mirror the contract shapes;
- the Linux forms of the `ipc` module, `PathProbe` and `LocalTime` from `/etc/localtime`, which
  wait for WP2's pull requests (Planned above);
- launching apps without D-Bus activation, which waits for WP0's `eileen_supervisor::user_launch`,
  phase B, which is not on `main` (the Linux process model it starts children with is, #106;
  LINUX.md sections 5 and 7);
- `RecentSources::watch` and `thumbnail`, and `Apps::main_window`, which answer `Unavailable`
  until L2;
- the conformance suites of LINUX.md section 6;
- the default audio endpoints, which wait for L2's audio-server client (the owner's choice of
  crate), and `SystemVolume` in `eileen-desktop-linux`;
- wiring `eileen-desktop-linux` and `eileen-audio-linux` into their processes, which waits for
  WP5's and WP4's composition;
- the rest of L2 (`TextInsertion`, input and capture through the portals, the HUD, slice S1 on
  Linux) and L3.

## Findings for other packages

The Linux lane runs other packages' branches before they merge (LINUX.md section 6). On
2026-10-21 it ran over WP0's unmerged base crates, `backup/wp0-base-crates` at ca336be: 305 of
305 tests passed, and clippy and cargo-deny failed, two problems that the Windows jobs cannot
show. Reading the guard's Linux memory code then found a third.
[LINUX.md section 10](../LINUX.md#10-what-the-linux-lane-found-already) has the detail. With the
three patches below, as commits on ca336be, that branch passes every Linux lane with 306 of 306
tests. WP0 fixed all three on its own branch, as this write-up proposed (c74a8c8, 06898a7 and
9f27f20), and #73 brought them to `main` (a67d096). The QoS test ends the boost with its scope, the
lock has section 4.1's tokio (1.53.3 by then, #59), and the guard reads commit headroom by
`vm.overcommit_memory` (`eileen-buildguard`'s `memory.rs`).

1. **A Linux-only clippy error** in `eileen-supervisor`. The test `an_audio_boost_reverts_on_drop`
   (`qos.rs` line 106) calls `drop(boost)`. Off Windows `AudioBoost` has no `Drop`, so
   `clippy::drop_non_drop` fails under `-D warnings`. The patch puts
   `cfg_attr(not(windows), expect(clippy::drop_non_drop, reason = "..."))` on that statement.
   Tested patch ready.
2. **A yanked pin.** `tokio` 1.54.0, BUILD.md section 4.1's pin, is yanked, so
   `cargo deny --locked check` fails. The patch pins 1.53.2 in the base crates' lock. #27 made
   the same change in BUILD.md section 4.1 and the root manifest. Tested patch ready.
3. **Commit headroom that Linux does not enforce** in `eileen-buildguard`. The guard reads
   `CommitLimit` minus `Committed_AS`, which can read 0 under default overcommit while gigabytes
   are free, and slot 0's rule then refuses every build. The patch applies the overcommit rule
   recorded below, with a unit test of both modes. Tested patch ready.

On 2026-10-25 WP10 reviewed #73's `eileen-trace`, the logging stack the Linux crates write
through, and confirmed 17 findings, numbered TR1 to TR18 with no TR14 among them. Each is fixed by
an amendment from WP10 with a regression test, or by WP0's #75:

| Findings | What was wrong | Fixed by |
|---|---|---|
| TR1, blocking | The core's log pruning deleted the other processes' log files | #79 |
| TR4, TR5, TR13 | The span-parent check reported parents that are there as missing, and its remote lookup was untested | #81 |
| TR3, TR8, TR12 | The ring dump redacted its own structure, a key header with no end line broke its JSON, and the ring's bound counted lines' lengths, not their allocations | #82 |
| TR7, TR11 | A card number followed by its expiry or CVV was not found; and a registered literal could never be removed (fixed by #75, with #88's regression test) | #88 |
| TR6, TR10, TR15, TR18 | A `log` record lost its origin, the PII scan missed Linux and WSL user folders and took dates for phone numbers; and the crate doc's Boundaries left out `thiserror` (fixed by #75, with #89's regression test) | #89 |
| TR2, TR16, TR17 | A logged `SpanTrace` showed `pii.` fields, the log writer's thread name was cut on Linux, and `init` could half install | #86 |
| TR9 | The console wrote on the logging thread, so a stopped terminal blocked it | #92 |

The same two pull requests, #86 and #92, also fixed what their own reviews found: `init` still
half installed under a scoped default, and at an exit with a full console buffer tracing-appender's guard printed to the same
stopped terminal and held the process. Found and not fixed: a span trace writes each span's
`at <file>:<line>` from static metadata, which for a registry crate's span is a path under the
build PC's home folder; and when the file sink's buffer is full at exit, tracing-appender prints
"Failed to send shutdown signal" on stdout, which carries a sidecar's protocol.

The same review of #73 confirmed findings in WP0's `eileen-supervisor` (SU1 to SU6), in
`eileen-rt` and `eileen-stdx` (PL7) and in `eileen-buildguard` (BU5). Each is fixed, or partly
fixed, by an amendment from WP10 with a regression test:

| Finding | What was wrong | Fixed by |
|---|---|---|
| SU1 | Dropping a `Child` killed only its leader, and nothing tied the children to the core, so a crashed core left them running | #106 |
| SU2 | `spawn_detached` dropped the guard without waiting on Unix, so an exited guard stayed a zombie that a liveness check by PID reported alive | #117 |
| SU3 | `try_wait` reaped behind tokio's back, and the drop later killed the freed PID by number | #106 |
| SU4 | On Linux a Claude session ran at the core's nice value | #115, for the nice value; its CPU share against the core under autogroup is the cgroup scope's |
| SU5 | stderr was piped but drained only when a caller asked, so a child writing more than the pipe holds blocked and `wait` never returned | #115 in part (an awaited `wait` on Linux drained it), then #118: `spawn` forwards stderr on every OS, on Windows from a named thread so that a quiet child holds no blocking-pool thread, and `ChildSpec::raw_stderr` hands a caller the raw pipe instead |
| SU6 | A child stayed in the core's session, so reading `/dev/tty` from a background group stopped it with `SIGTTIN` | #106 |
| PL7 | Thread names longer than the 15 bytes Linux keeps were cut: `<name>-worker` in `eileen_rt::runtime::build`, `eileen-log-writer` in `eileen-trace`, and `spawn_named` checks none | #105 for `eileen-rt`, #86 (as TR16) for `eileen-trace`; `eileen-stdx`'s half is still open (Planned, above) |
| BU5 | Off Windows cargo ran as a plain child at nice 0, a kill took cargo alone, and a killed `cargo xtask` left cargo building while its slot was already free | #108 |

The guard's fix, #108, leaves three limits, documented in the guard: a process that makes its own group or session
(cargo nextest runs each test so) leaves cargo's group and runs to its end; the build is not in
the terminal's foreground group; and a program `nice` cannot start exits 127 instead of failing
the spawn.

For WP2, from LINUX.md section 5's plan for the Linux forms of the `ipc` module and `PathProbe`
(decision D86). None of WP2's pull requests for them is open yet, so these are suggestions for
their interfaces, which can stay neutral and still take the Linux forms behind the same names:

1. **`Peer` from a connection.** Let a `PipeConn` give its peer (`client_peer()`,
   `server_peer()`) beside `Peer::open(pid, since)`. On Linux the connection carries the peer's
   pidfd (`SO_PEERPIDFD`), which leaves no window for a reused process id; opening by number
   leaves a 10 ms one, since `/proc/<pid>/stat` counts a start in clock ticks.
2. **`since` stays the module's own.** Keep it a value of the module that `Peer` compares, not a
   wall-clock time in the interface: Linux compares a boot-clock start time with it, Windows a
   `FILETIME` from `GetProcessTimes`.
3. **Bare, short names.** `PipeServer::bind` and `PipeConn::connect` take a name, such as
   `eileen-dispatch-<session>`, never a path or a `\\.\pipe\` prefix: a Unix socket's address
   holds 108 bytes.
4. **An error for an unsafe folder.** Linux refuses to bind or write a token when its folder is
   not the user's with mode 0700, or when `XDG_RUNTIME_DIR` is unset. An `IpcError` variant for
   that, apart from `NameTaken`, keeps the incident plain.
5. **`tcp_client_pid` may name no process.** After a `fork` several processes hold one socket,
   where Windows' table names one owner, so an `Option` lets the Linux form answer that no single
   process owns it, which the endpoint refuses.
6. **What being in a job means.** On Linux a process can leave its job (its session, with
   `setsid`) but cannot join another, so `PeerFacts.jobs`' doc and the role tests can say "in"
   without assuming it cannot be left; the tests that a worker's process is refused hold on both
   OSes. `ChildSpec::job_memory_limit_mb` and `breakaway_ok` do nothing on Linux until the
   cgroup scope, and `Child::memory_limit_hit` never resolves there, which their docs can say.
7. **`PathProbe`'s paths are the OS's.** BUILD.md section 3.4 says the trait's paths are strings
   as Windows writes them, and the classifier's grammar (drives, UNC, `\\?\`) is Windows'; on
   Linux a fully qualified path starts with `/`. `full_path` can apply `..` only where the OS
   reads it lexically: POSIX resolves it after the link before it, so the Linux probe leaves it
   to `final_path`.
8. **`link_names` with the count.** Linux cannot list a file's other names. If `link_names`
   answered the link count beside the names found, the classifier could take fewer names than
   the count as `Unknown` on any OS, with no Linux rule of its own.
9. **`handler` is the extension's.** On Linux the program that opens a file follows its MIME
   type, which GIO may take from the file's first bytes, so `runs_code` cannot rest on the
   extension's handler alone there; the trait's doc can say the answer is the extension's
   default.
10. **`share` needs no change.** No Linux path has a host and a share. A mount that shows this
    machine's files under another name, or reaches another host, is `final_path`'s `Refused` on
    Linux, which the classifier already takes as `Unknown`.

## Surprises

- zbus 5.19 bounds no connect. `Builder::build` was still connecting after 3 s, with a 500 ms
  method timeout, both against a socket that accepts and never answers and against a server that
  finishes the SASL handshake and never answers `Hello` (#62). So a hung bus holds a bridge job to
  its whole deadline, and the keep-awake holder bounds its connects itself (#85; the record "A
  job may bound a bus connect" below).
- GNOME's GlobalShortcuts portal is not a key's state: `Activated` repeats while a key is held,
  and no `Deactivated` comes for a key let go after one of its chord's modifiers (LINUX.md section
  3, from #93). Hold-to-talk on GNOME cannot rely on the release.
- On 2026-10-26 `cargo deny --locked check` failed on `main`'s lock, which was green on
  2026-10-25: four RustSec advisories had their ids assigned that day, RUSTSEC-2026-0391, -0398,
  -0403 and -0410, against `zstd` 0.13.3, `zstd-safe` 7.3.0 and `zstd-sys` 2.1.1, which `zip`
  2.6.1 brings in through `sherpa-onnx-sys`'s build script. Every pull request's `hygiene` job
  failed on them until #114 ignored them in `deny.toml` with three bans (decision D80, accepted on
  `main`), and cargo-deny passes on `main` since 1013e1c. Ten informational `unsound` advisories
  against `zstd-safe` 7.3.0 pass only because `deny.toml` sets no `unsound` scope, which D80 leaves
  to the owner.
- `PR_SET_PDEATHSIG` fires when the thread that forked the child ends, not the process, so a
  child spawned from a tokio blocking-pool thread, which exits after 10 s idle, would die with it
  (#106). An unreaped leader, even a zombie, keeps its PID, so killing its group by that PID is
  safe exactly while tokio still holds it.
- `setsid` gives every child a new autogroup, which defeats nice-based ordering against the core
  in the root CPU cgroup on Ubuntu's default kernels (#115).
- tokio reaps only the children it spawned, so nothing reaped the detached guard on Unix (#117).
- The build guard's slot lock lives on `cargo xtask`'s close-on-exec descriptor, so a killed
  `cargo xtask` freed the slot at once while cargo kept building (#108).
- RealtimeKit refuses every client that speaks only D-Bus on a default Ubuntu desktop: Linux's
  `RLIMIT_RTTIME` is unlimited by default, and rtkit asks for a hard limit at most its
  `RTTimeUSecMax`, with an `AccessDenied` that cannot be told from polkit's. rtkit 0.14 keeps a
  record of each thread's priority, which `MakeThreadHighPriority` leaves, and makes recorded
  threads real-time again after a suspend; rtkit 0.13 answers `Get` but not `GetAll`, so zbus's
  property cache cannot be used (#80, #90).
- notification-daemon crashes on Wayland, so the desktop lane runs it on an Xvfb display of its
  own; WirePlumber exits if it starts before PipeWire's sockets; logind refuses a VT number on a
  seat without VTs and one another session holds; and xdg-desktop-portal exports `Inhibit`
  whether or not its backend can start, so the lane pings the GTK backend too (#83).
- polkit on Ubuntu 26.04 gives `org.freedesktop.login1.inhibit-block-sleep` `auth_admin_keep` to
  any process and `no` on 24.04, so a process outside every session cannot take the sleep lock
  unattended (#83).
- tracing-error's `ErrorLayer` formats a span's fields once, when the span opens, and has no
  `on_record`, so a span trace shows only the fields a span had then (#86). tracing-appender's
  guard prints to stdout when its buffer is full at drop (#92).
- Cargo unifies features across the build, so one crate that asks for `zbus` with its default
  features turns them on for every crate. The Secret Service keyring store does, which is why
  `SecretStore` waits (its record below).
- `cargo xtask` finds the repository from `CARGO_MANIFEST_DIR`, fixed when xtask compiles
  (`repo_root` in `xtask/src/lib.rs`). With a `CARGO_TARGET_DIR` shared between worktrees,
  `cargo xtask pr-meta` ran a binary that another worktree had built, and checked that worktree's
  head. Each worktree keeps its own target folder, as BUILD.md section 6.1 rule 6 asks.
- On Ubuntu 26.04 GTK loads every image through glycin, whose loaders run in a sandbox that
  bwrap makes, and bwrap refuses to start in a non-root process that holds any permitted
  capability, a refusal glycin does not fall back from, so GTK 3 aborts on its first icon.
  pam_systemd in systemd 259 gives a regular user's seat login `CAP_WAKE_ALARM` in its ambient
  set, and systemd keeps it for a unit with `PAMName=`, so the lane's session unit leaves it out
  of its bounding set (#91).
- With no GNOME session manager, xdg-desktop-portal-gtk answers an idle `Inhibit` with
  `Response` 0 before its own `org.freedesktop.ScreenSaver` call returns, and that call fails
  where nothing owns the name, as in the lane's session: `DesktopHolder::Portal` there says that
  the portal took the request, not that the screen stays on (#91).
- The runner's Ubuntu mirror can serve the desktop lane's 99.6 MB of packages at 45 to 50 kB/s,
  a download that keeps moving, so no apt timeout ends it: the scheduled run of 2026-10-27 on
  `main` (40479985013) was cancelled at the job's 30 minutes inside the install. apt takes a
  trusted index's copy of a version over a local `.deb` of it, so the cached packages install
  without the mirror only from apt's archive folder, with `--no-download` (#121).
- zbus marks a connection whose socket failed closed only in the socket's reader, once it has
  failed the pending calls and handed the error to the connection's streams, so a caller that
  connects again through `Buses` right after the failure can get the failed connection back;
  `eileen-audio-linux`'s promoter waits for the mark, at most `METHOD_TIMEOUT` (#123).
- On Windows tokio reads a child's pipe as a blocking file, each pending read holding a thread
  of the blocking pool, so forwarding every child's stderr that way would fill the pool's 16
  threads; #118 reads it on a named thread of its own (`supervisor.err`).
- Under WSLg, `XDG_RUNTIME_DIR` is `/mnt/wslg/runtime-dir`, a tmpfs of mode 0777 without the
  sticky bit on this machine (2026-11-10), where the XDG Base Directory specification asks for
  the user's own folder with mode 0700, so Eileen's socket and `run/` folders check their own
  owner and mode and never rely on the parent's (LINUX.md section 5).
- rustix 1.1.5, the workspace's, wraps `SO_PEERCRED` but not `SO_PEERPIDFD` (Linux 6.5), so the
  one call that gives a peer's pidfd from its socket needs `libc`, which BUILD.md section 4.1
  does not pin (Planned).

## Decisions to record

Fourteen proposed decision records for the owner, in the shape of [DECISIONS.md](../DECISIONS.md),
whose last record on `main` is D86 (WP10's own D80 came from #114, accepted under the owner's
standing rule); the owner's open #169 adds two more, numbered D86 and D87 on its branch, which
conflicts with `main`. They have no numbers here: a record of these takes the next number that
neither `main` nor an open pull request's record holds when the pull request that adds it to
that log is published.
The pull request that adds them changes `docs/DECISIONS.md`, so it also updates
`docs/PLAYBOOK.md` or says why not (BUILD.md section 4b.3, check 30).

### Linux support, Ubuntu first, as WP10

- Date: 2026-10-21
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Decision: The plan supports Linux, Ubuntu first, through a new work package, WP10, whose text
  is LINUX.md section 8. The reference desktop is Ubuntu 26.04 LTS with GNOME on Wayland; Ubuntu
  24.04 LTS is supported where its desktop allows, and KDE Plasma and wlroots compositors come
  second. WP10 runs beside W0 to W6 on a Linux machine of its own, holds none of the owner PC's
  build slots, and lands in phases L0 to L3 with the acceptance of LINUX.md section 7. It owns the
  `-linux` crates (the five that mirror a Windows crate, and `eileen-dbus-linux`), `docs/LINUX.md`, `docs/wp/WP10-linux.md`, `scenarios/linux/` and, by
  amendment with WP0, the Linux desktop lane's workflow file. It does not own the traits, the
  contracts or the neutral servers.
- Why: The owner asked on 2026-10-21 that the plan support Linux (the entry proposed below).
  BUILD.md section 3.4 marks the traits "Linux later" and ROADMAP.md lists Linux for v0.2, but
  nothing said how, in what order or who builds it. A package on its own machine leaves the
  Windows waves and the owner PC's build slots as they are, and its small pull requests keep the
  owner's review time the only shared cost.
- Consequences: #31 added WP10 to BUILD.md as section 6.12. Nothing in L0
  or L1 makes a Windows package wait, and whether L2 ships with v0.1 or v0.2 is the owner's call.
- Source: the owner's ask of 2026-10-21 (the entry proposed below); LINUX.md sections 1, 3, 7
  and 8; BUILD.md sections 3.4 and 6; ROADMAP.md;
  [D38](../DECISIONS.md#d38-windows-first-not-windows-only-the-python-scaffold-is-retired).

### The -linux crates mirror the -win crates and compile empty elsewhere

- Date: 2026-10-21
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Amends: D38, which says a Linux port adds `-linux` crates and names none
- Decision: Linux implements the traits of BUILD.md section 3.4 in five crates, each the
  counterpart of a Windows crate in the same process: `eileen-shell-linux` (core),
  `eileen-audio-linux` (core and speech), `eileen-desktop-linux` (desktop), `eileen-apps-linux`
  (apps) and `eileen-hud-linux` (HUD). Each is gated with `#![cfg(target_os = "linux")]` and
  declares its dependencies only under `cfg(target_os = "linux")` target tables, so on Windows it
  is an empty library with no dependencies, as the `-win` crates are empty on Linux. The traits,
  the contracts and the neutral crates stay as they are; a Linux need that a contract or trait
  cannot express gets an amendment, not a workaround. One more crate, `eileen-dbus-linux`, has
  no Windows twin: it holds the D-Bus helpers the others share and depends on no workspace crate.
- Why: A port is new platform crates, not a rework of the core (D38; PLAYBOOK.md section 2.4).
  Pairing each Linux crate with a Windows one keeps one process map for both systems, and the
  empty build keeps Linux code and its dependencies out of the Windows jobs.
- Consequences: BUILD.md section 3.1's crate table gained the `-linux` crates in #31.
  `eileen-shell-linux` and `eileen-apps-linux`, merged in #26, already follow the rule and say
  `owns = "WP10"` in their metadata, and `eileen-dbus-linux` does too. Section 3.1 lists those
  three, and #78 and #80 added the rows of `eileen-desktop-linux` and `eileen-audio-linux`, whose
  crates are on `main` since #87 and #90 and follow the rule too.
- Source: LINUX.md sections 4 and 9; BUILD.md sections 3.1 and 3.4;
  [D38](../DECISIONS.md#d38-windows-first-not-windows-only-the-python-scaffold-is-retired) and
  [D55](../DECISIONS.md#d55-the-crate-graph-and-the-lint-bans-agree-secrets-volume-and-app-hooks-are-platform-traits);
  [PLAYBOOK.md section 2.4](../PLAYBOOK.md#24-platform-crates-behind-traits-windows-first-and-linux-next).

### D-Bus through zbus on Eileen's tokio runtime

- Date: 2026-10-22
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Decision: The Linux crates reach the desktop's D-Bus services through `zbus` 5.19.0 (MIT) with
  its default features off and only `tokio` on, so zbus spawns its tasks on Eileen's tokio
  runtime. Every client takes its `zbus::Connection` from the caller and never connects by
  itself. Tests serve fake services on a private bus, one `dbus-daemon` per test with no service
  activation, so a fake never meets a real service and CI needs no desktop. One `DbusError`, in
  `eileen-dbus-linux`, has the variants of `eileen-platform`'s `PlatformError` and keeps D-Bus
  error names and kinds, never a service's message text. Every method call is bounded by its
  connection's method timeout, 5 s; only a wait that a signal answers takes its own deadline.
- Why: zbus is pure Rust under a licence `deny.toml` allows. 5.19.0 (2026-08-09) was the newest
  release outside the cooling-off window on 2026-10-21; 6.0.0 (2026-10-16) is inside it. Its
  default features, `async-io` and `blocking-api`, bring a second async runtime, which zbus drives
  from a thread of its own when no tokio runtime is current, and a blocking API that Eileen does
  not use. `mpris-server` is MPL-2.0, outside `deny.toml`'s list, and the client is small enough
  to write.
- Consequences: #27 added the root manifest's `zbus` line; `eileen-dbus-linux` (order 8 above)
  holds the private bus, `DbusError` and the other shared helpers. A crate added later that turns
  on zbus's default features turns them on for the whole build, which is why `SecretStore` waits
  (the next record). Since #62 the traits reach the clients through `eileen-dbus-linux`'s
  `Bridge`, whose `Buses` connect on a job's first use, so a client still takes its connection
  from its caller. The record "A job may bound a bus connect" below adds a third bound, and "D-Bus
  clients of Eileen's own" keeps the other D-Bus crates out.
- Source: LINUX.md sections 4 and 6; 5559c92 and 92191c8 on `Ubuntu-Compatible-L1`; zbus
  5.19.0's `Cargo.toml` (`default = ["async-io", "blocking-api"]`) and its executor source, and
  crates.io's version list, read on 2026-10-22.

### SecretStore waits, because the Secret Service keyring store turns on zbus's default features

- Date: 2026-10-22
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Decision: The first L1 pull requests leave `SecretStore` out. LINUX.md section 4's route,
  `zbus-secret-service-keyring-store` 1.0.1 behind keyring-core, asks for `zbus` 5 and
  `secret-service` 5 with their default features, and Cargo unifies features across the build,
  so adding it would turn on zbus's `async-io` and `blocking-api` for every crate (the previous
  record). The other L1 backends land first, and the Secret Service route comes back to the owner
  as its own record.
- Why: One dependency would undo the tokio-only zbus build for the whole workspace. Deferring
  costs little now: the one secret allowed today, the DeepSeek key, is used by no v0.1 route
  (BUILD.md section 2.12).
- Consequences: L1's acceptance counts `SecretStore` among the D-Bus backends, so L1 stays open
  until it lands or the owner moves it. Windows is unchanged: `eileen-shell-win` implements it on
  Windows Credential Manager through keyring-core (BUILD.md sections 2.12 and 4.1). The route
  planned for the owner is a Secret Service client of Eileen's own on `zbus` with its default
  features off, a `plain` session and no prompts: #111 (`wp10/shell-secret-service`, a draft that
  waits for the owner's answer), which would supersede this record if accepted.
- Source: the `Cargo.toml` of `zbus-secret-service-keyring-store` 1.0.1, read on 2026-10-22;
  5559c92's message; BUILD.md section 4.1's comment (#27); LINUX.md sections 4 and 7; BUILD.md section 2.12.

### Linux commit headroom follows vm.overcommit_memory, in the runtime and in the build guard

- Date: 2026-10-21
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Amends: [D54](../DECISIONS.md#d54-memory-admission-and-the-build-guard-read-available-memory-commit-headroom-and-paging)
  (commit headroom, which BUILD.md section 2.5 reads as the commit limit minus the commit charge)
- Decision: On Linux, commit headroom is `CommitLimit` minus `Committed_AS` only under strict
  overcommit (`vm.overcommit_memory` = 2), the one mode in which the kernel enforces that limit,
  as Windows enforces its commit limit. In modes 0, the default, and 1 it is `MemAvailable` plus
  `SwapFree`, the memory that can still be backed. The runtime's sampler (`LinuxSystemMemory` in
  `eileen-shell-linux`) and the build guard (`eileen-buildguard`'s memory source) use the same
  rule, because both write the machine-wide memory samples (D69, D73).
- Why: In mode 0, `Committed_AS` can pass `CommitLimit` while gigabytes are free. The headroom
  then reads 0, and slot 0's rule refuses every build. Measured on 2026-10-21 under WSL2 with
  16 GB: a 12 GB mapping that was never touched left `MemAvailable` at 13,957 MB and took
  `CommitLimit` minus `Committed_AS` from 9,691 MB to 0 (LINUX.md section 10).
- Consequences: The guard's change is the third base-crates patch above, with a unit test of
  both modes, on `main` since #73. In `eileen-shell-linux`, the tests
  `a_sample_reads_available_memory_headroom_paging_and_the_largest_other_process` and
  `strict_overcommit_makes_the_commit_limit_binding` cover the two modes in the sampler. The
  sampler and the guard both still count 4 KB per major fault, which understates paging on kernels with 16 KB or 64 KB
  pages, so a fix there changes both.
- Source: LINUX.md sections 4 and 10; BUILD.md sections 2.5 and 6.1; D54, D69 and D73.

### tokio pinned to 1.53.2, because 1.54.0 is yanked

- Date: 2026-10-21
- Status: applied in BUILD.md section 4.1 by #27 (merged 2026-10-22), and since moved to 1.53.3 by
  #59; the owner numbers the record
- Amends: BUILD.md section 4.1's pin, `tokio` 1.54.0
- Decision: The workspace pins `tokio` 1.53.2 (2026-10-03), the newest release outside the
  14-day cooling-off window, with the same feature union as before. 1.54.0 (2026-10-19) is yanked
  on crates.io, so `cargo deny --locked check` fails on any lockfile that holds it.
- Why: cargo-deny is one of the lanes every pull request must pass, and it failed this way on the
  Linux lane over `backup/wp0-base-crates` at ca336be (LINUX.md section 10). Section 4.1's
  cooling-off rule picks 1.53.2: 1.53.3 (2026-10-13) and 1.54.1 (2026-10-21) become eligible on
  2026-10-27 and 2026-11-04. The owner may prefer 1.54.1, which is newer but inside the window.
- Consequences: #27 changed BUILD.md section 4.1 and the root manifest together, with a line in
  section 8's revision log. #59 then moved the pin to 1.53.3 for RUSTSEC-2026-0386, under section
  4.1's security-patch rule. The three findings of the Linux lane are fixed on `main`: #73 merged
  WP0's base crates with the pin's tokio in their lock, so `main` locks 1.53.3 and no lock on
  `main` holds the yanked 1.54.0.
- Source: crates.io's version list for tokio, read on 2026-10-22; LINUX.md section 10; BUILD.md
  section 4.1.

### File reads that can block on another process have a deadline

- Date: 2026-10-22
- Status: proposed (by the assistant, for the owner to accept, change or strike). BUILD.md
  section 3.4 states the rule for every platform since #76, so the owner may mark this record
  superseded.
- Decision: In the `-linux` crates, reading or writing a file in the user's home or data
  folders, which a FUSE or NFS mount can serve, is bounded by the caller's deadline, as a D-Bus
  method call is bounded by its connection's method timeout. Reads of `/proc` and `/sys`, which
  the kernel answers, stay unbounded.
- Why: BUILD.md section 2.1, rule 2: every call to another process has a deadline. A FUSE or NFS
  home is another process. `Opener::open_path` already bounds its file checks, while
  `RecentlyUsed::recent` and `set_run_at_sign_in` would block a thread for as long as a hung mount
  does. `eileen-dbus-linux`'s deadline model covers method calls and signal-answered waits only,
  and says not to change without a decision record, so the file rule needs its own. Dropping
  `open_path`'s bound instead would make the three agree by making all of them unbounded.
- Consequences: #76 chose a `Deadline` parameter and a helper thread for every synchronous method
  that reads the user's files, and the owner's #77 (open) changes the signatures, with WP10's
  #116 into it for the Linux adapters. On `main` the
  reads of `LinuxApps::index` and `LinuxRecent::recent` run on a named thread within
  `DEFAULT_DEADLINE` (#66), and `set_run_at_sign_in` still writes unbounded; the list under
  Progress has the rest. The L1 critic's finding 7 is closed when the adapters take the caller's
  deadline and `set_run_at_sign_in` writes within it.
- Source: the L1 critic of 2026-10-22, finding 7; `within` in `crates/eileen-apps-linux/src/open.rs`
  on `wp10/open-portal`; BUILD.md section 2.1.

### D-Bus clients of Eileen's own, in place of ashpd, ksni and atspi

- Date: 2026-10-25
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Amends: LINUX.md section 4's crate column as first written, which named `ashpd` 0.13.13 for
  the portals, `ksni` 0.3.6 for the tray and `atspi` 0.30.0 for AT-SPI2
- Decision: WP10 writes these D-Bus clients itself on `zbus` 5.19.0, as it wrote the MPRIS client
  and the desktop-entry parser: the portals through `eileen-dbus-linux`'s `Request`, the tray's
  `StatusNotifierItem` and dbusmenu objects with `#[zbus::interface]`, and AT-SPI2 and
  RealtimeKit as small proxies. All are on `main`: the GlobalShortcuts client, `ShortcutsSession`
  (#93), and the holder behind `LinuxShell` that uses it (#107); the tray (#110); the AT-SPI2
  client, `Atspi` (#87); and the RealtimeKit client, `RealtimeKit` (#90).
- Why: BUILD.md section 4.1 pins none of the three, and a new external crate needs its own
  amendment. The portal client they would bring already exists: `Request`, which the OpenURI and
  Inhibit portals use, and the GlobalShortcuts protocol takes four calls and three signals. A
  client of Eileen's own keeps the deadlines, the sender checks and the error rules of the other
  clients, on the zbus build the workspace has, tokio only.
- Consequences: The clients are Eileen's to keep current with the specifications. For
  GlobalShortcuts, following a portal that restarts (the host registry asks an app to register
  again on `NameOwnerChanged`) and the user's later changes (`ShortcutsChanged`) are follow-ups.
  The client passes GNOME's signals on as they come, repeats and missing releases included, and
  its caller decides what they count as: #107's holder counts a press only when the shortcut was
  up. The holder's session lives on a connection of its own, whose registry call is its first; a
  changed set is bound again in a new session, after the old one closes; and a refused change
  binds the set the desktop had again.
- Source: #93's description; the GlobalShortcuts and host Registry documentation;
  xdg-desktop-portal's `global-shortcuts.c` and `registry.c` and its NEWS for 1.19.4 and 1.21.0,
  xdg-desktop-portal-gnome's `globalshortcuts.c`, GNOME Settings' global shortcuts provider and
  mutter's `src/core/keybindings.c`, read on 2026-10-25; BUILD.md section 4.1; LINUX.md section 4.

### A job may bound a bus connect

- Date: 2026-10-25
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Amends: the deadline model of `eileen-dbus-linux` (its crate doc's Invariants, and
  `src/bus.rs`), and the record "D-Bus through zbus on Eileen's tokio runtime" above, which bounds
  method calls and signal-answered waits only
- Decision: A bridge job may bound a bus connect, the SASL handshake and the `Hello` call
  included, by a budget of its own counted from the trait call's start, since zbus 5.19 bounds
  neither. A connect not done by then is a bus that is there and has not answered:
  `DbusError::DeadlineExceeded` naming the bus, never `Unavailable`, which stays for a bus that is
  not there. The keep-awake holder in `eileen-shell-linux` bounds its two connects by half the
  call's deadline, and gives the portal's answers half of what is left. Method calls keep their
  connection's method timeout, and a wait that a signal answers its explicit `Duration`.
- Why: BUILD.md section 2.1, rule 2: every call to another process has a deadline, and a connect
  starts the first call on a bus. The bridge's deadline bounds a job as a whole, but a job with a
  holder on each of two buses needs a hung bus to cost it part of the deadline only, so that the
  other holder still answers in time.
- Consequences: Once the record is accepted, `eileen-dbus-linux`'s crate doc and `src/bus.rs` name
  the connect bound beside the other two, when the crate doc has room under its 80 lines. Other
  jobs connect without a bound of their own, within the bridge's deadline, as `DEFAULT_DEADLINE`'s
  doc says.
- Source: #85, on `main` as 2d5acb8; `DEFAULT_DEADLINE`'s doc in
  `crates/eileen-dbus-linux/src/bridge.rs`; BUILD.md section 2.1; #64's "Left for later".

### The desktop lane is a logind session on the hosted runner

- Date: 2026-10-25
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Decision: The nightly Linux desktop lane makes a real logind session of the runner's user:
  `.github/linux-desktop/session.sh` enters it through `systemd-run` with PAM's `login` stack and
  makes it the active session on seat0, so polkit's defaults apply as on a desktop. Considered
  and not taken: a seatless session with a polkit rule for the runner's user (a JavaScript rule,
  an owner decision, kept as the fallback should seat0 refuse the session), and a nested VM.
- Why: logind and polkit treat a process by its session, and the lock, the inhibitors and the
  power profile need an active local one, which a plain `dbus-run-session` on the runner is not.
- Consequences: Tests that need such a session live in `linux_session` modules in a crate's
  `tests/session/` binary, which the lane's `linux-session` profile runs, and the owner
  profile's `all()` filter takes too (Still open); the `ci` and default profiles leave them out.
  systemd 259's logind defaults to `RemoveIPC=yes`, so the runner user's SysV IPC and POSIX
  shared memory go when a session step's session is the user's last; no step of the lane keeps
  any between steps. PAM's `login` stack on seat0 gives the session's programs `CAP_WAKE_ALARM`,
  which stops bwrap, so the session's unit leaves it out of its bounding set (#91, Surprises).
- Source: #83, on `main` as d446972; #91, on `main` as b13e05c; `.config/nextest.toml`; BUILD.md
  section 4.6; LINUX.md section 6.

### What outlives a supervised child in a crash is the cgroup scope's

- Date: 2026-10-26
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Decision: On Linux `eileen-supervisor` kills a child's whole process group on drop, on
  `start_kill` and when the leader exits, and the parent-death signal kills each child when the
  core dies. What kills a group's members that outlive their leader when the core dies without
  unwinding, or that leave the group, is the cgroup scope of LINUX.md section 5 (`cgroup.kill`),
  which a containment pull request of its own brings. Until then they run on.
- Why: The kernel clears the parent-death signal on fork, so it never reaches grandchildren, and
  a process that calls `setsid` or `setpgid` leaves the group a kill reaches. Only a cgroup holds
  every descendant whatever it does.
- Consequences: `child.rs`'s module doc names the gap. BUILD.md section 4.7's invariant 8 holds on
  Linux for descendants that stay in the group until the scope lands. The same scope sets a
  Claude session's CPU weight, which a nice value cannot under autogroup (LINUX.md section 5).
- Source: #106, on `main` as 7bded52; #115; `crates/eileen-supervisor/src/child.rs`; LINUX.md
  section 5.

### AT-SPI2 through Eileen's own client on zbus, each call bounded at 800 ms

- Date: 2026-10-25
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Decision: `eileen-desktop-linux` reads the accessibility bus through `Atspi`, a client of
  Eileen's own on `zbus`, not the `atspi` crate. Each call waits at most `CALL_TIMEOUT`, 800 ms,
  from the first call to an app on.
- Why: BUILD.md section 4.1 pins no `atspi`; the four pull requests of the desktop lane need some
  fifteen calls; and the timeout, the walk's bounds, the password rule and the error rules stay in
  one place.
- Consequences: libatspi bounds a call to an app it first saw less than 15 s ago by what is left
  of those 15 s, and by 800 ms only after; this client keeps 800 ms from the start, so a read
  right after an app starts can time out where a screen reader waits. Whether the trait adapters
  give the first reads of a just-launched app a longer bound, within `eileen-dbus-linux`'s
  deadline model, is still open: draft #94 keeps `CALL_TIMEOUT`.
- Source: #87, on `main` as 4729302; libatspi 2.52's data, its `set_timeout` and its enum
  tables, and at-spi2-core 2.52's introspection.

### Real-time audio threads through RealtimeKit, with RLIMIT_RTTIME bounded to its maximum

- Date: 2026-10-25
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Decision: On Linux, `AudioEndpoints::register_audio_thread` makes the calling thread real-time
  (`SCHED_RR`) only through RealtimeKit (rtkit) on the system bus, never through `RLIMIT_RTPRIO`
  or `CAP_SYS_NICE`. Before its first request the process lowers its hard `RLIMIT_RTTIME` to
  rtkit's `RTTimeUSecMax` (200 ms by default) and keeps its soft limit within it, never raising
  either. It asks for priority 10, capped at rtkit's maximum, and falls back to nice -11, floored
  at rtkit's minimum, when rtkit refuses, unless the thread's nice level is already that level or
  lower, which it then leaves alone with nothing to revert (nice levels only: a thread already
  real-time still falls back). The ticket reverts the thread to the nice level it had, floored
  at rtkit's minimum, and then makes one more call, so that rtkit 0.14 forgets the thread's
  priority. A request whose answer never came, which rtkit may still grant, is reverted the same
  way at once: over the same connection after a method timeout, and over a new connection to the
  system bus after a failed one.
- Why: rtkit grants real-time only to a process whose hard `RLIMIT_RTTIME` is at most its maximum,
  and Linux's default is unlimited; it needs no privilege and no group, and polkit lets the active
  local session use it. Priority 10 leaves PipeWire's threads, which get rtkit's maximum, 20,
  above Eileen's [inferred: PipeWire 1.0.5 asks for 88, which rtkit refuses past its maximum].
  rtkit waits for polkit inside a call and acts after a caller has given up, and serves its calls
  in order, so a revert sent then lands after the grant.
- Consequences: The bound is process-wide and permanent, and every process the core starts after
  it inherits it; a real-time thread that runs past it without a blocking call ends the process
  with `SIGKILL`. WP4's audio threads register once each, before their first period, and end with
  their tickets. `eileen-audio-linux`'s crate doc lists the bound, and that real-time comes only
  through RealtimeKit, under "Do not change without a decision record".
- Source: LINUX.md sections 4 and 5; rtkit 0.13's and 0.14's `rtkit-daemon.c`; PipeWire 1.0.5's
  `pipewire.conf`; #80, on `main` as d4945f2; #90, on `main` as aac5055; #123, on `main` as
  2fda2e9.

### On Linux, the ipc sockets and run/ live under XDG_RUNTIME_DIR, and a job is a session until the cgroup scope

- Date: 2026-11-10
- Status: proposed (by the assistant, for the owner to accept, change or strike)
- Decision: On Linux the `ipc` module's sockets live in `$XDG_RUNTIME_DIR/eileen/` and BUILD.md
  section 2.15's `run/` in `$XDG_RUNTIME_DIR/eileen/run/`, each folder the user's own with mode
  0700, checked for owner and mode before a socket is bound or a token written. With
  `EILEEN_DATA_DIR` set both follow it; with no `XDG_RUNTIME_DIR` the `ipc` calls answer an
  error and fall back to no shared folder. Until the cgroup scope of LINUX.md section 5 exists, a
  caller's job on Linux is the session each child's `setsid` gives it: `PeerFacts.jobs` names
  the keys whose session the peer is in. Once the scope exists, `in_any_job` also counts a
  process under any entry's scope, and `jobs` stays by session.
- Why: The XDG runtime folder is the user's, per machine, local, mode 0700, and gone at the last
  logout and at reboot, so a per-boot token never reaches a disk, a backup or a second machine
  that shares an NFS home, as `~/.local/state/eileen/run/` would let it. A session can be left
  but not joined (`setsid(2)`, `setpgid(2)`), so membership by session cannot be forged, where a
  process of the user can write itself into a scope that the user's service manager made
  [inferred: L1 checks the owner of a user scope's files on Ubuntu 26.04].
- Consequences: LINUX.md section 5's data folder table gains the row and its path classes the
  folder, as `EileenData`. The runtime folder sits in memory under logind's size limit, so the
  sessions' folders under `run/` stay small. A process that leaves its session loses every role
  and passes the "outside every Eileen job" checks: BUILD.md section 7's open risk, cheaper on
  Linux, until v0.2's sandbox. Under WSLg the parent folder is 0777 (Surprises), which the
  folder's own check makes harmless. The `ipc` pull requests under Planned build it.
- Source: LINUX.md section 5; the XDG Base Directory specification; unix(7), setsid(2),
  setpgid(2), proc_pid_stat(5) and the cgroup v2 documentation, read on 2026-11-10; BUILD.md
  sections 2.12, 2.15 and 3.1; D86.

## Owner direction to record

A proposed entry for [OWNER-DIRECTION.md](../OWNER-DIRECTION.md), under its `## 2026-10-21`
heading. The owner gave this direction on 2026-10-21 outside the repository, and that file does
not hold it yet. Only the owner adds entries there.

> - **Linux in the plan, Ubuntu first.** The plan supports Linux as well as Windows, starting with
>   Ubuntu. Ayham works on the Ubuntu side and tests Eileen there, with free rein in how, as long
>   as the work respects BUILD.md's foundation and what is already built.

Once it is there, the first record above cites it as its source.

## Measured, and not yet verified

Measured so far, each where it is recorded:

- The Linux lane on `main` at daf8e13, under WSL2: every lane passes, 165 of 165 tests (LINUX.md
  sections 1 and 11). On `main` at db428f3, after #26 to #28: every lane passes, 229 of 229; at 39e389e, after #30, 231 of 231.
- #32's head, e5e83a3: every lane passes with 243 of 243 tests.
- The D-Bus pull requests #35 to #53, at each of their 19 heads on `main` at a5dfa2f: every lane
  passes, from 251 to 378 tests up the shell chain and 291 to 365 up the apps chain.
- `main` at 8734897, with every L1 D-Bus backend merged: every lane passes, 473 of 473 tests.
- `main` at 7fc7518, after the follow-ups #55 to #57: every lane passes, 489 of 489 tests.
- `main` at 5a4b11b, with the base crates, the bridge and the trait implementations: every job of
  CI run 40238334151 passes (2026-10-25), `logic-linux` with 948 of 948 tests on the
  `ubuntu-24.04` image, version 20261012.334.1.
- `main` at a84c198, after #117: every job of CI run 40414356056 passes (2026-10-26),
  `logic-linux` with 1057 of 1057 tests and the 4 `linux_session` tests skipped, on the
  `ubuntu-24.04` image, version 20261012.334.1.
- `main` at 7f493bf, after #124: every job of CI run 40564307456 passes (2026-10-27),
  `logic-linux` with 1078 of 1078 tests and the 9 `linux_session` tests skipped, on the
  `ubuntu-24.04` image, version 20261012.334.1.
- `main` at 18f71fd, after WP0 to WP4's 57 commits: CI run 42405160476 (2026-11-07) ran
  `logic-linux` on the `ubuntu-26.04` image, version 20261101.186.1, where `main`'s run of
  2026-11-02 (41375489424) still ran on `ubuntu-24.04`: 1807 of 1807 tests passed, and the ci
  profile skipped 10, the 9 `linux_session` tests among them. Its `windows-test` failed one test,
  WP1's `eileen-core::it::job_model::the_registry_agrees_with_the_reference_model`.
- #83's `session` job, on the hosted `ubuntu-26.04` runner: its four session tests pass against
  the real services in the headless session, at e24cb4a and at its last head 8aca526 (run
  40404308881 of 2026-10-26, on the `ubuntu-26.04` image, version 20261012.163.1: Ubuntu 26.04.1
  with systemd 259.5, which the job's package install moves to 259.9), in about 2 minutes with
  the Linux build cache restored. A run at 1ca07be failed after 34 minutes because
  WirePlumber started before PipeWire's sockets.
- #91's lane runs 40406035648 at 2d7d033 and 40414700997 at 54f9f5f, on `ubuntu-26.04`: nine
  `linux_session` tests pass. Its first run at 43b8fc8 failed the three notification tests:
  notification-daemon aborted on a GTK icon load because bwrap refused to start with a
  capability.
- The desktop lane's scheduled runs on `main`: run 40479985013 at 71b8dad (2026-10-27) was
  cancelled at the job's 30 minutes while apt downloaded the session's packages at 45 to
  50 kB/s (#121). Run 40668602200 at 7f493bf (2026-10-28), the first to pass, missed the package
  cache, installed from the mirror, saved the key, and passed 520 of 520 integration tests and
  9 of 9 `linux_session` tests in about 2.5 minutes. Every scheduled run since has passed but
  one: run 42210445439 at 7209390 (2026-11-06), the first on image 20261101.186.1, missed the
  cache and ended at the install step's 10 minutes, and run 42364908679 (2026-11-07) installed
  from the mirror and saved that image's key. At 18f71fd, runs 42472734272 (2026-11-08),
  42614231101 (2026-11-09) and 42809906168 (2026-11-10) restored the packages from the cache
  and passed 520 of 520 integration tests and 9 of 9 `linux_session` tests; the last took 4.5
  minutes, with no build cache found.
- #121's runs on `wp10/desktop-lane-apt` at 3ca7c67: a cold install from the mirror took 59 s
  and 63 s, and a warm one from the cache, with no download, 42 s (run 40526956903); one manual
  run on a slow mirror ended at the install step's 10 minutes (40524629042).
- #106 at a9a3536, under WSL2 (Ubuntu 24.04): the six supervisor process tests pass 30 of 30 runs
  with `--stress-count 30`, and a mutation that forks off the fork thread fails 30 of 30.
- #105, under WSL2: before the fix the kernel showed `eileen-guard-wo` for the thread
  `eileen-guard-worker`.
- #92 at 5a642af and 2c4385e, under WSL2 with a real pty and IXON on: before the fix the core
  stopped at 20 warnings after Ctrl-S, and with 5,000 warnings queued never exited until Ctrl-Q;
  after it, warnings kept returning and the exit took 1.25 s. The console's 4,096-line buffer
  costs 244 to 316 KiB resident at start, where tracing-appender's default 128,000 lines cost
  4,084 to 4,136 KiB.
- #114: `cargo deny --frozen check` on 5a4b11b, with the advisory database of 2026-10-26, failed
  its advisories step on exactly the four `zstd` advisories; at edf4515, with the ignores, every
  step passed (cargo-deny 0.20.2).
- This WSL2 machine's `/proc/self/limits` reads "Max realtime timeout unlimited unlimited", and
  Ubuntu 24.04's systemd leaves `DefaultLimitRTTIME` unset (#80).
- Pull request #26's head, 132c11b: every lane passes under WSL2 with 229 of 229 tests, and every
  job of CI run 39700538734 passes on the hosted runners (the pull request's Evidence).
- Under WSL2, in a release build, each live test reads the facts, the load and a memory sample in
  about 5 ms, against BUILD.md section 2.16's 200 ms budget, and the recorded WSL2 fixture matches
  a live reading field for field (the pull request's Evidence). The sampler's live sample matches
  the kernel's figures (LINUX.md section 7).
- Commit headroom under default overcommit, under WSL2 with 16 GB: the 12 GB mapping in the
  overcommit record (LINUX.md section 10).
- The base crates at ca336be: 305 of 305 tests, with clippy and cargo-deny failing; with the three
  patches, every lane passes with 306 of 306 (the findings above).
- The hosted runner: pull request #26's `logic-linux` job asked for `ubuntu-latest` and ran on the
  `ubuntu-24.04` image, version 20261012.334.1 (Ubuntu 24.04.5 LTS), as its log's header says.
- For LINUX.md section 5's `ipc` plan, this WSL2 machine (Ubuntu 24.04.4, kernel
  6.18.33.2-microsoft-standard-WSL2, glibc 2.39, `linux-libc-dev` 6.8.0) on 2026-11-10:
  `XDG_RUNTIME_DIR` is `/mnt/wslg/runtime-dir`, a tmpfs of mode 0777; `getconf CLK_TCK` is 100;
  Yama's `ptrace_scope` and `fs.protected_hardlinks` are 1; `/mnt/c` and `/mnt/d` are `9p`
  mounts of `C:\` and `D:\`; the kernel headers define `SO_PEERPIDFD` and
  `INET_DIAG_CGROUP_ID`; and an owner can set a file's sticky bit (`chmod 1600`).

Not yet verified:

- A real Ubuntu desktop. WSL2 has no GNOME, no portals and no PipeWire by default, and WSLg's
  compositor and audio are no measure of GNOME (LINUX.md section 6). L0's 200 ms budget has to
  hold on a real Ubuntu desktop, and L1's backends have to pass against the real services.
- Ubuntu 26.04, the reference desktop. The desktop lane's `session` job (#83, on `main`)
  runs there on a headless session rather than GNOME, and since `ubuntu-latest` moved, so does
  `logic-linux` (below).
- The hosted runner's move to 26.04. GitHub moves `ubuntu-latest` to 26.04 between 2026-10-19
  and 2026-11-19 (LINUX.md section 3). Pull request #26 ran on 24.04, and so did `main`'s runs
  through 2026-11-02 (41375489424); run 42405160476 at 18f71fd (2026-11-07) ran `logic-linux` on
  26.04 and passed. L0's acceptance needs the pull request lanes green on both images: each has
  passed on `main`, at different heads, and `ci.yml`, WP0's, has no job pinned to
  `ubuntu-24.04`, so 24.04 is no longer checked on each pull request.
- The L2 pieces on a real desktop. To verify on Ubuntu 26.04, attended: that GNOME's
  `AppIndicator` extension is on by default and honours `ItemIsMenu`, and that Breeze names the
  muted microphone icon as Adwaita does (#110); the shortcuts holder against GNOME 50's portal
  (#107); the AT-SPI2 client against real apps on a real accessibility bus (#87); and a thread
  registered with RealtimeKit, with `chrt -p` (#90).
- The points of LINUX.md section 5's `ipc` and `PathProbe` plan marked [inferred], which their
  pull requests test first: that a `/proc/<pid>` descriptor fails once its process is gone; that
  the claude CLI keeps the shim and its hooks' connections in its own session; the cost of the
  `/proc` walk behind `tcp_client_pid`; the owner of a user scope's files; the kernel of Ubuntu
  26.04; and that the walk of `final_path` mounts nothing.

## Outcome

Open.
