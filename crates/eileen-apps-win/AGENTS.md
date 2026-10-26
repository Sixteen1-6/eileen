# eileen-apps-win

The Windows half of the app adapters: media sessions (SMTC), the app hooks (Office and Explorer
COM, the Resolve process gate) and the recent-work sources, behind the `eileen-platform` traits
(BUILD.md sections 2.7 and 3.4). WP6 owns it. Read the crate doc in `src/lib.rs` first; this file
adds the rules only this folder has.

## Why this folder has its own rules

This crate talks to other applications over COM and WinRT, so one slow app can hang a call and a
careless test can change a user's document or project. Calls need their own apartment thread and
a deadline, and tests may only touch fixtures they own.

## Rules

- Every COM and WinRT call runs on the crate's own apartment thread with the caller's `Deadline`,
  never on a tokio worker; past it the call answers `PlatformError::DeadlineExceeded`, and the
  thread is abandoned and replaced if it never returns.
- With Resolve closed no call stalls: the process gate answers that it is not running at once.
- Unsafe code lives only in `ffi/`, with `.claude/rules/ffi.md`'s policy.
- Tests touch only what they own: documents the test creates and closes without saving, a
  silent-WAV media session, and in Resolve only the dedicated test project, with the owner's
  agreement and both build slots while Resolve runs (BUILD.md section 6.1 rule 2).
- The whole package is in nextest's `desktop` group, so its tests run one at a time, and
  `cargo xtask test` takes `desktop.lock` for them on the owner's PC.
- Text from an app (titles, selections, recent file names) is `Untrusted` or `Personal` before it
  leaves the crate.
