//! The session tests of `eileen-shell-linux`: the crate against the real services of a desktop
//! session, on the real session and system buses. The session is the Linux desktop lane's, which
//! `.github/linux-desktop/session.sh` makes for them, headless and with no screen locker, and they
//! are for it alone: on a developer's own desktop the lock test can lock the screen, after which
//! the check that the session reads unlocked can fail, and the power test leaves the power profile
//! at `balanced`, the lane's.
//!
//! The lane's workflow, `.github/workflows/linux-desktop.yml`, runs them inside that session with
//! nextest's `linux-session` profile. Every other profile's default filter leaves out
//! `linux_session` modules, except the `owner` profile's `all()`, which takes them: on the owner's
//! Windows PC this binary is empty, and the profile must not run them on a Linux desktop (BUILD.md
//! section 4.7). Linux only, as the crate is. The module is `cfg(test)`, so clippy holds it to the
//! test rules, as in `tests/it`.

#![cfg(target_os = "linux")]

#[cfg(test)]
mod linux_session;
