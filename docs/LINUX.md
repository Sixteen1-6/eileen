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
  - lock state: logind's session `LockedHint`, which GNOME Shell and KDE Plasma's locker set and swaylock does not.

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

Their D-Bus clients share one more crate, `eileen-dbus-linux`, which has no Windows counterpart. It holds `DbusError` and its mappings, the session and system buses with a method timeout, name owners and their `NameOwnerChanged` signals, the portal `Request` client, and, for tests, a private bus behind its `private-bus` feature. No client writes its own.

| Trait | Linux implementation | Crates (eligible version, licence) | Phase |
|---|---|---|---|
| `SystemMemory` | `/proc/meminfo`'s `MemAvailable`. Commit headroom follows `vm.overcommit_memory`: `CommitLimit` minus `Committed_AS` under strict overcommit, else `MemAvailable` plus `SwapFree` (section 10). The paging rate is `pgmajfault` deltas at 4 KB per fault, the build guard's formula. The largest process is the one with the most resident memory (`VmRSS` in `/proc/<pid>/status`) outside Eileen's own process tree | none beyond the crate's | L0: built as `LinuxSystemMemory`; the trait implementation lands with `eileen-platform` |
| `MachineIdentity` | `eileen_platform::machine::FileMachineIdentity`, already written | none | L0 |
| `MachineProbe` | See the list after this table | `rustix` 1.1.5 for `statvfs`, already in the lockfile; `thiserror`; `zbus` 5.19.0 for the power facts over D-Bus | L0: the readers are built. L1: the power facts over D-Bus are built as `LinuxPower` in `eileen-shell-linux`, which `LinuxMachineProbe` does not use yet. The trait implementation lands with WP0 phase B's `wp0/machine-contracts` |
| `SecretStore` | Secret Service over D-Bus through keyring-core, the same front as Windows; the kernel keyring as a fallback | `keyring-core` 1.0.1 (pinned in 4.1), `zbus-secret-service-keyring-store` 1.0.1, `linux-keyutils-keyring-store` 1.0.0, all MIT OR Apache-2.0 | L1, waiting: the Secret Service store turns on zbus's default features, which Cargo would then turn on for every crate, so it waits for a store that leaves them off (BUILD.md 4.1's comment) |
| `MediaSessions` | MPRIS 2.2 over D-Bus: list `org.mpris.MediaPlayer2.*`, read `PlaybackStatus`, `Metadata` and `Position`, call `Play`, `Pause`, `PlayPause`, `Next`, `Previous`, `Seek` and `SetPosition`, subscribe to `PropertiesChanged` and `Seeked`, and follow `NameOwnerChanged` as players arrive and leave | `zbus` 5.19.0, MIT. `mpris-server` is MPL-2.0, which `deny.toml` does not allow, and the client side is small enough to write | L1: built as `MprisSessions`, with its `MprisWatch` stream, in `eileen-apps-linux`; the trait implementation lands with `eileen-platform` |
| `ShellHost`: toasts | `org.freedesktop.Notifications` | `zbus` 5.19.0 | L1: built as `Notifier`, with its `NotificationEvents` stream, in `eileen-shell-linux`; the trait implementation lands with `eileen-platform` |
| `ShellHost`: `keep_awake` | logind's `sleep:idle` block lock, plus the desktop's hold on suspend and idle: GNOME's session manager (`Inhibit` flags 12), else the Inhibit portal (flags 12, then 8 after a backend's refusal) | `zbus` 5.19.0, `tokio` 1.53.2 | L1: built as `LinuxPowerRequest` in `eileen-shell-linux`; the trait implementation lands with `eileen-platform` |
| `ShellHost`: `session_locked`, `lock_workstation` | logind's `session/auto` object: `LockedHint` and `Lock()` | `zbus` 5.19.0 | L1: built as `SessionLock` in `eileen-shell-linux`; the trait implementation lands with `eileen-platform` |
| `ShellHost`: `set_run_at_sign_in` | `$XDG_CONFIG_HOME/autostart/<id>.desktop`, `Exec` quoted by the Desktop Entry rules | std | L1: built as `set_run_at_sign_in` in `eileen-shell-linux`; the trait implementation lands with `eileen-platform` |
| `ShellHost`: `set_tray` | StatusNotifierItem | `ksni` 0.3.6, Unlicense (allowed) | L2 |
| `ShellHost`: `register_hotkey`, `watch_input` | The GlobalShortcuts portal (section 3) | `ashpd` 0.13.13, MIT | L2 |
| `Apps` | Index: XDG desktop entries under `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`, including the Flatpak and Snap exports. Launch: D-Bus activation (`org.freedesktop.Application`; built), or `gio launch` through `eileen_supervisor::user_launch`. Open: the OpenURI portal (`OpenURI` and `OpenFile`; built) or `gio open` | own desktop-entry parser (the `freedesktop-desktop-entry` crate is MPL-2.0); `zbus` 5.19.0 for the portal and D-Bus activation; the portal's client is `eileen-dbus-linux`'s `Request`, so `ashpd` is not needed for `OpenURI` | L1: built in `eileen-apps-linux` as `index` and `Opener`, with `entry_for`. Launching an app that is not D-Bus activatable, and opening a file with a packaged app, wait for `eileen_supervisor::user_launch`. The trait implementation lands with `eileen-platform` |
| `RecentSources` | `recently-used.xbel`, which GTK and KDE's KIO keep; the media history; `inotify` for watching; thumbnails from the freedesktop thumbnail cache | `quick-xml` 0.42.0 (MIT), `inotify` 0.11.5 (ISC) | L1: `recent` is built as `RecentlyUsed` in `eileen-apps-linux`. `watch` and `thumbnail` are not built yet, and the media history stays with the recent-work index (BUILD.md 2.7). The trait implementation lands with `eileen-platform` |
| `AppHost` | Per app, as on Windows. LibreOffice through its own scripting bridge, the file manager through D-Bus `org.freedesktop.FileManager1`, Resolve on Linux through the same Lua bridge (Resolve Studio has a Linux build) | to choose per app | L2 and later |
| `AudioEndpoints`, `SystemVolume` | PipeWire: the default source and sink. The far-end reference for AEC is the default sink's monitor, which plays the role of WASAPI loopback. Real-time priority comes through RealtimeKit (this is `register_audio_thread`). Volume and mute come from the sink. PipeWire's own echo-cancel module is the alternative to sonora, as the Communications category is on Windows | `pipewire` 0.10.1 (MIT; needs libpipewire, pkg-config and clang to build); `cpal` 0.18.2 streams through ALSA, which PipeWire serves | L2 |
| `Grounding`, `Automation`, `TextInsertion` | AT-SPI2: trees, states, `Action`, `EditableText` and `Text`, focus events; selection through the `Text` interface; insertion through `EditableText`, then the clipboard (the Wayland data device) with a paste. Password fields are refused by the `password text` role, the rule Windows applies to `IsPassword` | `atspi` 0.30.0 (Apache-2.0 OR MIT), `wl-clipboard-rs` 0.9.4 (MIT OR Apache-2.0) | L2 |
| `Input` | libei through the RemoteDesktop portal; `user_idle_ms` from GNOME's `org.gnome.Mutter.IdleMonitor` | `reis` 0.7.1 (MIT, pure Rust), `ashpd` 0.13.13 | L2 |
| `Capture` | The ScreenCast portal and a PipeWire stream | `ashpd` 0.13.13, `pipewire` 0.10.1 | L2 |
| `HudSurface` | wlr-layer-shell where the compositor has it; on GNOME, section 3's options | `smithay-client-toolkit` 0.21.1 and `wayland-client` 0.31.15 (both MIT) | L2 |

The L1 rows that say built are tested in CI's `logic-linux` job: the D-Bus clients against fake services on `eileen-dbus-linux`'s private bus, the rest over fixture files and folders. Checks against the real services wait for the nightly Linux desktop lane (section 6).

`MachineProbe` reads these sources, all without elevation. The readers are in `crates/eileen-shell-linux`.

- **CPU.**
  - The name, the vendor and the features come from `/proc/cpuinfo`: `avx2`, `avx512f`, `avx_vnni` or `avx512_vnni`, and Arm's `asimd` and `asimddp`. An Arm kernel whose `cpuinfo` names no processor gives the chip's name instead, from `/sys/devices/soc0` or the device tree's `compatible` list.
  - Logical processors are the `online` list. Physical cores are the distinct `topology/core_cpus_list` sets, because `core_id` restarts in each Arm cluster.
  - Core classes come from Intel's hybrid PMU lists (`cpu_core`, `cpu_atom`), which give the same two classes Windows reports. Otherwise they come from distinct `cpu_capacity` values (Arm big.LITTLE), and otherwise there is one class.
- **GPUs and NPUs.**
  - Adapters come from `/sys/class/drm/card*/device/uevent`: the driver, the PCI ids and the slot, with names from `pci.ids`. Display-only and virtual adapters are left out.
  - NVIDIA GPUs are discrete, and NVIDIA's own driver also lists them under `/proc/driver/nvidia/gpus`, which finds them when `nvidia-drm` has made no DRM card. Intel's are integrated at slot `00:02.0`.
  - An amdgpu GPU is integrated only beside an AMD processor. When the processor's name mentions Radeon, it is the AMD GPU with the least VRAM, whatever the carve-out (a Strix Halo's can be 96 GB). Otherwise it is one whose VRAM is a carve-out of 2 GB or less with more GTT than VRAM, as on desktop Ryzen 7000 parts. VRAM and GTT are the dedicated and shared memory; an integrated GPU's carve-out counts as dedicated, as Windows reports it.
  - An Arm chip's own GPU (panthor, panfrost, msm and others) is integrated and has no PCI id.
  - NVIDIA's own driver gives the model and the driver version from `/proc/driver/nvidia`. The GPU's UUID in the same file is never kept.
  - NPUs come from `/sys/class/accel`.
  - Under WSL the GPU is `/dev/dxg`, with no DRM card, so the adapters are listed as degraded.
- **Disk.**
  - `statvfs` on the data folder gives the space, or on its nearest existing parent before setup creates it.
  - The block device's `queue/rotational` gives the media. The device is found by its number in `/sys/block`, partitions, LVM and LUKS included, or through `/proc/self/mountinfo` for btrfs, whose device numbers match no device.
  - WSL's and virtual machines' disks claim to rotate, or claim to be solid-state for a network volume. They are reported as `Unknown` and degraded, never as hard disks. A guest is known by WSL's kernel release, the CPU's `hypervisor` flag or Xen's `/sys/hypervisor`, and otherwise (an Arm guest) by its disks' names and models.
- **OS.** `/etc/os-release`, or `/usr/lib/os-release`, and the kernel release: `ubuntu 24.04, kernel 6.8.0-45-generic`. The build changes with each kernel update, as Windows' update revision does, so benchmarks that depend on it run again.
- **Power.**
  - The battery comes from `/sys/class/power_supply`. A battery whose scope is `Device`, such as a mouse's, is not the PC's.
  - Battery saver is the ACPI platform profile `low-power` or power-profiles-daemon's saved `power-saver` profile.
  - `UPower` gives the battery and the power source over D-Bus (`LinuxPower` in `crates/eileen-shell-linux`): `OnBattery`, and its devices of `Type` 2 with `PowerSupply` and `IsPresent`. A desktop on a discharging UPS reports `on_battery` without `has_battery`.
  - L1 reads the active profile over D-Bus (`LinuxPower` in `crates/eileen-shell-linux`): `ActiveProfile` under `org.freedesktop.UPower.PowerProfiles`, else `net.hadess.PowerProfiles`, which also sees a program's hold.
- **Monitors.** The DRM connectors whose status is `connected` and that are not `disabled`.
- **Load.** `MachineProbe::load`'s CPU busy share comes from the `cpu` line of `/proc/stat`: busy ticks over all ticks since the previous call, as `GetSystemTimes` differences give it on Windows. The power state comes with it.

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
   - Status on 2026-10-21: `crates/eileen-shell-linux` holds the machine facts' readers and the memory sampler.
     - The readers are tested over six fixture machines: one recorded under WSL2, plus synthetic trees for the owner's laptop shape, AMD on btrfs, NVIDIA on LVM, an Arm board and a QEMU virtual machine. Their types mirror BUILD.md 3.3.17 until the contracts land.
     - The sampler, `LinuxSystemMemory`, fills the contracts' `MemoryPressure`, and its live sample matches the kernel's figures.
     - The no-sudo developer setup is section 11.
   - Acceptance:
     - Both lanes green on `ubuntu-24.04` and `ubuntu-26.04`.
     - A `SystemMemory` sample that matches recorded `/proc` fixtures exactly and a live reading within the kernel's own figures.
     - Machine facts read in under 200 ms, BUILD.md 2.16's budget, on a real Ubuntu desktop.
2. **L1, during W2 to W4.**
   - Builds: the D-Bus backends (`MediaSessions`, toasts, `keep_awake`, the lock state, the power facts, `SecretStore`, `Apps`, `RecentSources`) and the nightly Linux desktop lane.
   - Acceptance: each passes its trait's conformance suite against a fake service in CI and against the real service on Ubuntu 26.04.
   - Status on 2026-10-21: `Apps::index` is built in `crates/eileen-apps-linux`. It reads the desktop entries with Eileen's own parser and follows the specification's precedence and masking (`Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn`, `TryExec`). Names follow `LANGUAGE` and the locale, as GNOME's menu does. Flatpak and Snap apps count as packaged. It is tested over a fixture session. Launch waits for the supervisor's Linux process model.
   - Status on 2026-10-22: pull requests #35 to #53 merged the other L1 backends into `main` that day, with `eileen-dbus-linux`, the shared crate their D-Bus clients use; [docs/wp/WP10-linux.md](wp/WP10-linux.md) keeps the progress. Every L1 backend but `SecretStore` is built, `Apps` and `RecentSources` in part.
     - `crates/eileen-shell-linux` holds the autostart entry (`set_run_at_sign_in`), the lock state (`SessionLock`), keeping the session awake (`LinuxPowerRequest`) and toasts (`Notifier`, with its `NotificationEvents` stream).
     - The power facts are built in `crates/eileen-shell-linux` (`LinuxPower`): one reading from `UPower` and power-profiles-daemon that names a daemon that does not answer as degraded, and a stream of changes the caller drives (`PowerChanges`). After its first reading the stream never has the bus start a daemon, so a daemon the user stops stays stopped. They are tested against fake daemons on a private bus; `LinuxMachineProbe` does not use them yet.
     - `crates/eileen-apps-linux` holds `Opener`, which opens addresses, URIs and files through the `OpenURI` portal and launches D-Bus-activatable apps, with `entry_for` to find an app's entry. It also holds the recent files from `recently-used.xbel` (`RecentlyUsed`) and the MPRIS client (`MprisSessions`, with its `MprisWatch` stream). Launching an app that is not D-Bus activatable still waits for the supervisor's Linux process model, and `RecentSources::watch` and `thumbnail` are not built yet.
     - The other D-Bus clients are tested against fake services on `eileen-dbus-linux`'s private bus too, the autostart entry in folders of the tests' own and the recent files over fixture files, all in CI's `logic-linux` job.
     - None is wired into a trait implementation yet, because `eileen-platform` is not on `main`; until it is, the crates mirror the traits' types. No conformance suite exists yet, and the checks against the real services wait for the nightly Linux desktop lane, which is not built yet.
     - `SecretStore` waits (section 4).
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

Pull request #31 added the text below to BUILD.md as section 6.12 on 2026-10-22, with one more crate, `eileen-dbus-linux`, which holds the D-Bus helpers the other Linux crates share; the owner can change or strike it there. [docs/wp/WP10-linux.md](wp/WP10-linux.md) keeps the progress.

- **Branch** `wp10/<topic>` for each pull request.
- **Depends on** WP0 phase A for the traits. Each Linux backend also depends on the package that owns its consumer: WP2 the runtime, WP4 voice, WP5 desktop, WP6 apps, WP8 the HUD.
- **Desktop** Linux: a real Ubuntu desktop for L2 and later, never the owner's PC.
- **Wave** Beside W0 to W6, on its own machine. It holds none of the owner PC's build slots, and its pull requests are small, so the owner's review time stays the only shared cost.
- **Owns:**
  - the crates: `eileen-shell-linux`, `eileen-audio-linux`, `eileen-desktop-linux`, `eileen-apps-linux`, `eileen-hud-linux`, and `eileen-dbus-linux`, which they share;
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
- **Autostart paths.** A program path with `%` or a backslash cannot go into an autostart entry, because systemd's autostart generator, which starts the entries of KDE Plasma's sessions, reads it differently from the specification. `set_run_at_sign_in` refuses such a path when it writes the entry.
- **Keeping the session awake.**
  - Before systemd 257, logind's block lock does not stop the user's own `Suspend()`, so on Ubuntu 24.04 the desktop's hold must cover suspend too.
  - xdg-desktop-portal's Inhibit checks no lockdown setting and never answers `NotAllowed`. A permission store that masks every flag leaves the request unanswered, so it shows only once the deadline has passed.
- **Native build dependencies.** PipeWire's binding needs `libpipewire-0.3-dev`, `pkg-config` and `clang` at build time, in CI and on developer machines. Everything in L0 and L1 is pure Rust.
- **The runner image change.** `ubuntu-latest` changes from 24.04 to 26.04 during the current rollout, and tool versions with it. Pinning the image keeps a pull request's result from changing under it.

## 10. What the Linux lane found already

Running the PR lanes on Ubuntu 24.04 over the unmerged `wp0/base-crates` work (`backup/wp0-base-crates` at ca336be) gave 305 of 305 tests passing and two failures that the Windows jobs cannot show. Reading the build guard's Linux memory code then showed a third problem, which no test exercises yet.

- **A Linux-only clippy error.** In `eileen-supervisor`, `qos.rs` line 106, the test `an_audio_boost_reverts_on_drop` calls `drop(boost)`. Off Windows, `AudioBoost` holds no MMCSS handle and implements no `Drop`, so `clippy::drop_non_drop` fails the build under `-D warnings`. A `cfg_attr(not(windows), expect(clippy::drop_non_drop, reason = "..."))` on that statement, or ending the scope instead of calling `drop`, fixes it.
- **A yanked pin.** `tokio` 1.54.0, BUILD.md 4.1's pin, is yanked on crates.io, so `cargo deny --locked check` fails. 1.54.1 appeared on 2026-10-21. The newest release outside the 14-day window is 1.53.2, of 2026-10-03. Pull request #27 moved BUILD.md 4.1's pin and the root manifest to 1.53.2; `backup/wp0-base-crates` still locks 1.54.0.
- **Commit headroom that Linux does not enforce.** On Linux the guard reads commit headroom as `CommitLimit` minus `Committed_AS`. The kernel enforces that limit only under strict overcommit (`vm.overcommit_memory = 2`). In the default mode, 0, `Committed_AS` can pass `CommitLimit` on a busy desktop, because browsers and runtimes reserve far more than they touch [inferred: the measurement below shows the mechanism, not how often a real desktop reaches it]. The headroom then reads 0 while gigabytes are free, and slot 0's rule refuses every build.
  - Measured on 2026-10-21, WSL2 with 16 GB and default overcommit: a 12 GB mapping that is never touched left `MemAvailable` at 13,957 MB but took the headroom from 9,691 MB to 0. Slot 0's rule, at the default 2,048 MB floor plus a two-job build's 2,048 MB estimate, went from admitting to refusing.
  - The fix that matches the kernel's policy is the one `LinuxSystemMemory` uses: under strict overcommit, keep `CommitLimit` minus `Committed_AS`; otherwise use `MemAvailable` plus `SwapFree`, the memory that can still be backed before the OOM killer acts. The guard and the runtime write the same machine-wide sample files, so both should use the same rule.

`main` itself passes the whole lane on Ubuntu 24.04.

## 11. Developer setup on Ubuntu, without sudo

This is the setup the local Linux lane runs on: Ubuntu 24.04 under WSL2 on Windows 11, with every tool under the home folder and no `sudo`. It runs every check of CI's `hygiene` and `logic-linux` jobs. A native Ubuntu install or a virtual machine works the same way. Until `cargo xtask setup` and `cargo xtask ci` exist (WP0 phase B), these steps stand in for them.

1. **Ubuntu.** On Windows 11, `wsl --install -d Ubuntu-24.04` in PowerShell (an administrator one the first time) gives a Linux shell in minutes. WSL2 runs the neutral crates and every PR lane, but it is not a Linux desktop (section 6). The desktop work of L1 and L2 needs an Ubuntu 26.04 desktop: a virtual machine (Hyper-V, VirtualBox, QEMU) or a spare PC.
2. **Rust, as the repository pins it.** rustup installs into `~/.rustup` and `~/.cargo`. Then, inside the clone, `rustup toolchain install` reads `rust-toolchain.toml` (1.99.0 with rustfmt and clippy), as CI does:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none
   . "$HOME/.cargo/env"            # once per shell, or add it to ~/.bashrc
   git clone https://github.com/Sixteen1-6/eileen.git && cd eileen
   rustup toolchain install
   rustup default 1.99.0           # so cargo also runs outside the clone
   ```

3. **The CI tools, prebuilt, at CI's versions.** BUILD.md 4b's rule is never to `cargo install` from source:

   ```sh
   curl -sSfL https://github.com/cargo-bins/cargo-binstall/releases/download/v1.26.0/cargo-binstall-x86_64-unknown-linux-musl.tgz \
     | tar -xz -C "$HOME/.cargo/bin"
   cargo binstall --no-confirm cargo-nextest@0.9.148 cargo-deny@0.20.2 cargo-machete@0.9.2
   ```

4. **The lanes.** These are CI's `hygiene` and `logic-linux` steps, in order:

   ```sh
   cargo fmt --all --check
   cargo deny --locked check
   cargo machete
   cargo clippy --workspace --all-targets --locked -- -D warnings
   cargo nextest run --workspace --locked --profile ci
   cargo test --workspace --doc --locked
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
   ```

5. **Optional: Windows clippy from Linux.** `rustup target add x86_64-pc-windows-msvc` installs the Windows standard library. After that, `cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings` finds most Windows-only compile errors before a push, as long as no dependency compiles C. Test binaries cannot link without MSVC's `link.exe`, so `windows-test` still runs only in CI.

Later phases need system packages, which do need `sudo`: `pkg-config`, `clang` and `libpipewire-0.3-dev` for PipeWire's bindings (L2), and `weston` for the headless desktop lane (L1).
