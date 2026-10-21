# Eileen on Linux: the plan, Ubuntu first

A proposal, written 2026-10-21 after the owner asked that Linux be supported in the plan. It is for the owner, who decides what it changes in [BUILD.md](BUILD.md), and for whoever builds the Linux backends. It changes nothing in the Windows design. Linux implements the same platform traits of BUILD.md section 3.4, in `-linux` crates that mirror the `-win` crates, and the neutral crates stay as they are.

Sources are linked where they are used. Crate versions were read from crates.io on 2026-10-21; "eligible" means the newest release at least 14 days old that day (BUILD.md section 4.1's cooling-off rule). Statements marked [inferred] are reasoning that a later measurement must confirm.

## 1. Summary

- **What already holds.** Every platform-neutral crate builds and passes its tests on Linux, which CI's `logic-linux` job checks on every pull request (BUILD.md section 4.6). On Ubuntu 24.04 the full Linux lane passes on `main` (daf8e13: fmt, cargo-deny, cargo-machete, clippy with warnings denied, 165 of 165 tests, doc tests, docs).
- **What Linux needs.** Implementations of the section 3.4 traits, a Linux process model behind `eileen-supervisor`, Linux paths in the data directory and the path classes, and test lanes that exercise real Linux desktop services.
- **The reference desktop.** Ubuntu 26.04 LTS with GNOME on Wayland, which is the only GNOME session it ships. Ubuntu 24.04 LTS (GNOME 46) is supported where its desktop allows. KDE Plasma and wlroots compositors come second.
- **How it is built.** A new work package, WP10 (section 8), runs on a Linux machine of its own. It uses none of the owner PC's build slots and lands in four phases:
  - L0: memory, machine facts, identity, and the Linux lane;
  - L1: the D-Bus services;
  - L2: audio, accessibility, input and the HUD, ending in slice S1 on Linux;
  - L3: packaging and the Linux fleet.
- **What Linux cannot match without consent.** Wayland keeps clients from reading other windows' positions, injecting input or capturing the screen on their own. Eileen asks once through the desktop portals, and a few Windows behaviours (background input to a named window, a click-through overlay on GNOME) degrade honestly (section 3).

## 2. Where Linux stands in the plan today

- **The owner direction.** "Windows first, not Windows only": everything that touches the OS sits behind platform traits, and the neutral crates build and test on Linux from day one ([OWNER-DIRECTION.md](OWNER-DIRECTION.md), 2026-10-20; decision D38).
- **The roadmap.** It lists Linux as a v0.2 target, built as "`-linux` platform crates: AT-SPI, PipeWire, Wayland or X11, MPRIS, Secret Service" ([ROADMAP.md](ROADMAP.md)). BUILD.md section 3.4 marks the traits "Linux later": `MediaSessions` on MPRIS, `SecretStore` on Secret Service, `MachineProbe` on "/proc, sysfs and udev", and `AppHost` per app.
- **Already Linux-aware in the design.**
  - The speech pipe uses a Unix socket under `$XDG_RUNTIME_DIR` in Linux tests (BUILD.md 3.3.7).
  - The fleet's machine hash reads `/etc/machine-id`, then `/var/lib/dbus/machine-id` (owner answer 6; `eileen_platform::machine::FileMachineIdentity`).
  - `ConnectorManifest.platforms` and `MachineFacts.os` carry `Linux` (3.3.12, 3.3.17).
  - Native libraries appear only under Windows target tables, so the Linux lane never links them (4.1).

## 3. The reference desktop and what it allows

| | Ubuntu 26.04 LTS | Ubuntu 24.04 LTS |
|---|---|---|
| Desktop | GNOME 50 on Wayland only. The Xorg session is gone; X11 apps run through Xwayland ([OMG! Ubuntu](https://www.omgubuntu.co.uk/2026/04/ubuntu-26-04-lts-changes-since-24-04/amp)) | GNOME 46 on Wayland by default, with an Xorg session available |
| Global shortcuts | The GlobalShortcuts portal, whose GNOME backend arrived in xdg-desktop-portal-gnome 48 ([BLFS](https://linuxfromscratch.org/blfs/view/svn/gnome/xdg-desktop-portal-gnome.html)) | No portal backend; see below |
| Audio | PipeWire with its PulseAudio server [inferred: Ubuntu's default since 22.10] | The same |
| CI | GitHub moves `ubuntu-latest` to 26.04 between 2026-10-19 and 2026-11-19 ([GitHub changelog](https://github.blog/changelog/2026-09-17-ubuntu-26-generally-available-and-latest-migration)) | `ubuntu-24.04` stays available |

What GNOME on Wayland means for each part of Eileen:

- **Reading the desktop.** AT-SPI2, the accessibility bus over D-Bus, gives the application, window and element trees, states (focused, active, editable), actions and text, much as UI Automation does. Wayland gives a client no global window coordinates and GNOME exposes no protocol for listing other clients' windows, so `DesktopState` on Linux carries window-relative element boxes. Monitor geometry comes from the compositor's own D-Bus interface or `wl_output`, and the window list comes from AT-SPI's frames [inferred; L2 measures it].
- **Input and capture.** No client may inject input or read the screen without the user's consent. The XDG RemoteDesktop portal's `ConnectToEIS` hands Eileen an EIS socket for libei, after which input goes to the compositor directly ([libei portal integration](https://planet.gnome.org/page4.html)). The ScreenCast portal hands Eileen a PipeWire stream. Each asks once, and a restore token keeps the grant. The Windows rule "background input to a named window" has no Linux equivalent: input goes to the focused surface, so on Linux every keyboard and pointer action is a foreground action and takes the input lock and a lease (BUILD.md 2.6).
- **Global shortcuts.**
  - On 26.04, the mute key and the dictation key come through the GlobalShortcuts portal. Its `Activated` and `Deactivated` signals give press and release, so hold-to-talk works.
  - On 24.04 there is no portal backend. Eileen offers to add GNOME custom keyboard shortcuts that run `eileen ctl mute` and `eileen ctl dictate --toggle`, so dictation there is toggle, not hold [inferred].
  - Reading the keyboard directly (evdev) needs the `input` group, which Eileen never asks for.
- **The HUD.** GNOME's compositor implements no wlr-layer-shell ([fuzzel's GNOME fallback discussion](https://zff.dev/dnkl/fuzzel/pulls/758)), so a client cannot place an always-on-top, click-through overlay on GNOME. The options, in order:
  1. A small GNOME Shell extension that draws the HUD from `HudState` over D-Bus. It is JavaScript, so it needs the owner's decision.
  2. wlr-layer-shell on KDE Plasma and wlroots compositors.
  3. A normal window plus notifications.

  L2 measures and picks.
- **Tray, notifications, secrets, media, power and lock state** are D-Bus services every Ubuntu desktop runs:
  - the tray: StatusNotifierItem, which Ubuntu's GNOME shows through its AppIndicator extension [inferred: shipped and enabled by default; verify on 26.04];
  - notifications: `org.freedesktop.Notifications`;
  - secrets: the Secret Service (GNOME Keyring);
  - media: MPRIS, which browsers and media players expose;
  - power: UPower and power-profiles-daemon (its `power-saver` profile is the battery-saver fact);
  - lock state: logind's session `LockedHint`.

## 4. Each trait on Linux

The traits are BUILD.md section 3.4's, with the signatures in `eileen-platform`. Each Linux crate mirrors a Windows one:

| Linux crate | Process | Mirrors |
|---|---|---|
| `eileen-shell-linux` | core | `eileen-shell-win` |
| `eileen-audio-linux` | core and speech | `eileen-audio-win` |
| `eileen-desktop-linux` | desktop | `eileen-desktop-win` |
| `eileen-apps-linux` | apps | `eileen-apps-win` |
| `eileen-hud-linux` | HUD | `eileen-hud-win` |

Every one of them is gated with `#![cfg(target_os = "linux")]`, so on Windows it compiles to an empty library, as the `-win` crates do on Linux.

| Trait | Linux implementation | Crates (eligible version, licence) | Phase |
|---|---|---|---|
| `SystemMemory` | `/proc/meminfo` (`MemAvailable`; `CommitLimit` minus `Committed_AS` as the commit headroom), the 30 s paging rate from `/proc/vmstat` (`pgmajfault` and `pswpin` deltas), the largest non-Eileen process by resident size from `/proc/<pid>/status` | none (std); `rustix` 1.1.5 if needed, already in the lockfile | L0 |
| `MachineIdentity` | `eileen_platform::machine::FileMachineIdentity`, already written | none | L0 |
| `MachineProbe` | See the list after this table | none (std); `rustix` 1.1.5 for `statvfs` | L0 (with WP0 phase B's `wp0/machine-contracts`) |
| `SecretStore` | Secret Service over D-Bus through keyring-core, the same front as Windows; the kernel keyring as a fallback | `keyring-core` 1.0.1 (pinned in 4.1), `zbus-secret-service-keyring-store` 1.0.1, `linux-keyutils-keyring-store` 1.0.0, all MIT OR Apache-2.0 | L1 |
| `MediaSessions` | MPRIS over D-Bus: list `org.mpris.MediaPlayer2.*`, read `PlaybackStatus`, `Metadata` and `Position`, call `PlayPause`, `Seek` and `SetPosition`, subscribe to `PropertiesChanged` and `Seeked` | `zbus` 5.19.0, MIT. `mpris-server` is MPL-2.0, which `deny.toml` does not allow, and the client side is small enough to write | L1 |
| `ShellHost`: toasts | `org.freedesktop.Notifications` | `zbus` 5.19.0 | L1 |
| `ShellHost`: `keep_awake` | logind or ScreenSaver inhibitors | `zbus` 5.19.0 | L1 |
| `ShellHost`: `session_locked`, `lock_workstation` | logind | `zbus` 5.19.0 | L1 |
| `ShellHost`: `set_run_at_sign_in` | An XDG autostart desktop file | std | L1 |
| `ShellHost`: `set_tray` | StatusNotifierItem | `ksni` 0.3.6, Unlicense (allowed) | L2 |
| `ShellHost`: `register_hotkey`, `watch_input` | The GlobalShortcuts portal (section 3) | `ashpd` 0.13.13, MIT | L2 |
| `Apps` | Index: XDG desktop entries under `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`, including the Flatpak and Snap exports. Launch: D-Bus activation or `gio launch`. Open: the OpenURI portal or `gio open` | own desktop-entry parser (the `freedesktop-desktop-entry` crate is MPL-2.0); `zbus` and `ashpd` for the portal | L1 |
| `RecentSources` | GTK's `recently-used.xbel`, KDE's `RecentDocuments`, the media history; `inotify` for watching; thumbnails from the freedesktop thumbnail cache | `quick-xml` 0.42.0 (MIT), `inotify` 0.11.5 (ISC) | L1 |
| `AppHost` | Per app, as on Windows. LibreOffice through its own scripting bridge, the file manager through D-Bus `org.freedesktop.FileManager1`, Resolve on Linux through the same Lua bridge (Resolve Studio has a Linux build) | to choose per app | L2 and later |
| `AudioEndpoints`, `SystemVolume` | PipeWire: the default source and sink. The far-end reference for AEC is the default sink's monitor, which plays the role of WASAPI loopback. Real-time priority comes through RealtimeKit (this is `register_audio_thread`). Volume and mute come from the sink. PipeWire's own echo-cancel module is the alternative to sonora, as the Communications category is on Windows | `pipewire` 0.10.1 (MIT; needs libpipewire, pkg-config and clang to build); `cpal` 0.18.2 streams through ALSA, which PipeWire serves | L2 |
| `Grounding`, `Automation`, `TextInsertion` | AT-SPI2: trees, states, `Action`, `EditableText` and `Text`, focus events; selection through the `Text` interface; insertion through `EditableText`, then the clipboard (the Wayland data device) with a paste. Password fields are refused by the `password text` role, the rule Windows applies to `IsPassword` | `atspi` 0.30.0 (Apache-2.0 OR MIT), `wl-clipboard-rs` 0.9.4 (MIT OR Apache-2.0) | L2 |
| `Input` | libei through the RemoteDesktop portal; `user_idle_ms` from GNOME's `org.gnome.Mutter.IdleMonitor` | `reis` 0.7.1 (MIT, pure Rust), `ashpd` 0.13.13 | L2 |
| `Capture` | The ScreenCast portal and a PipeWire stream | `ashpd` 0.13.13, `pipewire` 0.10.1 | L2 |
| `HudSurface` | wlr-layer-shell where the compositor has it; on GNOME, section 3's options | `smithay-client-toolkit` 0.21.1 and `wayland-client` 0.31.15 (both MIT) | L2 |

`MachineProbe` reads these sources:

- **CPU.** Logical and physical counts and the core classes from `/sys/devices/system/cpu/cpu*/topology` and `cpu_capacity`, where the kernel publishes it for hybrid parts. The feature flags (`avx2`, `avx512f`, `avx_vnni`) come from `/proc/cpuinfo`.
- **GPUs and NPUs.**
  - Adapters come from `/sys/class/drm/card*/device`, with the vendor, the driver and, for amdgpu, `mem_info_vram_total`.
  - NPUs come from `/sys/class/accel`.
  - NVIDIA's proprietary driver is read from `/proc/driver/nvidia`.
  - Under WSL the GPU is `/dev/dxg`, with no DRM card, so the adapters are listed as degraded.
- **Disk.** `statvfs` on the data folder, and `queue/rotational` of its block device.
- **OS.** `/etc/os-release` and the kernel release.
- **Power.** `/sys/class/power_supply` and power-profiles-daemon.
- **Monitors.** The DRM connectors whose status is `connected`.

Three licence notes, from the crates.io data:

- `mpris-server`, `freedesktop-desktop-entry` (MPL-2.0) and `udisks2` (LGPL-2.1) are outside `deny.toml`'s allow list. Eileen writes those small pieces itself.
- `zbus` 6.0.0 (2026-10-16), `ksni` 0.3.7 (2026-10-20) and `keyring-core` releases after 1.0.1 are inside the cooling-off window. The eligible versions above are older.
- Every crate named here is MIT, Apache-2.0, ISC or Unlicense, which `deny.toml` already allows.

## 5. Processes, IPC and security on Linux

The Windows design's safety rests on Job Objects, named-pipe caller checks and per-user folders. Linux has an equivalent for each.

- **Supervision.** `eileen-supervisor` starts every child in its own process group, with `PR_SET_PDEATHSIG` so a child dies with the supervisor. It holds a pidfd for each child, so waiting and killing never race a reused PID.
  - Containment uses cgroup v2. Each Claude session runs in a transient user scope (`systemd-run --user --scope`) or a sub-cgroup of Eileen's delegated slice. Killing the scope with `cgroup.kill` takes the grandchildren too, which is the property BUILD.md's "killing a supervised child's Job Object kills its grandchild" test asks for.
  - `memory.worker_job_limit_mb` maps to the scope's `MemoryMax`, and the scope's `memory.events` OOM count becomes `WorkerEvent::MemoryLimit`, as `JOB_OBJECT_MSG_JOB_MEMORY_LIMIT` does on Windows.
  - The guard starts outside Eileen's scope, as it starts outside the Job Object on Windows.
  - `process-wrap` 10.0.1, already pinned, has `process-group` and `process-session` features for Unix. Turning them on is a one-line amendment to the root manifest's feature union.
- **Scheduling.** There is no EcoQoS on Linux to opt out of. Audio threads ask RealtimeKit for real-time priority. Builds and the librarian run at a high nice value with idle I/O priority.
- **Pipes become Unix sockets.** The speech pipe, the dispatcher pipe and the session sockets live under `$XDG_RUNTIME_DIR/eileen/`, mode 0700.
  - Caller checks use `SO_PEERCRED`: the user id must match, then `/proc/<pid>/exe` must be the installed `eileen-shim` and `/proc/<pid>/cgroup` the job's scope. `SO_PEERPIDFD` (Linux 6.5 and later) closes the PID-reuse window.
  - These are the Linux forms of `GetNamedPipeClientProcessId` and the Job Object membership check (BUILD.md 2.12).
  - The Claude CLI's HTTP hooks still need a TCP loopback port. Its connections are mapped to a PID through the sock_diag netlink interface and `/proc/<pid>/fd` [inferred: L1 measures the cost].
- **Data directory.** XDG base directories:

  | Data | Linux location |
  |---|---|
  | Configuration | `~/.config/eileen` |
  | Data | `~/.local/share/eileen` |
  | Logs, incidents and the machine-wide `machine/` files | `~/.local/state/eileen` |
  | Caches and models | `~/.cache/eileen` |
  | Sockets | `$XDG_RUNTIME_DIR/eileen` |

  `EILEEN_DATA_DIR` redirects everything exactly as on Windows. The machine-wide files keep their rule of ignoring it (BUILD.md 2.5 and 6.1).
- **Path classes.** The classes of BUILD.md 2.12 need a Linux list.
  - `Credential`: `~/.ssh`, `~/.aws`, `~/.config/gh`, `~/.git-credentials`, `~/.netrc`, `~/.gnupg`, `~/.password-store`, `~/.local/share/keyrings`, `~/.docker/config.json`, `~/.kube`, and the vendor CLI credential files. It is kept as data, as check 14 already expects.
  - `BrowserProfile`: `~/.mozilla`, `~/.config/google-chrome`, `~/.config/chromium`, `~/.config/BraveSoftware`, `~/snap/firefox/common/.mozilla`, `~/.var/app/*`.
  - `Toolchain`: `~/.cargo`, `~/.rustup`, `/usr`, `/opt`.
  - Paths are canonicalised with `openat2` and `RESOLVE_NO_SYMLINKS` where a write must stay inside a job folder.
  - Linux is case-sensitive, except on the mounts that are not: the `/mnt/<drive>` DrvFs mounts under WSL, FAT and exFAT drives, and ext4 directories with casefolding. The `.git` spelling rules of pull request #18 therefore apply by filesystem, not by OS.
  - Under WSL, the Windows user's credential folders reach Linux as `/mnt/c/Users/<name>/.ssh` and similar, and are `Credential` too.

## 6. Testing on Linux

| Lane | What runs | Where | State |
|---|---|---|---|
| PR, neutral | fmt, cargo-deny, cargo-machete, clippy with warnings denied, nextest, doc tests, docs | `ubuntu-latest` in `ci.yml` | Running. The runner moves to 26.04 during this month's rollout, so pin `ubuntu-24.04` or run both until it settles |
| PR, `-linux` crates | The same lanes over the `-linux` crates, with their fixture tests (recorded `/proc` and `/sys` trees, D-Bus fakes) | `ubuntu-latest` | From L0 |
| Local Linux lane | The PR lanes on Ubuntu before a pull request opens, including other work packages' branches, so a Linux-only failure is found before it merges | WP10's Linux machine | Running |
| Linux desktop, nightly | The real D-Bus services in a headless session, against the `-linux` crates' integration tests: `dbus-run-session`, at-spi2-core, PipeWire and WirePlumber with a null sink, and a headless compositor (`mutter --headless` or `weston --backend=headless` [inferred: to prove on the runner]) | `ubuntu-26.04` | From L1 |
| Linux desktop scenarios | `scenarios/` entries that need a real Ubuntu desktop (portals with consent, audio devices, AT-SPI against real apps), run attended | WP10's machine and Linux fleet PCs | From L2 |

Three habits carry over from the Windows plan.

- **Fakes at the protocol level.** A test of an MPRIS, Notifications, Secret Service or logind client runs a small fake service on a private bus, so CI needs no desktop. These are the D-Bus equivalents of the fakes beside each trait.
- **One conformance suite per trait.** The same tests run against the fake, the Windows implementation and the Linux one, so the three cannot drift apart [proposed].
- **Honest numbers.** Every Linux measurement records the distribution, the desktop, the session type and the machine. A WSL2 run says so: WSLg's compositor and audio stand in for a desktop and are no measure of GNOME.

WSL2 is fine for the neutral crates and the D-Bus fakes. It is not a Linux desktop: it has no GNOME, no portals and no PipeWire by default. The desktop lanes run on a real Ubuntu desktop or in the headless session above.

## 7. Phases and acceptance

1. **L0, now, beside W0 to W2.**
   - Builds:
     - The local Linux lane over every work package's branch, with findings reported to that package before it merges.
     - `eileen-shell-linux` with `SystemMemory` and, once WP0 phase B adds the machine contracts and the trait, `MachineProbe`.
     - A short Ubuntu developer setup that needs no `sudo`.
   - Acceptance:
     - Both lanes green on `ubuntu-24.04` and `ubuntu-26.04`.
     - A `SystemMemory` sample that matches recorded `/proc` fixtures exactly and a live reading within the kernel's own figures.
     - Machine facts read in under 200 ms, BUILD.md 2.16's budget, on a real Ubuntu desktop.
2. **L1, during W2 to W4.**
   - Builds: the D-Bus backends (`MediaSessions`, toasts, `keep_awake`, the lock state, the power facts, `SecretStore`, `Apps`, `RecentSources`) and the nightly Linux desktop lane.
   - Acceptance: each passes its trait's conformance suite against a fake service in CI and against the real service on Ubuntu 26.04.
3. **L2, toward v0.2.**
   - Builds:
     - audio on PipeWire;
     - AT-SPI grounding and automation;
     - the tray and the GlobalShortcuts portal;
     - input and capture through the portals;
     - the HUD;
     - slice S1 on Linux.
   - Acceptance, S1-L on Ubuntu 26.04: the real microphone and speakers, mute and tray, one fast-path command, one barge-in, one brain answer, one Claude job, with latencies recorded (BUILD.md 6.4's S1 on Linux).
4. **L3.**
   - Builds:
     - packages: `.deb` first, then Flatpak, whose sandbox already expects the portals;
     - the Linux fleet (`eileen test --fleet` on Ubuntu PCs);
     - the Linux part of `TESTING.md`.

Whether L2 ships with v0.1 or v0.2 is the owner's call. Nothing in L0 or L1 needs any Windows work package to wait.

## 8. Proposed work package: WP10, Linux (Ubuntu first)

The text below is for the owner to accept, change or strike. If accepted, it becomes BUILD.md section 6.12 through an amendment pull request.

- **Branch** `wp10/<topic>` for each pull request.
- **Depends on** WP0 phase A for the traits. Each Linux backend also depends on the package that owns its consumer: WP2 the runtime, WP4 voice, WP5 desktop, WP6 apps, WP8 the HUD.
- **Desktop** Linux: a real Ubuntu desktop for L2 and later, never the owner's PC.
- **Wave** Beside W0 to W6, on its own machine. It holds none of the owner PC's build slots, and its pull requests are small, so the owner's review time stays the only shared cost.
- **Owns:**
  - the crates: `eileen-shell-linux`, `eileen-audio-linux`, `eileen-desktop-linux`, `eileen-apps-linux`, `eileen-hud-linux`;
  - the docs: `docs/LINUX.md` and `docs/wp/WP10-linux.md`;
  - the Linux scenarios: `scenarios/linux/`;
  - the Linux desktop lane's workflow file, by amendment with WP0, which owns `.github/`.
- **Does not own** the traits, the contracts or the neutral servers. A Linux backend that needs a contract or trait change opens an amendment, like every other package.
- **Acceptance** The phase lists of section 7.

## 9. Risks

- **Wayland's consent model.** Input and capture need the user's yes in a portal dialog, at least once, and some Windows behaviours have no Linux form. The Linux machine panel and the docs say plainly what works on which desktop.
- **The HUD on GNOME.** An honest overlay may need a GNOME Shell extension in JavaScript, a second language in the repository, which is an owner decision.
- **Contracts shaped by Windows.** `hwnd: i64` and physical virtual-screen pixels have no Wayland meaning.
  - On Linux, the window handle is an AT-SPI object reference mapped to an `i64` inside the `-linux` crate.
  - Element boxes are window-relative.
  - A contract that cannot express Linux gets an amendment, not a workaround.
- **Native build dependencies.** PipeWire's binding needs `libpipewire-0.3-dev`, `pkg-config` and `clang` at build time, in CI and on developer machines. Everything in L0 and L1 is pure Rust.
- **The runner image change.** `ubuntu-latest` changes from 24.04 to 26.04 during the current rollout, and tool versions with it. Pinning the image keeps a pull request's result from changing under it.

## 10. What the Linux lane found already

Running the PR lanes on Ubuntu 24.04 over the unmerged `wp0/base-crates` work (`backup/wp0-base-crates` at ca336be) gave 305 of 305 tests passing and two failures that the Windows jobs cannot show.

- **A Linux-only clippy error.** In `eileen-supervisor`, `qos.rs` line 106, the test `an_audio_boost_reverts_on_drop` calls `drop(boost)`. Off Windows, `AudioBoost` holds no MMCSS handle and implements no `Drop`, so `clippy::drop_non_drop` fails the build under `-D warnings`. A `cfg_attr(not(windows), expect(clippy::drop_non_drop, reason = "..."))` on that statement, or ending the scope instead of calling `drop`, fixes it.
- **A yanked pin.** `tokio` 1.54.0, BUILD.md 4.1's pin, is yanked on crates.io, so `cargo deny --locked check` fails. 1.54.1 appeared on 2026-10-21. The newest release outside the 14-day window is 1.53.2, of 2026-10-03.

`main` itself passes the whole lane on Ubuntu 24.04.
