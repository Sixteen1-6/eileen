//! Eileen's one doorway to MCP (BUILD.md sections 2.1 and 3.3.9): the sidecars' servers and the
//! core's clients over supervised stdio, and the protocol `eileen-shim` speaks to the claude CLI.
//! A stub until WP2 builds it on rmcp; for now it holds the [`handshake`] the stub shim answers.
//!
//! # Owns
//!
//! - Everything that knows rmcp: the sidecar servers, the core's clients with deadlines,
//!   heartbeats and restart with backoff, and the shim's side of section 3.3.9.
//! - [`handshake`]: the stub shim's answers to `server/discover`, `initialize` and `tools/list`.
//! - The `mcp_client_message`, `mcp_tool_arguments` and `dispatch_line` fuzz targets' entries
//!   (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The tools (the sidecars and the core's dispatcher own their handlers), the dispatcher pipe
//!   and caller verification (`eileen-runtime`), or the shim binary (`eileen-shim`).
//!
//! # Invariants
//!
//! - A sidecar that dies mid-call gives its caller a typed error, and its client restarts it
//!   within the backoff bound (WP2 acceptance 2).
//! - The stub answers `server/discover` with method-not-found, so the CLI falls back to
//!   `initialize` at [`handshake::PROTOCOL_VERSION`], and lists no tools (unit tests
//!   `handshake::tests::the_discover_probe_gets_method_not_found`,
//!   `handshake::tests::initialize_names_the_server_and_the_protocol_version` and
//!   `handshake::tests::the_tool_list_is_empty`).
//!
//! # Boundaries
//!
//! - Called by `eileen-runtime`, `eileen-shim`, `eileen-desktop`, `eileen-apps` and `eileen-e2e`.
//! - May depend on `eileen-rt`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`, and on
//!   rmcp, which only this crate, and the binaries through it, may know (section 3.1).
//! - Must not depend on `eileen-core`, on `eileen-runtime`, `eileen-shim` or a sidecar's crate
//!   (they depend on it), nor know vendor CLI flags (section 3.1); nor on a `-win` or `-linux`
//!   crate (check 15).
//! - Wire types: MCP's JSON-RPC; Eileen's own pipe lines are contract types (`tools::ShimLine`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-mcp` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-mcp`).
//! - Layers: unit tests for the handshake; WP2 adds the client against stand-in sidecars, the
//!   shuttle test of a dropped shim pipe, and the three fuzz targets.
//! - A bug here starts as a session whose `system/init` lists no `eileen` server.
//!
//! # Traps
//!
//! - The claude CLI opens with `server/discover`; answering anything but method-not-found would
//!   commit the shim to a protocol version its library does not speak.
//! - rmcp's `#[tool_handler]` trips `unused_async`: one `#[expect]` per server type with the
//!   reason "rmcp tool handler signature" is pre-approved (section 4b.2).
//!
//! # Logs and tracing
//!
//! - None yet; WP2 logs the `_meta` the CLI sends on its first call and passes
//!   `_meta.traceparent` through.
//!
//! # Do not change without a decision record
//!
//! - The server name `eileen` (Claude sees `mcp__eileen__<tool>`) and the protocol version.

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod fuzz_entry;
pub mod handshake;
