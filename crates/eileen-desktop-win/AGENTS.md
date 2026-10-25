# eileen-desktop-win

The Windows half of desktop control: UI Automation, capture, input, the app index, text insertion
and the system volume, behind the `eileen-platform` traits (BUILD.md sections 2.6 and 3.4). WP5
owns it. Read the crate doc in `src/lib.rs` first; this file adds the rules only this folder has.

## Why this folder has its own rules

This crate calls COM and drives real windows, so a mistake hangs a thread or touches the user's
desktop. UI Automation needs its own apartment thread and a deadline on every call, and its tests
must run one at a time on windows they own.

## Rules

- Every UI Automation, COM and input call runs on the crate's own apartment thread
  (`eileen_stdx::thread::spawn_named`) with the caller's `Deadline`, never on a tokio worker. Past
  the deadline the call answers `PlatformError::DeadlineExceeded`; a hung provider's thread is
  abandoned and replaced, never waited on.
- Unsafe code lives only in `ffi/`, one file per API area (`ffi/uia.rs`, `ffi/input.rs`), with
  `.claude/rules/ffi.md`'s policy.
- Text leaving the crate goes through `eileen_contracts::desktop::sanitize`. No global UIA focus
  handler; the window index is persisted and rebuilt on start.
- The Raw Input module (`ffi::raw_input`) never stores or forwards a key code other than the
  configured dictation key, and for the approve, deny and unmute chords only a "came from a
  device" flag; never a pointer position (check 25).
- Tests touch only what they create: the `testpad` fixture, Notepad launched by AUMID and closed
  unsaved, an isolated Edge profile. The clipboard is restored from a full-format snapshot.
- The whole package is in nextest's `desktop` group, so its tests run one at a time, and
  `cargo xtask test` takes `desktop.lock` for them on the owner's PC.
- Geometry is per-monitor DPI aware (V2), in physical pixels.
