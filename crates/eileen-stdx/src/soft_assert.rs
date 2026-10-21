//! Soft assertions: [`always!`](crate::always) and [`never!`](crate::never).
//!
//! A soft assertion states something that should hold but is not worth crashing an always-on
//! process for. When it fails, the failure is counted and passed to the hook `eileen_trace`
//! installs (which logs `soft_assert.failed` and asks for a flight recorder dump); then, in
//! [`Mode::Panic`], it panics. Debug builds start in `Panic`, release builds in `Report`, and
//! `eileen_trace::init` switches every product process to `Report`, so tests and the simulator
//! panic while a running Eileen reports and carries on. Both macros return the condition, so a
//! caller can recover:
//!
//! ```
//! # use eileen_stdx::never;
//! fn pick(items: &[u32]) -> Option<u32> {
//!     if never!(items.is_empty(), "nothing to pick from") {
//!         return None;
//!     }
//!     items.first().copied()
//! }
//! # eileen_stdx::soft_assert::with_mode(eileen_stdx::soft_assert::Mode::Report, || {
//! # assert_eq!(pick(&[]), None);
//! # });
//! # assert_eq!(pick(&[3]), Some(3));
//! ```

use std::cell::Cell;
use std::fmt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// What a failed soft assertion does after it is counted and reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Panic: tests, the simulator, and debug builds before tracing starts.
    Panic,
    /// Carry on: every product process once `eileen_trace::init` has run.
    Report,
}

/// One failed soft assertion, as the hook sees it.
#[derive(Debug, Clone, Copy)]
pub struct Failure<'a> {
    /// The source file of the assertion.
    pub file: &'static str,
    /// Its line.
    pub line: u32,
    /// What it says.
    pub message: fmt::Arguments<'a>,
}

/// The function `eileen_trace` installs to log a failure and request a flight recorder dump.
pub type Hook = fn(&Failure<'_>);

/// True while the process-wide mode is `Panic`.
static PANICS: AtomicBool = AtomicBool::new(cfg!(debug_assertions));
static FAILURES: AtomicU64 = AtomicU64::new(0);
static HOOK: OnceLock<Hook> = OnceLock::new();

thread_local! {
    static OVERRIDE: Cell<Option<Mode>> = const { Cell::new(None) };
}

/// Sets the process-wide mode.
pub fn set_mode(mode: Mode) {
    PANICS.store(mode == Mode::Panic, Ordering::Relaxed);
}

/// The mode in force on this thread.
#[must_use]
pub fn mode() -> Mode {
    OVERRIDE
        .get()
        .unwrap_or_else(|| if PANICS.load(Ordering::Relaxed) { Mode::Panic } else { Mode::Report })
}

/// Runs `f` with `mode` in force on this thread only, then restores the previous one. The
/// simulator runs the core under `Panic` this way, whatever the process mode is.
pub fn with_mode<T>(mode: Mode, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<Mode>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.set(self.0);
        }
    }
    let _restore = Restore(OVERRIDE.replace(Some(mode)));
    f()
}

/// Installs the hook. The first call wins; later calls return false.
pub fn set_hook(hook: Hook) -> bool {
    HOOK.set(hook).is_ok()
}

/// How many soft assertions have failed in this process.
#[must_use]
pub fn failures() -> u64 {
    FAILURES.load(Ordering::Relaxed)
}

/// Records a failure. Called by the macros, not by hand.
#[doc(hidden)]
pub fn fail(file: &'static str, line: u32, message: fmt::Arguments<'_>) {
    FAILURES.fetch_add(1, Ordering::Relaxed);
    let failure = Failure { file, line, message };
    if let Some(hook) = HOOK.get() {
        hook(&failure);
    }
    if mode() == Mode::Panic {
        panic_now(&failure);
    }
}

#[expect(clippy::panic, reason = "a soft assertion is a hard one under tests and the simulator")]
fn panic_now(failure: &Failure<'_>) -> ! {
    panic!("soft assertion failed at {}:{}: {}", failure.file, failure.line, failure.message)
}

/// Checks that `cond` holds. On failure it counts, reports and, in [`Mode::Panic`], panics.
/// Returns `cond`.
#[macro_export]
macro_rules! always {
    ($cond:expr $(,)?) => {
        $crate::always!($cond, "{}", ::core::stringify!($cond))
    };
    ($cond:expr, $($arg:tt)+) => {{
        let cond: bool = $cond;
        if !cond {
            $crate::soft_assert::fail(::core::file!(), ::core::line!(), ::core::format_args!($($arg)+));
        }
        cond
    }};
}

/// Checks that `cond` does not hold. On failure it counts, reports and, in [`Mode::Panic`],
/// panics. Returns `cond`, so `if never!(...) { return ...; }` recovers in production.
#[macro_export]
macro_rules! never {
    ($cond:expr $(,)?) => {
        $crate::never!($cond, "{}", ::core::stringify!($cond))
    };
    ($cond:expr, $($arg:tt)+) => {{
        let cond: bool = $cond;
        if cond {
            $crate::soft_assert::fail(::core::file!(), ::core::line!(), ::core::format_args!($($arg)+));
        }
        cond
    }};
}

#[cfg(test)]
mod tests {
    use std::panic::catch_unwind;

    use super::{Mode, failures, mode, with_mode};

    #[test]
    fn report_mode_counts_and_returns_the_condition() {
        let before = failures();
        let held = with_mode(Mode::Report, || crate::always!(1 + 1 == 3));
        assert!(!held, "always! returns the condition");
        let hit = with_mode(Mode::Report, || crate::never!(true, "value {}", 4));
        assert!(hit, "never! returns the condition");
        assert_eq!(failures() - before, 2, "both counted");
        assert!(with_mode(Mode::Report, || crate::always!(true)), "a holding assertion");
        assert_eq!(failures() - before, 2, "a holding assertion is not counted");
    }

    #[test]
    fn panic_mode_panics() {
        let result = catch_unwind(|| with_mode(Mode::Panic, || crate::never!(true)));
        assert!(result.is_err(), "panicked");
        assert_eq!(super::OVERRIDE.get(), None, "the override is restored after the panic");
        assert_eq!(with_mode(Mode::Report, mode), Mode::Report, "the override is in force");
    }
}
