//! The entry point of the `sim` fuzz target (BUILD.md section 4.7): a simulator run whose
//! inputs are drawn from the fuzzer's bytes, checked by every invariant.

use crate::profile::Profile;
use crate::run::run_bytes;

/// The most steps one fuzz input runs: enough for turns, timers and playback to interleave,
/// few enough that the fuzzer tries many inputs a second.
const MAX_STEPS: u32 = 256;

/// One simulator run from bytes, on the calling thread. `Err` names the invariant the run
/// broke.
pub fn sim(data: &[u8]) -> Result<(), &'static str> {
    let profile = Profile::default_profile().map_err(|_| "the default profile does not load")?;
    let steps = u32::try_from(data.len() / 2).unwrap_or(MAX_STEPS).clamp(1, MAX_STEPS);
    match run_bytes(&profile, 0, data, steps).failure {
        None => Ok(()),
        Some((_, violation)) => Err(violation.name),
    }
}

#[cfg(test)]
mod tests {
    use super::sim;

    #[test]
    fn any_bytes_run_and_keep_the_invariants() {
        assert_eq!(sim(&[]), Ok(()), "no bytes");
        assert_eq!(sim(&[0xff; 600]), Ok(()), "a long run of one byte");
        let mixed: Vec<u8> = (0..=255).collect();
        assert_eq!(sim(&mixed), Ok(()), "every byte value");
    }
}
