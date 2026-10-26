//! Integration tests of `eileen-audio-linux` through its public API: the `RealtimeKit` client
//! against a fake rtkit on `eileen-dbus-linux`'s private bus, and `AudioEndpoints` through
//! `LinuxAudioEndpoints` on a bridge to that bus, against the same fake. nextest runs each test in
//! a process of its own, so the `RLIMIT_RTTIME` bound a promotion sets stays in that test's
//! process. Linux only, as the crate is. Each module is `cfg(test)`, so clippy holds it to the
//! test rules (`clippy.toml`'s test allowances and `tests_outside_test_module`).

#![cfg(target_os = "linux")]

#[cfg(test)]
mod endpoints;
#[cfg(test)]
mod rtkit;
