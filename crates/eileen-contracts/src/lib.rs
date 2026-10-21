//! The types every Eileen process, pipe, file and socket shares, with their JSON Schemas.
//!
//! One module per producing area (BUILD.md section 3.3). The modules on slice S1's path are
//! pinned: their snapshots are the contract, and a change is an amendment. The rest are marked
//! provisional in `modules.toml` and in their own module doc, and the package that owns their
//! producer pins them in its first pull request.
//!
//! # Owns
//!
//! - Every type that crosses a process, pipe, file or socket, and the ports the core calls.
//! - The JSON Schemas generated from them ([`schema`]) and the snapshots that pin them.
//! - The text rule: free text from outside Eileen's own code is [`ids::Untrusted`] or
//!   [`ids::Personal`], and text from UI Automation enters only through [`desktop::sanitize`].
//!
//! # Does not own
//!
//! - Behaviour. The few functions here are pure helpers over the types (ID text forms, dates,
//!   the scenario expiry rule, `sanitize`); every decision lives in `eileen-core` and the crates
//!   behind the ports.
//! - Types that appear only in another crate's own signatures (driver errors, harness and
//!   platform handles, the platform traits of section 3.4): those crates define them.
//!
//! # Invariants
//!
//! - Every serialized type derives `serde` and `schemars`, and every module's schemas are pinned
//!   by one insta snapshot (the tests `it::schemas::<module>_matches_its_snapshot`, one per
//!   module, which `it::schemas::every_module_has_a_snapshot_test` keeps complete).
//! - Every definition the generator produces belongs to exactly one module, and schema names are
//!   unique (test `it::schemas::every_definition_has_one_owner`).
//! - Maps that reach an effect or a snapshot are `BTreeMap`; snapshots sort their keys, so they
//!   do not depend on a feature-unified `preserve_order` (the `sort_maps` setting).
//! - Every free-text string reachable from `Input`, `ToolResult` and `DesktopState` is wrapped
//!   or listed in `text-allowlist.toml` with a reason (test
//!   `it::text::free_text_is_wrapped_or_allowlisted`).
//! - `modules.toml` lists exactly the modules here, and a provisional module says so in its doc
//!   (test `it::modules::modules_toml_matches_the_crate`).
//!
//! # Boundaries
//!
//! - Called by every Eileen crate. May depend on `serde`, `serde_json`, `schemars` and `ulid`
//!   only: no async runtime, no IO, no other Eileen crate.
//! - Wire types: all of them. The speech pipe's frame header is a `#[repr(C)]` layout.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-contracts` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-contracts`).
//! - Layers: unit tests beside the helpers, property tests for the text forms, one snapshot per
//!   module and per toolset, the text walk, and the scenario example parsed from TOML.
//! - A bug here starts as a failing round trip or a snapshot diff that nobody meant.
//!
//! # Traps
//!
//! - A schema name used twice gets a numeric suffix from schemars; the ownership test fails
//!   first, so give the second type a `#[schemars(rename)]`.
//! - `Option` fields serialize as `null` (the dispatcher rebuilds `{"action": "status",
//!   "session": null}`); a missing `Option` reads as `None`, a missing `Vec` does not unless the
//!   field has `#[serde(default)]`.
//! - `Personal` hides its text from `Debug`; read it with `get()` where a sink applies the policy.
//!
//! # Logs and tracing
//!
//! - None: this crate does no IO. Sinks apply the `pii.` policy to `Personal` values.
//!
//! # Do not change without a decision record
//!
//! - The text forms of the IDs, `Date` and `MachineHash`; the speech frame header; the
//!   `x-eileen-text`, `x-eileen-path` and `x-eileen-match` schema extensions.
//! - A new top-level `Input`, `Effect` or `BusEvent` variant (an amendment to BUILD.md).

#![forbid(unsafe_code)]

pub mod bus;
pub mod config;
pub mod connectors;
pub mod core_io;
pub mod desktop;
pub mod fleet;
#[doc(hidden)]
pub mod fuzz_entry;
pub mod harness;
pub mod hud;
pub mod ids;
pub mod intent;
pub mod jobs;
pub mod learn;
pub mod ports;
pub mod record;
pub mod scenario;
pub mod schema;
pub mod speech;
pub mod tools;
