# Eileen on Linux: the plan, Ubuntu first

A proposal, written 2026-10-21 after the owner asked that Linux be supported in the plan. It is for the owner, who decides what it changes in [BUILD.md](BUILD.md), and for whoever builds the Linux backends. It changes nothing in the Windows design. Linux implements the same platform traits of BUILD.md section 3.4, in `-linux` crates that mirror the `-win` crates, and the neutral crates stay as they are.

Sources are linked where they are used. Crate versions were read from crates.io on 2026-10-21; "eligible" means the newest release at least 14 days old that day (BUILD.md section 4.1's cooling-off rule). Statements marked [inferred] are reasoning that a later measurement must confirm.

## 1. Summary

- **What already holds.** Every platform-neutral crate builds and passes its tests on Linux, which CI's `logic-linux` job checks on every pull request (BUILD.md section 4.6). On Ubuntu 24.04 the full Linux lane passed on `main` at daf8e13 (fmt, cargo-deny, cargo-machete, clippy with warnings denied, 165 of 165 tests, doc tests, docs), and `logic-linux` passed 948 of 948 tests on `main` at 5a4b11b (CI run 40238334151) and 1057 of 1057, with the 4 desktop-lane session tests skipped, at a84c198 (CI run 40414356056).
- **What Linux needs.** Implementations of the rest of the section 3.4 traits (`main` serves `ShellHost`, `SystemMemory`, `MediaSessions`, `Apps`, `RecentSources` and `AudioEndpoints`, the last three in part, and `MachineIdentity` through `eileen_platform::machine::FileMachineIdentity`; section 4), the rest of the Linux process model behind `eileen-supervisor` (`main` has each child's session, process group and parent-death signal; the cgroup scope and `user_launch` are still to come, section 5), Linux paths in the data directory and the path classes, and test lanes that exercise real Linux desktop services, of which the nightly Linux desktop lane runs since #83 (section 6).
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
  - On 26.04, the mute key and the dictation key come through the GlobalShortcuts portal. On GNOME its `Activated` and `Deactivated` signals are not a key's state. `Activated` comes again with each auto-repeat while the key is held, since mutter runs the shortcut again and xdg-desktop-portal-gnome keeps no pressed state. `Deactivated` comes only when the key is let go while its chord's modifiers are still down, since mutter looks a release up with the modifiers it carries. So a dictation key bound to a chord can miss its release, and hold-to-talk cannot rely on it (mutter's `src/core/keybindings.c` and xdg-desktop-portal-gnome's `globalshortcuts.c`, read on 2026-10-25 for #93). Eileen counts a press only when the shortcut was up (#107). A press names no device: the portal drops it, and input injected through libei fires the shortcut too, so on Linux the approve, deny and unmute hotkeys cannot be checked for a keyboard device as Raw Input checks them on Windows. Until the owner decides how BUILD.md sections 2.5 and 2.9 apply on Linux, a Linux composition root must not map a press, or a tray choice, which any program on the session bus can send, to `Approve`, `Deny` or an unmuting `SetMute` or `ToggleMute` (`LinuxShellEvent`'s doc).
  - The portal needs the app's ID. Eileen's client registers it with the host registry (`org.freedesktop.host.portal.Registry`, xdg-desktop-portal 1.19.4 and later) before it opens a session, and the registry takes only the basename of an installed desktop file. xdg-desktop-portal 1.21.0 and later refuse a session to an app without an ID, and GNOME Settings discards a bind from one, so Eileen's desktop file must be installed (L3's packages install it). GNOME keeps the trigger the user chose for each shortcut ID, and asks again only for shortcuts it has not stored.
  - GNOME answers a bind dialog the user dismissed with 2, not 1. Closing the bind's request does not dismiss GNOME's dialog: an accept that comes later is stored but binds nothing until the next bind of those shortcuts, which GNOME answers without a dialog.
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

Every one of them is gated with `#![cfg(target_os = "linux")]`, so on Windows it compiles to an empty library, as the `-win` crates do on Linux. `eileen-shell-linux` and `eileen-apps-linux` are on `main`, and so, since BUILD.md section 3.1 gained their rows (#78 and #80), are `eileen-desktop-linux`, with its AT-SPI2 client and no trait served yet (#87), and `eileen-audio-linux`, with `register_audio_thread` (#90). Neither is wired into its process yet: `eileen-desktop`'s manifest has no target table, and `eileen-audio`'s and `eileen-speech`'s have only Windows ones. `eileen-hud-linux` is planned.

Their D-Bus clients share one more crate, `eileen-dbus-linux`, which has no Windows counterpart. It holds `DbusError`, its mappings and its `From` into `PlatformError`, the session and system buses with a method timeout, name owners and their `NameOwnerChanged` signals, the signal-stream helpers `subscribe` and `drain`, the portal `Request` client, whose `answer` gives the portal's results, and `close_request`, which closes a request and waits for the answer, the `Bridge` below, and, for tests, a private bus and a silent one behind its `private-bus` feature. No client writes its own.

The traits are synchronous and the clients are async, so the `Bridge` (#62) runs them: one named thread, `eileen-dbus`, with a current-thread tokio runtime. A trait call posts a job there and waits at most its deadline; past it, the call answers `DeadlineExceeded` and the job is cancelled. A job reaches the buses through `Buses`, which keeps the shared session and system connections and connects again once one closes, or opens a session connection of the job's own. Jobs answer plain data, never a connection, proxy or stream, and leave the runtime's two blocking threads to zbus, which connects a Unix socket on them, so a job that touches files does so on a thread of its own. A call made on the bridge's own thread is refused at once.

| Trait | Linux implementation | Crates (eligible version, licence) | Phase |
|---|---|---|---|
| `SystemMemory` | `/proc/meminfo`'s `MemAvailable`. Commit headroom follows `vm.overcommit_memory`: `CommitLimit` minus `Committed_AS` under strict overcommit, else `MemAvailable` plus `SwapFree` (section 10). The paging rate is `pgmajfault` deltas at 4 KB per fault, the build guard's formula. The largest process is the one with the most resident memory (`VmRSS` in `/proc/<pid>/status`) outside Eileen's own process tree | none beyond the crate's | L0: built as `LinuxSystemMemory`, which serves the trait (#64) on the clock it is given |
| `MachineIdentity` | `eileen_platform::machine::FileMachineIdentity`, which serves the trait from `/etc/machine-id`, then `/var/lib/dbus/machine-id` (#73) | none | L0: built |
| `MachineProbe` | See the list after this table | `rustix` 1.1.5 for `statvfs`, already in the lockfile; `thiserror`; `zbus` 5.19.0 for the power facts over D-Bus | L0: the readers are built. L1: the power facts over D-Bus are built as `LinuxPower` in `eileen-shell-linux`, which `LinuxMachineProbe` does not use yet. The trait implementation lands with WP0 phase B's `wp0/machine-contracts` |
| `SecretStore` | Secret Service over D-Bus through keyring-core, the same front as Windows; the kernel keyring as a fallback | `keyring-core` 1.0.1 (pinned in 4.1), `zbus-secret-service-keyring-store` 1.0.1, `linux-keyutils-keyring-store` 1.0.0, all MIT OR Apache-2.0 | L1, waiting: the Secret Service store turns on zbus's default features, which Cargo would then turn on for every crate, so it waits for a store that leaves them off (BUILD.md 4.1's comment). Planned: a proposal for the owner, a Secret Service client of Eileen's own on `zbus` with its default features off. Until one lands, nothing serves the trait on Linux |
| `MediaSessions` | MPRIS 2.2 over D-Bus: list `org.mpris.MediaPlayer2.*`, read `PlaybackStatus`, `Metadata` and `Position`, call `Play`, `Pause`, `PlayPause`, `Next`, `Previous`, `Seek` and `SetPosition`, subscribe to `PropertiesChanged` and `Seeked`, and follow `NameOwnerChanged` as players arrive and leave | `zbus` 5.19.0, MIT. `mpris-server` is MPL-2.0, which `deny.toml` does not allow, and the client side is small enough to write | L1: built as `MprisSessions`, with its `MprisWatch` stream, in `eileen-apps-linux`, and `LinuxMedia` serves the trait (#65). The listing reads every player at once, so players that never answer cost it one method timeout together (#68) |
| `ShellHost`: toasts | `org.freedesktop.Notifications` | `zbus` 5.19.0 | L1: built as `Notifier`, with its `NotificationEvents` stream, in `eileen-shell-linux`; `LinuxShell` serves the trait (#64) |
| `ShellHost`: `keep_awake` | logind's `sleep:idle` block lock, plus the desktop's hold on suspend and idle: GNOME's session manager (`Inhibit` flags 12), else the Inhibit portal (flags 12, then 8 after a backend's refusal) | `zbus` 5.19.0, `tokio` 1.53.3 | L1: built as `LinuxPowerRequest` in `eileen-shell-linux`; `LinuxShell` serves the trait (#64) from a holder task on the bridge, whose desktop hold is on a session connection of its own, so the hold ends when that connection closes even if `Uninhibit` fails. The holder connects both buses at once within half the call's deadline, counted from the call's start; a bus that hangs is a late holder, not an absent one, and the portal's answers get half of what is left (#85) |
| `ShellHost`: `session_locked`, `lock_workstation` | logind's `session/auto` object: `LockedHint` and `Lock()` | `zbus` 5.19.0 | L1: built as `SessionLock` in `eileen-shell-linux`; `LinuxShell` serves the trait (#64) |
| `ShellHost`: `set_run_at_sign_in` | `$XDG_CONFIG_HOME/autostart/<id>.desktop`, `Exec` quoted by the Desktop Entry rules | std | L1: built as `set_run_at_sign_in` in `eileen-shell-linux`; `LinuxShell` serves the trait (#64), writing on the caller's thread with no bound until the method takes the caller's `Deadline` (below) |
| `ShellHost`: `set_tray` | StatusNotifierItem: Eileen serves `org.kde.StatusNotifierItem` and `com.canonical.dbusmenu` with `#[zbus::interface]` and registers with `org.kde.StatusNotifierWatcher` | `zbus` 5.19.0, `tokio` 1.53.3. `ksni` is not pinned in BUILD.md 4.1, and the item and its menu are two small interfaces (below) | L2: built as the `tray` module and `LinuxShell::set_tray` in `eileen-shell-linux` (#110). A holder task on the bridge's thread keeps the item at `/StatusNotifierItem` and its menu at `/MenuBar` on a session connection of its own, and registers again whenever the watcher's name gets a new owner. `set_tray` answers `Ok` while a watcher has the item, and `Unavailable { what: "tray" }` while none does, as on GNOME without its `AppIndicator` extension; the item stays on the bus and registers once a watcher comes. A chosen menu item reaches `LinuxShell::with_events` as `LinuxShellEvent::TrayChoice`, which may not come from a person (section 3). The icons are Adwaita's names from `TraySettings` until Eileen ships its own (L3) |
| `ShellHost`: `register_hotkey`, `watch_input` | The GlobalShortcuts portal (section 3), through Eileen's own client, `ShortcutsSession` in `eileen-shell-linux`, on `eileen-dbus-linux`'s `Request` | `zbus` 5.19.0; `ashpd` is not used (below) | L2: built. The client is #93's, with the triggers `trigger_of_chord` and `trigger_of_key`; a holder behind `LinuxShell` (#107) binds the hotkeys and the dictation key in one session on a connection of its own, `SETTLE` (100 ms) after the set's last change, binds a changed set again in a new session, and waits `CONSENT_WAIT` (300 s) for the desktop's dialog. Presses and the bind's answer reach the sink given with `LinuxShell::with_events` (`LinuxShellEvent::Hotkey`, `ShortcutsBound`, `ShortcutsRefused`), and the dictation key's press and release reach each `watch_input` sink. `register_hotkey` never fails because another app owns the chord: the desktop resolves that in its dialog. Without the portal's GlobalShortcuts, as on Ubuntu 24.04, both answer `Unavailable` (`global hotkey`, `input watch`); section 3's fallback there, a portal that restarts, and the user's later changes in the desktop's settings are follow-ups |
| `Apps` | Index: XDG desktop entries under `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`, including the Flatpak and Snap exports. Launch: D-Bus activation (`org.freedesktop.Application`; built), or `gio launch` through `eileen_supervisor::user_launch`. Open: the OpenURI portal (`OpenURI` and `OpenFile`; built) or `gio open` | own desktop-entry parser (the `freedesktop-desktop-entry` crate is MPL-2.0); `zbus` 5.19.0 for the portal and D-Bus activation; the portal's client is `eileen-dbus-linux`'s `Request`, so `ashpd` is not needed for `OpenURI` | L1: built in `eileen-apps-linux` as `index` and `Opener`, with `entry_for`, and `LinuxApps` serves the trait (#66). The index opens each entry without waiting on a FIFO, skips what is not a file or is over 1 MiB (#67), and walks each folder once, at most 1024 per data folder (#69). Launching an app that is not D-Bus activatable, and opening a file with a packaged app, answer `Unavailable` until `eileen_supervisor::user_launch`, which is WP0's phase B and not on `main` (the Linux process model it starts children with is, section 5); `main_window` answers `Unavailable` until L2 |
| `RecentSources` | `recently-used.xbel`, which GTK and KDE's KIO keep; the media history; `inotify` for watching; thumbnails from the freedesktop thumbnail cache | `quick-xml` 0.42.0 (MIT), `inotify` 0.11.5 (ISC) | L1: `recent` is built as `RecentlyUsed` in `eileen-apps-linux`, which refuses a file over 16 MiB whole (#70), and `LinuxRecent` serves the trait (#66), with the app's name sanitized. `watch` and `thumbnail` answer `Unavailable` until L2, and the media history stays with the recent-work index (BUILD.md 2.7) |
| `AppHost` | Per app, as on Windows. LibreOffice through its own scripting bridge, the file manager through D-Bus `org.freedesktop.FileManager1`, Resolve on Linux through the same Lua bridge (Resolve Studio has a Linux build) | to choose per app | L2 and later |
| `AudioEndpoints`, `SystemVolume` | PipeWire: the default source and sink. The far-end reference for AEC is the default sink's monitor, which plays the role of WASAPI loopback. Real-time priority comes through RealtimeKit (this is `register_audio_thread`). RealtimeKit grants it only to a process whose hard `RLIMIT_RTTIME` is at most its `RTTimeUSecMax` property (200000 us by default), and Linux's default is unlimited, so the process lowers its hard limit, and its soft one to at most the hard, with rustix's `setrlimit` before the first `MakeThreadRealtime`. The thread is named by its kernel id (rustix's `gettid`), and the ticket reverts it with `MakeThreadHighPriority` to the nice level it had before, floored at `MinNiceLevel`, since neither std nor rustix has a safe `sched_setscheduler`. On rtkit 0.14 (Ubuntu 26.04) that call leaves rtkit's record of the thread's real-time priority; rtkit drops the record only in the garbage collection at the start of its next incoming call, and until then a system suspend makes rtkit put the thread back on real-time when the system resumes. The revert can also be refused, by rtkit's rate limit or, on 0.14, while a suspend is under way, which leaves the thread real-time. `eileen-audio-linux` takes both answers to that: the ticket follows the revert with a read of `RTTimeUSecMax`, even when the revert failed, which leaves a short window (a suspend that starts between the two calls) and does nothing for a refused revert or a thread whose nice level was negative before; and an audio thread that ends with its ticket escapes all three, since rtkit checks a thread's start time before it acts (the crate doc's Traps), which is how WP4's audio threads are to end. Volume and mute come from the sink. PipeWire's own echo-cancel module is the alternative to sonora, as the Communications category is on Windows. `SystemVolume` belongs to the desktop process, in `eileen-desktop-linux`, as on Windows it is `eileen-desktop-win`'s | `zbus` 5.19.0 and `rustix` 1.1.5 (its `thread` and `process` features) for RealtimeKit; for the endpoints, `pipewire` 0.10.1 (MIT; needs libpipewire, pkg-config and clang to build), not pinned in BUILD.md 4.1, or `cpal` 0.18.2, which streams through ALSA, which PipeWire serves, and is allowed only under Windows target tables today | L2, begun ahead of WP4: `register_audio_thread` is built as `LinuxAudioEndpoints` in `eileen-audio-linux`, over `RealtimeKit`, a client of rtkit on the system bus written on `zbus` (#90). It asks for `SCHED_RR` priority 10, capped at `MaxRealtimePriority`, falls back to nice -11, floored at `MinNiceLevel`, when rtkit refuses, and reverts a grant that came after the caller's deadline at once; a ticket's drop waits at most `REVERT_WAIT` (1 s). `set_communications_ducking` answers `Ok` to opting out and `Unavailable` to opting in, since there is no ducking of the OS's to opt into. The default endpoints answer `Unavailable`: no D-Bus surface serves PipeWire's graph, so they wait for L2's audio-server client, the owner's choice of crate. `SystemVolume` is not served yet |
| `Grounding`, `Automation`, `TextInsertion` | AT-SPI2: trees, states, `Action`, `EditableText` and `Text`, focus events; selection through the `Text` interface; insertion through `EditableText`, then the clipboard (the Wayland data device) with a paste. Password fields are refused by the `password text` role, the rule Windows applies to `IsPassword` | `zbus` 5.19.0, through Eileen's own AT-SPI2 client, `Atspi` in `eileen-desktop-linux`, each call bounded at 800 ms (`CALL_TIMEOUT`, libatspi's floor per call); the `atspi` crate is not used (below); `wl-clipboard-rs` 0.9.4 (MIT OR Apache-2.0) for the paste, planned | L2: the client is built (#87). It finds the accessibility bus through `org.a11y.Bus` (`accessibility_bus`), follows only a Unix socket's address (`eileen-dbus-linux`'s `bus_at_within`), so no address can make it start a program, answers `Unavailable` with no launcher or no bus, reads whether toolkits are asked for accessibility without ever switching it on (`toolkit_accessibility`), refuses a password field's text before any `Text` call, and walks a tree breadth first within node, depth and time budgets. No trait is served yet: `Grounding` on the client is draft #94 and `Automation` draft #112 on it; `TextInsertion` is not started |
| `Input` | libei through the RemoteDesktop portal; `user_idle_ms` from GNOME's `org.gnome.Mutter.IdleMonitor` | `reis` 0.7.1 (MIT, pure Rust); the portal on `zbus` through `eileen-dbus-linux`'s `Request` (`ashpd` is not used, below) | L2 |
| `Capture` | The ScreenCast portal and a PipeWire stream | the portal on `zbus` through `eileen-dbus-linux`'s `Request` (`ashpd` is not used, below), `pipewire` 0.10.1 | L2 |
| `HudSurface` | wlr-layer-shell where the compositor has it; on GNOME, section 3's options | `smithay-client-toolkit` 0.21.1 and `wayland-client` 0.31.15 (both MIT) | L2 |

The L1 rows that say built are tested in CI's `logic-linux` job: the D-Bus clients against fake services on `eileen-dbus-linux`'s private bus, the trait implementations on the same fakes through `Bridge::on_private_bus`, called from the test's thread as the core calls them, and the rest over fixture files and folders. The L2 pieces on `main` (the shortcuts holder, the tray, the AT-SPI2 client and RealtimeKit) are tested the same way, against fakes on the private bus. Since #83 the nightly Linux desktop lane (section 6) checks `ShellHost`'s `session_locked`, `lock_workstation`, `keep_awake` and toasts against logind and a real notification server on Ubuntu 26.04; the other clients run there only through the `-linux` crates' integration tests, still against their fakes. #91 (open) adds the power facts, notifications and the Inhibit portal.

**Clients of Eileen's own.** Where this table first named `ashpd`, `ksni` or `atspi`, WP10 writes the D-Bus client itself on `zbus` 5.19.0, as it did for MPRIS and the desktop-entry parser. BUILD.md section 4.1 pins none of the three, and the portal client they would bring already exists: `eileen-dbus-linux`'s `Request`, which the OpenURI and Inhibit portals use. A client of Eileen's own keeps the deadlines, the sender checks and the error rules of the other clients, on the zbus build the workspace has, tokio only. On `main` are the GlobalShortcuts client (#93) and its holder (#107), the tray's item and menu served with `#[zbus::interface]` (#110) and the AT-SPI2 client (#87), and, written the same way, the RealtimeKit client (#90).

**Deadlines.** None of the methods the Linux adapters serve takes a `Deadline` on `main` yet (`Apps::main_window` does, and answers `Unavailable`), so each adapter waits at most `eileen-dbus-linux`'s `DEFAULT_DEADLINE`, the 5 s method timeout plus half a second, and past it answers `DeadlineExceeded` naming the call. Two seams change that, for a caller that needs an answer sooner and for the tests that hold a call past its deadline: `LinuxShellSettings::deadline` and `LinuxMedia::new_within` take a deadline of their own, and `LinuxApps::reading_files_on` and `Opener::checking_files_on` run the file reads and checks on a runtime's blocking pool the caller gives, which caps the threads that hung reads keep. A read that another process can hold, such as a file on a FUSE or NFS home, runs on a named thread of its own within the deadline (`bounded` in `eileen-apps-linux`), and past it is left to that thread. BUILD.md section 3.4 has given the file-reading methods the caller's `Deadline` since #76, and the owner's #77, open, changes their signatures in `eileen-platform`. WP10's #116, ready and based on #77, moves `LinuxApps` and `LinuxRecent` to them, so that once both merge they read within the caller's deadline in place of `DEFAULT_DEADLINE`; `set_run_at_sign_in`, which then takes the deadline without enforcing it, is to write its entry on a helper thread within it. When the two merge is the owner's call.

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
- `zbus` 6.0.0 (2026-10-16) and `keyring-core` releases after 1.0.1 are inside the cooling-off window. The eligible versions above are older.
- Every crate named here is MIT, Apache-2.0, ISC or Unlicense, which `deny.toml` already allows.

## 5. Processes, IPC and security on Linux

The Windows design's safety rests on Job Objects, named-pipe caller checks and per-user folders. Linux has an equivalent for each.

- **Supervision.** On `main`, `eileen-supervisor` starts every child as the leader of a session of its own (`setsid`), so it has no controlling terminal and a prompt on `/dev/tty` fails at once instead of stopping it, and kills it with its whole process group (#106, for SU1, SU3 and SU6 of the review of #73).
  - Each child gets `SIGKILL` as its parent-death signal (`PR_SET_PDEATHSIG`), so a crash of Eileen kills every child. It is forked on the supervisor's own long-lived thread, `supervisor.fork`, because the parent-death signal follows the thread that forked the child, not the process. The kernel clears the signal on fork, so it never reaches grandchildren, and a set-user-ID program run as a child loses it at exec.
  - A pidfd per child says when the leader has exited. Its group is killed on drop, on `start_kill`, and when `wait` or `try_wait` sees the leader exit, always before tokio reaps it, so no reused PID is ever signalled. Nothing calls `waitpid` by PID or group. The pidfd needs Linux 5.4 or later (`pidfd_open` from 5.3, `waitid(P_PIDFD)` from 5.4), and `eileen_supervisor::spawn` on Linux needs a tokio runtime with I/O enabled: outside one it returns an error.
  - Descendants that outlive their leader in a crash, or leave its group, are not reached: they are for the cgroup scope below, which the supervisor does not have yet, and until then they run on.
  - stdout carries the protocol. On Linux an awaited `Child::wait` forwards a stderr pipe that nobody took into `child.stderr` events first, so a child that fills stderr cannot hang the wait (#115, part of SU5). A caller that reads stdout to its end before it waits, or polls `try_wait`, still has to take or forward stderr itself.
  - Containment uses cgroup v2, and is planned. Each Claude session runs in a transient user scope (`systemd-run --user --scope`) or a sub-cgroup of Eileen's delegated slice. Killing the scope with `cgroup.kill` takes the grandchildren too, which is the property BUILD.md's "killing a supervised child's Job Object kills its grandchild" test asks for.
  - `memory.worker_job_limit_mb` maps to the scope's `MemoryMax`, and the scope's `memory.events` OOM count becomes `WorkerEvent::MemoryLimit`, as `JOB_OBJECT_MSG_JOB_MEMORY_LIMIT` does on Windows.
  - The guard starts outside Eileen's scope, as it starts outside the Job Object on Windows. On Unix `spawn_detached` starts it as the leader of a new process group, and the core reaps it on a thread of its own, `supervisor.reap`, so a guard that exits leaves no zombie, and a liveness check by its PID no longer reports a dead guard as alive for the rest of the core's life (#117, for SU2). It is still a check by PID, so once the guard is reaped its PID can go to another process; a check that must be exact needs a pidfd opened while the guard runs.
  - `process-wrap` 10.0.1, already pinned, has `process-group` and `process-session` features for Unix. On Linux the supervisor uses neither: `setsid` and the parent-death signal are set in its own pre-exec setup. Its manifest turns `process-group` on, which only its code for Unix systems other than Linux uses, and BUILD.md section 3.1 lets a member add features to a dependency other than `windows`, `windows-sys` and `tokio`, so no root manifest amendment is needed.
- **Scheduling.** There is no EcoQoS on Linux to opt out of.
  - Audio threads ask RealtimeKit for real-time priority. That needs the process's hard `RLIMIT_RTTIME` lowered first (section 4), and the limit reaches past the audio thread. It is process-wide. Every child spawned after it is set inherits it, and the core starts the speech, desktop, apps and HUD processes, the guard and every Claude session (BUILD.md 2.1 rule 1). It cannot be raised again without `CAP_SYS_RESOURCE`. It counts the CPU time a thread under a real-time policy uses without a blocking system call, so an audio thread that waits on its device every period stays far below it. When a real-time thread runs past the soft limit, the process gets `SIGXCPU`, whose default action ends it, and `SIGKILL` at the hard limit; with the soft limit equal to the hard one, as `eileen-audio-linux` sets it when the soft one was unlimited, `SIGKILL` comes with no `SIGXCPU` first. In the core, either costs everything (BUILD.md 2.1). Which process sets the bound first is WP4's wiring.
  - A `ClaudeSession` child runs `CLAUDE_SESSION_NICER_BY` (10) nice steps below the thread that forks it, at most at 19, set between fork and exec so its builds inherit it; `Sidecar` and `Tool` children keep that thread's value (#115, SU4). The value is raised from the forking thread's rather than set, so a core already running nicer never asks for a lower value, which needs privilege. A nice value orders tasks only within one CPU cgroup, or, where the kernel's autogroup applies (on by default on Ubuntu, and only for tasks in the root CPU cgroup), within one session's autogroup. Each child's `setsid` starts a new autogroup, so there its builds still get an equal share against the core's; that share is the cgroup scope's CPU weight to set.
  - Builds under `cargo xtask` run 10 nice steps down: on Unix the build guard runs cargo through `nice -n 10` in a process group that a `/bin/sh` watchdog leads and kills with `SIGKILL` once the guard's pipe closes, however `cargo xtask` ends (#108, for BU5).
  - Idle I/O priority, for Claude sessions and builds alike, waits for an `ioprio_set` call (or `ionice -c 3`), which neither the supervisor nor the build guard makes yet.
- **Thread names.** Linux keeps 15 bytes of a thread name, and std cuts a longer one without a word before `pthread_setname_np`; the kernel's copy is what `/proc/<pid>/task/<tid>/comm`, `top -H`, gdb and core dumps show. On Linux `eileen_rt::runtime::build` names its threads `<name>-rt` and takes a runtime name of at most 12 bytes; a longer one fails a soft assertion and is cut to its first 12 bytes (#105). WP10's own names and the ones its amendments added fit: `eileen-dbus` and `eileen-dbus-blk` in the bridge, `supervisor.fork` and `supervisor.reap`, and `eileen-trace`'s `eileen-log`, `eileen-console` and `eileen-flush` (#86, #92). `eileen_stdx::thread::spawn_named` checks no length yet.
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
| PR, neutral | fmt, cargo-deny, cargo-machete, clippy with warnings denied, nextest, doc tests, docs | `ubuntu-latest` in `ci.yml` | Running. The runner moves to 26.04 during this month's rollout, so pin `ubuntu-24.04` or run both until it settles; `main`'s runs of 2026-10-25 at 5a4b11b and of 2026-10-26 at a84c198 still ran on the `ubuntu-24.04` image, version 20261012.334.1 |
| PR, `-linux` crates | The same lanes over the `-linux` crates, with their fixture tests (recorded `/proc` and `/sys` trees, D-Bus fakes) | `ubuntu-latest` | Running: `logic-linux` runs the `-linux` crates' tests with the workspace's, 948 tests on `main` at 5a4b11b and 1057 at a84c198, where the ci profile skips the 4 `linux_session` tests |
| Local Linux lane | The PR lanes on Ubuntu before a pull request opens, including other work packages' branches, so a Linux-only failure is found before it merges | WP10's Linux machine | Running |
| Linux desktop, nightly | `linux-desktop.yml`, nightly at 03:41 UTC, on demand and on pull requests that change the lane or a `-linux` crate's `tests/session/`: one `session` job within 30 minutes, which makes a logind session of the runner's user on seat0 (`.github/linux-desktop/session.sh`), with `dbus-run-session`, weston's headless backend, PipeWire and WirePlumber with a null sink, at-spi2-core, xdg-desktop-portal with its GTK backend, notification-daemon on an Xvfb display (it crashes on Wayland), GNOME Keyring's Secret Service and an MPRIS player (mpv), and logind, polkit, UPower and power-profiles-daemon on the system bus. The `-linux` crates' integration tests run inside it (the `ci` profile), then the `linux_session` tests against its services (the `linux-session` profile), which are for that session alone: on a developer's own desktop the lock test can lock the screen | `ubuntu-26.04` | Running since #83 (d446972, 2026-10-26), not a required check. Its four tests drive the shell's lock state, `Lock()`, the keep-awake inhibitor and a toast. It has run on pull requests so far (run 40404308881 on #83's last head passed); its first scheduled run is on 2026-10-27. The compositor is weston's headless backend; `mutter --headless` is not needed so far. #91 (open) adds the power facts, notifications and the Inhibit portal |
| Linux desktop scenarios | `scenarios/` entries that need a real Ubuntu desktop (portals with consent, audio devices, AT-SPI against real apps), run attended | WP10's machine and Linux fleet PCs | From L2 |

Three habits carry over from the Windows plan.

- **Fakes at the protocol level.** A test of an MPRIS, Notifications, Secret Service or logind client runs a small fake service on a private bus, so CI needs no desktop. These are the D-Bus equivalents of the fakes beside each trait.
- **One conformance suite per trait.** The same tests run against the fake, the Windows implementation and the Linux one, so the three cannot drift apart [proposed].
- **Honest numbers.** Every Linux measurement records the distribution, the desktop, the session type and the machine. A WSL2 run says so: WSLg's compositor and audio stand in for a desktop and are no measure of GNOME.

What the desktop lane has shown so far:

- weston's headless backend offers no seat (no keyboard or pointer), so tests that need input or a clipboard (a Wayland data device needs a seat) need another compositor setup, and no app window is ever active there.
- A second at-spi-bus-launcher started with the session's `XDG_RUNTIME_DIR` and `DISPLAY` takes over the live bus's socket path and, on X11, the root window's `AT_SPI_BUS`, which cuts the desktop's apps off from the live bus. A test that starts its own launcher gives it a runtime folder and a display of its own (#87).
- `eileen-audio-linux`'s tests run against a fake RealtimeKit on the private bus; no live test runs yet, since WSL2 has no RealtimeKit. A real Ubuntu desktop checks a registered thread with `chrt -p` once WP4 registers its threads.

The three Linux fuzz targets, `linux_mountinfo`, `linux_proc_status` and `desktop_entry`, are declared in `fuzz/` since #109, beside the other packages' targets, and their corpora replay in `logic-linux` through the crates' `fuzz_corpus` tests. No workflow builds a fuzz target yet: the pull-request lane's `cargo fuzz build` and the nightly fuzzing of BUILD.md section 4.6 come with WP0 phase B's jobs.

The scenario privacy check's PII scan knows the Linux and WSL user folders (`/home/<name>`, `/media/<name>`, `/run/media/<name>` and `\\wsl.localhost\<distro>\home\<name>`), so a scenario saved on Ubuntu, or read through WSL, is checked as one from Windows is (#89).

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
   - Status on 2026-10-26: WP0's base crates are on `main` (#73), so `LinuxSystemMemory` serves `SystemMemory` (#64) and `eileen_platform::machine::FileMachineIdentity` serves `MachineIdentity`. `MachineProbe` still waits for WP0 phase B's `wp0/machine-contracts`. On `main` at 5a4b11b, CI's `logic-linux` job passes 948 of 948 tests on the `ubuntu-24.04` image; no lane has run on `ubuntu-26.04` from `main` yet.
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
   - Status on 2026-10-26: `main` has the bridge from the synchronous traits to the async clients (#62) and the trait implementations over the L1 clients: `LinuxShell` for `ShellHost` (#64, with the keep-awake connect bound of #85), `LinuxMedia` for `MediaSessions` (#65), and `LinuxApps` and `LinuxRecent` for `Apps` and `RecentSources` (#66). Section 4's table says what each serves and what still answers `Unavailable`. The nightly Linux desktop lane is on `main` (#83) and checks the lock state, `Lock()`, keep-awake and toasts against the real services on Ubuntu 26.04, so L1's acceptance against the real service holds for those four so far; #91 (open) adds the power facts, notifications and the Inhibit portal. `SecretStore` waits for the owner's answer on #111, WP10's proposal of a Secret Service client of Eileen's own. Launching an app that is not D-Bus activatable waits for `eileen_supervisor::user_launch` (WP0's phase B); the Linux process model it starts children with is on `main` (#106, section 5). No conformance suite exists yet.
3. **L2, toward v0.2.**
   - Builds:
     - audio on PipeWire;
     - AT-SPI grounding and automation;
     - the tray and the GlobalShortcuts portal;
     - input and capture through the portals;
     - the HUD;
     - slice S1 on Linux.
   - Status on 2026-10-26: the shell's part of L2 is built. `register_hotkey` and `watch_input` are served through the GlobalShortcuts portal (#93's client and #107's holder), and `set_tray` as a StatusNotifierItem with a dbusmenu menu (#110); neither has been checked against GNOME 50's portal or its `AppIndicator` extension yet. `eileen-desktop-linux` and `eileen-audio-linux` have their section 3.1 rows (#78 and #80) and are on `main`: the AT-SPI2 client (#87), with `Grounding` on it in draft #94 and `Automation` in draft #112, and `register_audio_thread` through RealtimeKit (#90), begun ahead of WP4, whose default endpoints wait for the audio-server client. Input, capture, `TextInsertion`, `SystemVolume` and the HUD are not started.
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

Running the PR lanes on Ubuntu 24.04 over the then unmerged `wp0/base-crates` work (`backup/wp0-base-crates` at ca336be) gave 305 of 305 tests passing and two failures that the Windows jobs cannot show. Reading the build guard's Linux memory code then showed a third problem, which no test exercised. All three are fixed on `main`: the base crates merged as #73 (a67d096) with a fix for each, as the write-up below proposed. The three findings are kept here as the lane found them.

- **A Linux-only clippy error.** In `eileen-supervisor`, `qos.rs` line 106, the test `an_audio_boost_reverts_on_drop` calls `drop(boost)`. Off Windows, `AudioBoost` holds no MMCSS handle and implements no `Drop`, so `clippy::drop_non_drop` fails the build under `-D warnings`. A `cfg_attr(not(windows), expect(clippy::drop_non_drop, reason = "..."))` on that statement, or ending the scope instead of calling `drop`, fixes it. #73 ends the boost with its scope.
- **A yanked pin.** `tokio` 1.54.0, BUILD.md 4.1's pin, is yanked on crates.io, so `cargo deny --locked check` fails. 1.54.1 appeared on 2026-10-21. The newest release outside the 14-day window was 1.53.2, of 2026-10-03. Pull request #27 moved BUILD.md 4.1's pin and the root manifest to 1.53.2, and #59 then to 1.53.3 for RUSTSEC-2026-0386. #73 locks section 4.1's pin, so `main` locks 1.53.3.
- **Commit headroom that Linux does not enforce.** On Linux the guard reads commit headroom as `CommitLimit` minus `Committed_AS`. The kernel enforces that limit only under strict overcommit (`vm.overcommit_memory = 2`). In the default mode, 0, `Committed_AS` can pass `CommitLimit` on a busy desktop, because browsers and runtimes reserve far more than they touch [inferred: the measurement below shows the mechanism, not how often a real desktop reaches it]. The headroom then reads 0 while gigabytes are free, and slot 0's rule refuses every build.
  - Measured on 2026-10-21, WSL2 with 16 GB and default overcommit: a 12 GB mapping that is never touched left `MemAvailable` at 13,957 MB but took the headroom from 9,691 MB to 0. Slot 0's rule, at the default 2,048 MB floor plus a two-job build's 2,048 MB estimate, went from admitting to refusing.
  - The fix that matches the kernel's policy is the one `LinuxSystemMemory` uses: under strict overcommit, keep `CommitLimit` minus `Committed_AS`; otherwise use `MemAvailable` plus `SwapFree`, the memory that can still be backed before the OOM killer acts. The guard and the runtime write the same machine-wide sample files, so both should use the same rule. #73's guard reads commit headroom by `vm.overcommit_memory`, as `LinuxSystemMemory` does.

`main` itself passes the whole lane on Ubuntu 24.04: at 5a4b11b, CI's `logic-linux` job ran 948 of 948 tests (run 40238334151).

## 11. Developer setup on Ubuntu, without sudo

This is the setup the local Linux lane runs on: Ubuntu 24.04 under WSL2 on Windows 11, with every tool under the home folder and no `sudo`. It runs every check of CI's `hygiene` and `logic-linux` jobs. A native Ubuntu install or a virtual machine works the same way. Since #98, `cargo xtask setup` runs step 2's `rustup toolchain install`, installs step 3's tools once cargo-binstall is there, and sets the git hooks, and `cargo xtask ci` runs step 4's lanes through the build guard ([DEV-SETUP.md](DEV-SETUP.md)). The steps below do the same by hand.

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

Later phases need system packages, which do need `sudo`: `pkg-config`, `clang` and `libpipewire-0.3-dev` for PipeWire's bindings (L2), `libasound2-dev` beside `pkg-config` once `cpal` joins the Linux target tables, which BUILD.md section 4.1 does not allow yet (`cpal` 0.18.2 takes `alsa` 0.11 on Linux whatever its features, and `alsa` takes `alsa-sys` 0.4, which links libasound through pkg-config), and `weston` for the headless desktop lane (L1).
