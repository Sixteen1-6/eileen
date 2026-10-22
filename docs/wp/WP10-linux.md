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
| 10 | this pull request | `docs/wp10-progress` | This file's progress, and LINUX.md's L1 rows and status as built |
| none | #29 | `Ubuntu-Compatible-L1` | Closed: the first D-Bus work as one pull request, replaced by orders 8 and 9 |

Pull request #26 was one large change, 5,192 lines in 56 files, more than BUILD.md section 6.1's
small pull requests allow. Since then each pull request has one purpose, and a plan change ships
as an amendment before the code that needs it, as pull request #22 did for `wp0/base-crates`. A
branch that needs an unmerged pull request sits on its head. The repository squash-merges, so
when the pull request below merges, the branch moves with `git rebase --onto origin/main <old
parent>` and its lanes and `pr-meta` run again.

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

Next, after the chains, each its own pull request:

- `wp10/dbus-streams`: the subscribe-and-drain helpers that three backends still write
  themselves, a portal `Close` that waits for its answer, and logind's `NoSessionForPID` in
  `DbusError`'s mappings.
- Deadlines for `RecentlyUsed::recent` and `set_run_at_sign_in`, under the file-deadline record
  below.
- `RecentlyUsed::bookmarks` refuses a FIFO or a device in `recently-used.xbel`'s place instead of
  blocking on its open, as `open_path` already does. The chain's review found it; the fix and its
  test are written and wait for this follow-up.
- `AutostartError::Refused` for a folder that refuses the write, as `FileError` has.
- One naming rule for change streams, and the `keep_awake` exception (a connection taken by value,
  because dropping it ends the hold) in `eileen-dbus-linux`'s Invariants.
- LINUX.md's L1 rows and status, as the backends merge; each description lists its lines.

Not started yet:

- the trait implementations, which wait for `eileen-platform` (on the unmerged base crates) and,
  for the machine facts, WP0 phase B's `wp0/machine-contracts`; until then the crates mirror the
  trait and contract shapes;
- `SecretStore` (its record below);
- launching apps without D-Bus activation, which waits for the supervisor's Linux process model
  (LINUX.md section 7);
- the nightly Linux desktop lane, whose workflow file WP0 owns;
- L2 and L3.

## Findings for other packages

The Linux lane runs other packages' branches before they merge (LINUX.md section 6). On
2026-10-21 it ran over WP0's unmerged base crates, `backup/wp0-base-crates` at ca336be: 305 of
305 tests passed, and clippy and cargo-deny failed, two problems that the Windows jobs cannot
show. Reading the guard's Linux memory code then found a third.
[LINUX.md section 10](../LINUX.md#10-what-the-linux-lane-found-already) has the detail. With the
three patches below, as commits on ca336be, that branch passes every Linux lane with 306 of 306
tests. The owner chooses whether they arrive as patch files or as a branch.

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

## Surprises

- Cargo unifies features across the build, so one crate that asks for `zbus` with its default
  features turns them on for every crate. The Secret Service keyring store does, which is why
  `SecretStore` waits (its record below).
- `cargo xtask` finds the repository from `CARGO_MANIFEST_DIR`, fixed when xtask compiles
  (`repo_root` in `xtask/src/lib.rs`). With a `CARGO_TARGET_DIR` shared between worktrees,
  `cargo xtask pr-meta` ran a binary that another worktree had built, and checked that worktree's
  head. Each worktree keeps its own target folder, as BUILD.md section 6.1 rule 6 asks.

## Decisions to record

Seven proposed decision records for the owner, in the shape of [DECISIONS.md](../DECISIONS.md),
whose last record is D76. They have no numbers, because the owner assigns them when they join that log.
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
  `owns = "WP10"` in their metadata, and `eileen-dbus-linux` does too.
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
  (the next record).
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
  Windows Credential Manager through keyring-core (BUILD.md sections 2.12 and 4.1).
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
  both modes. In `eileen-shell-linux`, the tests
  `a_sample_reads_available_memory_headroom_paging_and_the_largest_other_process` and
  `strict_overcommit_makes_the_commit_limit_binding` cover the two modes in the sampler. The
  sampler and the guard both still count 4 KB per major fault, which understates paging on kernels with 16 KB or 64 KB
  pages, so a fix there changes both.
- Source: LINUX.md sections 4 and 10; BUILD.md sections 2.5 and 6.1; D54, D69 and D73.

### tokio pinned to 1.53.2, because 1.54.0 is yanked

- Date: 2026-10-21
- Status: applied in BUILD.md section 4.1 by #27 (merged 2026-10-22); the owner numbers the record
- Amends: BUILD.md section 4.1's pin, `tokio` 1.54.0
- Decision: The workspace pins `tokio` 1.53.2 (2026-10-03), the newest release outside the
  14-day cooling-off window, with the same feature union as before. 1.54.0 (2026-10-19) is yanked
  on crates.io, so `cargo deny --locked check` fails on any lockfile that holds it.
- Why: cargo-deny is one of the lanes every pull request must pass, and it failed this way on the
  Linux lane over `backup/wp0-base-crates` at ca336be (LINUX.md section 10). Section 4.1's
  cooling-off rule picks 1.53.2: 1.53.3 (2026-10-13) and 1.54.1 (2026-10-21) become eligible on
  2026-10-27 and 2026-11-04. The owner may prefer 1.54.1, which is newer but inside the window.
- Consequences: #27 changed BUILD.md section 4.1 and the root manifest together, with a line in
  section 8's revision log. `main` locks no tokio yet; `eileen-dbus-linux` brings 1.53.2 into the
  lock. `backup/wp0-base-crates` locks 1.54.0, which its second patch moves to 1.53.2.
- Source: crates.io's version list for tokio, read on 2026-10-22; LINUX.md section 10; BUILD.md
  section 4.1.

### File reads that can block on another process have a deadline

- Date: 2026-10-22
- Status: proposed (by the assistant, for the owner to accept, change or strike)
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
- Consequences: A later pull request gives `recent` and `set_run_at_sign_in` a deadline (the
  list under Progress). The L1 critic's finding 7 is closed when it merges.
- Source: the L1 critic of 2026-10-22, finding 7; `within` in `crates/eileen-apps-linux/src/open.rs`
  on `wp10/open-portal`; BUILD.md section 2.1.

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

Not yet verified:

- **A real Ubuntu desktop.** WSL2 has no GNOME, no portals and no PipeWire by default, and WSLg's
  compositor and audio are no measure of GNOME (LINUX.md section 6). L0's 200 ms budget has to
  hold on a real Ubuntu desktop, and L1's backends have to pass against the real services.
- **Ubuntu 26.04**, the reference desktop. Nothing has run on it yet, and L0's acceptance needs
  the lanes green on `ubuntu-26.04` as well as `ubuntu-24.04`.
- **The hosted runner's move to 26.04.** GitHub moves `ubuntu-latest` to 26.04 between 2026-10-19
  and 2026-11-19 (LINUX.md section 3), and pull request #26 still ran on 24.04, so no WP10 run has
  shown the lanes on 26.04 yet. LINUX.md section 6 suggests pinning `ubuntu-24.04`, or running
  both, until the move settles; `ci.yml` is WP0's.

## Outcome

Open.
