//! Integration tests of `eileen-desktop-linux` through its public API: the AT-SPI2 client, and
//! `Grounding` and `Automation` through a bridge, against a fake launcher on one private bus and
//! a fake registry and apps on a second (`fake`), both `eileen-dbus-linux`'s
//! `testing::PrivateBus`. Linux only,
//! as the crate is. Each module is `cfg(test)`, so clippy holds it to the test rules
//! (`clippy.toml`'s test allowances and `tests_outside_test_module`).

#![cfg(target_os = "linux")]

#[cfg(test)]
mod atspi;
#[cfg(test)]
mod automation;
#[cfg(test)]
mod fake;
#[cfg(test)]
mod grounding;
