//! Integration tests of `eileen-shell-linux` through its public API: the probe and the memory
//! sampler over the fixture machines in `tests/fixtures/`, and once over the machine the tests
//! run on, and the fuzz seed corpora; Eileen's autostart entry, in a folder of each test's own
//! that a guard removes; the session's lock state and keeping it awake, against fake logind,
//! GNOME session manager and Inhibit portal services on `eileen-dbus-linux`'s private bus; the
//! notification client against fake servers on the same bus; the power facts over D-Bus against
//! fake daemons there too; and `ShellHost` through `LinuxShell` on a bridge to that bus, against
//! the same fakes. Linux only, as the crate is. Each module is `cfg(test)`, so clippy holds it to
//! the test rules (`clippy.toml`'s test allowances and `tests_outside_test_module`).

#![cfg(target_os = "linux")]

#[cfg(test)]
mod autostart;
#[cfg(test)]
mod codeowners;
#[cfg(test)]
mod fuzz_corpus;
#[cfg(test)]
mod keep_awake;
#[cfg(test)]
mod live;
#[cfg(test)]
mod machines;
#[cfg(test)]
mod memory;
#[cfg(test)]
mod notifications;
#[cfg(test)]
mod session_lock;
#[cfg(test)]
mod shell_host;
#[cfg(test)]
mod tree;
#[cfg(test)]
mod upower;
