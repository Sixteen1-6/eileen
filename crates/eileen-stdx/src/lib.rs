//! The small things every Eileen crate shares: where time and IDs come from, named threads and
//! sleep, the core's seeded random numbers, soft assertions, fault points and checked casts.
//!
//! # Owns
//!
//! - The clock and ID ports ([`clock::Clock`], [`ids::Ids`]) and their one real implementation
//!   each, plus the one way to start a thread or sleep on it ([`thread`]).
//! - [`rng::CoreRng`], the core's seeded generator, with no `rand`.
//! - The `always!`, `never!` and `buggify!` macros and the checked conversions in [`num`].
//!
//! # Does not own
//!
//! - Async tasks and channels (`eileen-rt`), tracing (`eileen-trace`), child processes
//!   (`eileen-supervisor`). The core's own use of time and IDs is `eileen-core`'s.
//!
//! # Invariants
//!
//! - The only `#[expect(clippy::disallowed_methods)]` here sit on [`clock::SystemClock`],
//!   [`ids::OsIds`], [`thread::spawn_named_with_stack`] and [`thread::sleep`] (check 14 holds
//!   the list).
//! - [`rng::CoreRng`] gives the same sequence for a seed on every OS (unit test
//!   `rng::tests::matches_the_reference_vectors`).
//! - `buggify!` never fires unless [`buggify::enable`] was called on the same thread (unit test
//!   `buggify::tests::off_unless_enabled`).
//! - A fault site is the name written at it (`buggify!("store.write")`), never its file and
//!   line, so a seed switches on the same sites on every OS and after unrelated edits (unit
//!   test `buggify::tests::activation_is_pinned_for_a_seed_and_a_name`).
//!
//! # Boundaries
//!
//! - Called by every Eileen crate except `eileen-buildguard`, `xtask` and `tools/repo-tools`.
//! - May depend on `eileen-contracts` and `ulid` only: std, no IO beyond threads and sleep.
//! - Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-stdx`.
//! - Layers: unit tests beside each module, property tests for the RNG's ranges and the casts.
//! - A bug here starts as a replay that diverges (RNG or IDs) or a cast that silently wrapped.
//!
//! # Traps
//!
//! - A soft assertion panics in debug builds until `eileen_trace::init` switches the process
//!   to reporting; tests and the simulator rely on the panic.
//! - `buggify!` state is per thread: enable it on the thread that runs the core.
//! - [`clock::system`] starts its monotonic time at its first use, not at process start.
//!
//! # Logs and tracing
//!
//! - None: this crate cannot depend on `eileen-trace`. Soft assertion failures go through the
//!   hook `eileen_trace::init` installs ([`soft_assert::set_hook`]).
//!
//! # Do not change without a decision record
//!
//! - The `CoreRng` algorithm and seeding, and the fault sites' activation hash (recorded seeds
//!   would replay differently).
//! - The list of `#[expect]`ed disallowed methods above.

#![forbid(unsafe_code)]

pub mod buggify;
pub mod clock;
pub mod ids;
pub mod num;
pub mod rng;
pub mod soft_assert;
pub mod thread;
