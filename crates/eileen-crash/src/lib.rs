//! Crash capture for every Eileen process (BUILD.md sections 2.1 and 2.13): the in-process crash
//! handler that hands a minidump request to `eileen-guard` over `minidumper`, and the panic hook
//! that cleans a panic's message before it is logged. A stub until WP2 builds it; it holds only
//! the native dependencies, so their build failures show up in W0.
//!
//! # Owns
//!
//! - The crash handler of every process (`crash-handler` on Windows, in a `cfg(windows)` module
//!   with its unsafe code in a private `ffi` module).
//! - The minidump client side of the guard's `minidumper` server.
//! - The panic-hook message cleaning, tested on Linux.
//!
//! # Does not own
//!
//! - The guard process that writes the dumps and restarts the core (`eileen-guard`), incidents
//!   (`eileen-runtime`), or symbolication (`cargo xtask symbolicate`).
//!
//! # Invariants
//!
//! - `crash-handler` and `minidumper` appear only under this crate's and `eileen-guard`'s
//!   Windows target tables (section 4.1, check 15).
//! - Unsafe code only in the private `ffi` module, one operation per block (check 10).
//! - A dump never carries the user's words into a log line: messages pass the `pii.` policy
//!   (WP2 acceptance 7's redaction canary).
//!
//! # Boundaries
//!
//! - Called by every Eileen binary at start.
//! - May depend on `eileen-contracts` and `eileen-trace`, and on `crash-handler` and
//!   `minidumper` under the Windows target table.
//! - Must not depend on `eileen-guard` (the dependency runs the other way), `eileen-rt`,
//!   `eileen-supervisor` or rmcp, nor know vendor CLI flags (section 3.1); nor on a `-win` or
//!   `-linux` crate, or on its native libraries outside the Windows target table (check 15).
//! - Wire types: `minidumper`'s own protocol to the guard.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-crash` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-crash`).
//! - Layers: unit tests for the message cleaning; a Windows test that a crashing child leaves a
//!   dump the guard wrote.
//! - A bug here starts as a crash with no dump, or a dump with no symbols.
//!
//! # Traps
//!
//! - The handler runs in a crashed process: it allocates nothing and takes no lock.
//!
//! # Logs and tracing
//!
//! - None yet; WP2 declares its events in `eileen_trace::names` and logs a dump's file name,
//!   never its contents.
//!
//! # Do not change without a decision record
//!
//! - Where dumps are written and how long they are kept.
