//! Build slots with memory admission, so agents building Eileen on a 16 GB PC never thrash it
//! (BUILD.md section 6.1 rule 1). `cargo xtask` and the harness's build-heavy coding jobs
//! take a slot through [`acquire`]; Eileen's runtime reads the machine-wide memory files
//! through [`samples`].
//!
//! # Owns
//!
//! - The two slot locks and `desktop.lock` with their holder records ([`slots`]), admission
//!   by available memory, commit headroom and paging ([`admission`]), the wait and exit 75.
//! - The machine-wide files: the memory samples, the 30 s paging windows and
//!   `buildguard.toml`, with the baseline that writes it ([`samples`], [`baseline`]).
//! - [`spawn_cargo`]: cargo in a kill-on-close Job Object at below-normal priority.
//!
//! # Does not own
//!
//! - The `cargo xtask` command line (`xtask`), runtime memory admission (`eileen-core` decides,
//!   `eileen-runtime` samples), `procinfo` for `measure-idle` (phase B).
//!
//! # Invariants
//!
//! - std and `windows-sys` only, so xtask compiles in seconds; unsafe code only in `ffi`.
//! - The job count never falls as available memory rises (property test
//!   `admission::tests::jobs_never_fall_as_memory_rises`).
//! - A fake memory source never writes the machine-wide files (`guard::take_sample`).
//! - The rule a build is admitted under depends on whether another build holds a slot, never
//!   on which index is free, and a refused attempt takes no lock and writes no record
//!   (`it::admission::a_second_build_on_slot0_meets_slot1s_requirement_while_slot1_runs`,
//!   `guard::tests::a_refused_attempt_takes_no_lock_and_writes_no_record`).
//! - The only `#[expect(clippy::disallowed_methods)]`: [`spawn_cargo`] and the sampling clock
//!   and sleep in [`clock`] (check 14).
//!
//! # Boundaries
//!
//! - Called by `xtask`, `eileen-harness` (slots for coding jobs) and `eileen-runtime` (the
//!   [`samples`] module only).
//! - Must not depend on any other crate: no Eileen crate, no serde, no tokio.
//! - Wire types: the two JSON-lines formats and `buildguard.toml` (see [`samples`]).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-buildguard`.
//! - Layers: unit tests of the rules with fake memory and a manual clock; integration tests
//!   with child processes (the test binary re-run as a helper) for the cross-process files,
//!   the stale holder and, on Windows, the kill-on-close job.
//! - A bug here starts as a build that ran when it should have waited, or waited for ever.
//!
//! # Traps
//!
//! - Windows file locks are mandatory: a holder's record lives beside its lock, never in it.
//! - Point a test at a temporary root with [`samples::Store::at`]; the machine root comes from
//!   `LOCALAPPDATA` (off Windows, when unset, the XDG state folder; on Windows nothing else),
//!   never from `EILEEN_DATA_DIR`.
//! - Every wait runs on the caller's [`clock::Clock`] (admission, the re-check, a reading's
//!   minimum interval, the samples lock), so a test on a `ManualClock` never waits for real.
//! - The paging rate is judged as a 30 s average against limits measured on this PC; a
//!   number from elsewhere means nothing.
//! - [`SystemMemory`] is not `Send` on any OS (its Windows PDH query stays on the thread that
//!   opened it), and its first reading waits until its rate covers at least
//!   [`memory::MIN_RATE_INTERVAL`] since it opened.
//! - A probe of a slot lock ([`slots::is_slot_held`], [`samples::Store::slot_held`]) holds it
//!   for an instant, so a take that meets a probe refuses for that tick and decides again on
//!   the next.
//! - Unlock a lock before its handle closes: Windows releases a lock left on a closed handle
//!   only when the system gets to it, so a probe or a finished build could hold a slot longer
//!   than it ran (test
//!   `slots::tests::a_slot_is_free_to_take_straight_after_a_release_and_a_probe`).
//!
//! # Logs and tracing
//!
//! - None: no tracing dependency. Waiting reasons go to the caller's `report` callback, which
//!   xtask prints every 30 s with the holders.
//!
//! # Do not change without a decision record
//!
//! - The line formats, the lock file names and `buildguard.toml`'s keys (the runtime reads
//!   them); exit code 75; the job formula.

pub mod admission;
pub mod baseline;
pub mod clock;
pub mod disk;
#[cfg(windows)]
#[expect(unsafe_code, reason = "the Win32 calls for memory, paging, disk and the cargo Job Object")]
mod ffi;
pub mod guard;
mod json;
pub mod memory;
pub mod pressure;
pub mod procs;
pub mod samples;
pub mod slots;
pub mod spawn;

pub use admission::{Reason, Thresholds, jobs_for};
pub use guard::{
    EXIT_NO_SLOT, Env, Refusal, Request, SLOT_HELD_ENV, Slots, Ticket, acquire, acquire_desktop,
};
pub use memory::{FakeMemory, MemorySample, MemorySource, SystemMemory};
pub use samples::Store;
pub use spawn::{CargoChild, CargoInvocation, spawn_cargo};
