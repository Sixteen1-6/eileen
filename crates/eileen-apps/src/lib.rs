//! Eileen's app adapters (BUILD.md section 2.7): the neutral MCP server behind the `apps_*`
//! tools, with the Resolve bridge and its MCP route, media and the URL builder for video pages,
//! Office, Explorer, recent work, face picking and sharpness on images, and `routes_lookup`, run
//! as the `eileen-apps` sidecar. A lib and a binary. A stub until WP6 builds it; the stub binary
//! says it is not built yet.
//!
//! # Owns
//!
//! - The tool handlers and their `ToolSpec`s with per-op tiers, `observes` rules and
//!   `preview_before`; the bridge line protocol, drop-frame-aware timecode math and the render
//!   events; the media history; recent-work ranking.
//! - `routes_lookup` and the installer for approved MCP servers; `models.toml` for the face
//!   model and `eileen-apps setup` and `doctor` ([`cli`]).
//! - The `resolve_reply` and `registry_reply` fuzz targets' entries (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The Windows implementations of `AppHost`, `Apps`, `MediaSessions` and `RecentSources`
//!   (`eileen-apps-win`), the Lua bridge itself (`adapters/resolve/`), or MCP (`eileen-mcp`).
//!
//! # Invariants
//!
//! - Bridge replies are parsed as JSON, never compared as strings.
//! - "Official" is never said of a route; the publisher's verified namespace is shown instead.
//! - `ort` appears only under the Windows target table (section 4.1, check 15).
//!
//! # Boundaries
//!
//! - Started by `eileen-runtime` through the supervisor; called over MCP.
//! - May depend on `eileen-mcp`, `eileen-platform`, `eileen-supervisor`, `eileen-rt`,
//!   `eileen-contracts`, `eileen-stdx` and `eileen-trace`; `eileen-apps-win` and `ort` under the
//!   Windows target table, `eileen-apps-linux` under the Linux one.
//! - Wire types: the `apps` toolset's arguments and results (`eileen-contracts::tools::apps`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-apps` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-apps`).
//! - Layers: neutral unit tests on Linux (timecode, URLs, ranking, the Laplacian on decoded
//!   images), the two fuzz targets, and desktop tests with Resolve on the owner's PC.
//! - A bug here starts as a playhead one frame off, or a render reported done that was not.
//!
//! # Traps
//!
//! - The registry query runs `curl.exe` through the supervisor against one fixed host, with a
//!   10 s deadline and a 256 KB cap.
//!
//! # Logs and tracing
//!
//! - None yet. WP6's span is `apps.call` from `eileen_trace::names`, taking its remote parent
//!   from the call.
//!
//! # Do not change without a decision record
//!
//! - The bridge's line protocol and the installer's refusal rules.

#![forbid(unsafe_code)]

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;
