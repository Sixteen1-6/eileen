//! [`buggify!`](crate::buggify): fault points that fire only under the simulator.
//!
//! A fault point marks a place where the real world can misbehave (a reply arrives late, a
//! write fails, a queue fills). Outside a simulator run it is a thread-local check that always
//! answers false. Inside one, each site is switched on for the whole run with probability
//! `activate_per_mille / 1000`, decided from the run's seed and the site's location so the
//! choice does not depend on which site is reached first, and an active site then fires on
//! each evaluation with probability `fire_per_mille / 1000` from the run's own [`CoreRng`].
//! This is a runtime switch, never a Cargo feature: the shipped binary contains the sites and
//! never enables them.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::marker::PhantomData;

use crate::rng::CoreRng;

/// The default chance a site is active in a run, per mille (`FoundationDB`'s 25 percent).
pub const DEFAULT_ACTIVATE_PER_MILLE: u32 = 250;
/// The default chance an active site fires when reached, per mille.
pub const DEFAULT_FIRE_PER_MILLE: u32 = 250;

struct Run {
    seed: u64,
    rng: CoreRng,
    activate_per_mille: u32,
    fire_per_mille: u32,
    fired: BTreeMap<&'static str, u64>,
}

thread_local! {
    static RUN: RefCell<Option<Run>> = const { RefCell::new(None) };
}

/// Fault points switched on for this thread. Dropping it switches them off again.
#[derive(Debug)]
#[must_use = "fault points switch off again when the scope is dropped"]
pub struct Scope {
    // Not Send: the state it clears lives on the thread that enabled it.
    _thread_bound: PhantomData<*const ()>,
}

impl Drop for Scope {
    fn drop(&mut self) {
        RUN.set(None);
    }
}

/// Switches fault points on for this thread with the default chances. A second call replaces
/// the first run.
pub fn enable(seed: u64) -> Scope {
    enable_with(seed, DEFAULT_ACTIVATE_PER_MILLE, DEFAULT_FIRE_PER_MILLE)
}

/// Switches fault points on for this thread with the given chances, per mille.
pub fn enable_with(seed: u64, activate_per_mille: u32, fire_per_mille: u32) -> Scope {
    RUN.set(Some(Run {
        seed,
        rng: CoreRng::from_seed(seed ^ 0xB066_1F1E_D000_0000),
        activate_per_mille,
        fire_per_mille,
        fired: BTreeMap::new(),
    }));
    Scope { _thread_bound: PhantomData }
}

/// True inside a simulator run on this thread.
#[must_use]
pub fn is_enabled() -> bool {
    RUN.with_borrow(Option::is_some)
}

/// The sites that fired in this thread's run so far, with how often, in site order.
#[must_use]
pub fn fired() -> Vec<(&'static str, u64)> {
    RUN.with_borrow(|run| {
        run.as_ref()
            .map(|run| run.fired.iter().map(|(s, n)| (*s, *n)).collect())
            .unwrap_or_default()
    })
}

/// Evaluates the fault point at `site`. Called by the macro, not by hand.
#[doc(hidden)]
#[must_use]
pub fn hit(site: &'static str) -> bool {
    RUN.with_borrow_mut(|run| {
        let Some(run) = run.as_mut() else {
            return false;
        };
        if !site_active(run.seed, site, run.activate_per_mille) {
            return false;
        }
        let fire = run.rng.below(1000) < u64::from(run.fire_per_mille);
        if fire {
            *run.fired.entry(site).or_insert(0) += 1;
        }
        fire
    })
}

/// Whether `site` is active in the run with `seed`: FNV-1a over the seed and the site, mixed.
fn site_active(seed: u64, site: &str, per_mille: u32) -> bool {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in seed.to_le_bytes().iter().chain(site.as_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01B3);
    }
    let mut rng = CoreRng::from_seed(hash);
    rng.below(1000) < u64::from(per_mille)
}

/// A fault point: true when the simulator decides this place should misbehave now.
///
/// ```
/// fn write_reply() -> Result<(), &'static str> {
///     if eileen_stdx::buggify!() {
///         return Err("injected: the write failed");
///     }
///     Ok(())
/// }
/// assert_eq!(write_reply(), Ok(()));
/// ```
#[macro_export]
macro_rules! buggify {
    () => {
        $crate::buggify::hit(::core::concat!(
            ::core::file!(),
            ":",
            ::core::line!(),
            ":",
            ::core::column!()
        ))
    };
}

#[cfg(test)]
mod tests {
    use super::{enable, enable_with, fired, hit, is_enabled};

    #[test]
    fn off_unless_enabled() {
        assert!(!is_enabled(), "off by default");
        assert!((0..1_000).all(|_| !crate::buggify!()), "never fires when off");
    }

    #[test]
    fn a_run_is_deterministic_and_ends_with_its_scope() {
        let run = |seed| {
            let _scope = enable_with(seed, 1000, 500);
            let hits: Vec<bool> = (0..64).map(|_| hit("site-a")).collect();
            (hits, fired())
        };
        assert_eq!(run(11), run(11), "same seed, same faults");
        assert!(!is_enabled(), "the scope switched it off");
        let (hits, counts) = run(11);
        let fired_a = hits.iter().filter(|h| **h).count();
        assert_eq!(counts, vec![("site-a", u64::try_from(fired_a).unwrap())], "counted");
    }

    #[test]
    fn activation_is_per_site() {
        let _scope = enable(5);
        let sites = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"];
        let active = sites.iter().filter(|s| super::site_active(5, s, 250)).count();
        assert!(active < sites.len(), "not every site is active at 25 percent");
        let never = enable_with(5, 0, 1000);
        assert!((0..100).all(|_| !hit("a")), "no site is active at zero");
        drop(never);
    }
}
