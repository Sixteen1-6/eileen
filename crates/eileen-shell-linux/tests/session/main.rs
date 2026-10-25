//! The session tests of `eileen-shell-linux`: the crate against the real services of a desktop
//! session, on the real session and system buses. nextest's `linux-session` profile runs them,
//! which the Linux desktop lane (`.github/workflows/linux-desktop.yml`) uses inside its headless
//! session. Every other profile's default filter leaves out `linux_session` modules, except the
//! `owner` profile's `all()`, which takes them; on the owner's Windows PC this binary is empty
//! (BUILD.md section 4.7). Linux only, as the crate is. The module is `cfg(test)`, so clippy holds
//! it to the test rules, as in `tests/it`.

#![cfg(target_os = "linux")]

#[cfg(test)]
mod linux_session;
