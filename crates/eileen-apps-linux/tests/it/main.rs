//! Integration tests of `eileen-apps-linux` through its public API: the app index over the fixture
//! session in `tests/fixtures/session/`, and once over the session the tests run in; the fuzz seed
//! corpus; the opener against a fake `OpenURI` portal and fake D-Bus-activatable apps, and media
//! sessions against fake MPRIS players, on `eileen-dbus-linux`'s private bus; the recent files
//! over the bookmark files in `tests/fixtures/recent/`; and the `Apps` and `RecentSources`
//! implementations over the same fixtures and fakes (`apps`, `recent_sources`). `tests/fixtures/`'s
//! README says how each fixture was made. Linux only, as the crate is. Each module is `cfg(test)`,
//! so clippy holds it to the test rules (`clippy.toml`'s test allowances and
//! `tests_outside_test_module`).

#![cfg(target_os = "linux")]

#[cfg(test)]
mod apps;
#[cfg(test)]
mod fuzz_corpus;
#[cfg(test)]
mod index;
#[cfg(test)]
mod mpris;
#[cfg(test)]
mod open;
#[cfg(test)]
mod recent;
#[cfg(test)]
mod recent_sources;
