//! [`CoreRng`], the core's seeded random numbers, with no `rand`.
//!
//! xoshiro256** (Blackman and Vigna), seeded through `SplitMix64` so that any `u64` seed gives a
//! well-mixed state. The sequence for a seed is the same on every OS and in every build, which
//! the simulator's seeds and recorded replays rely on.

/// The core's random number generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreRng {
    s: [u64; 4],
}

/// `SplitMix64`'s increment.
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// One `SplitMix64` step: advances `state` and returns the next output.
const fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(GOLDEN);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl CoreRng {
    /// A generator seeded from `seed`.
    #[must_use]
    pub const fn from_seed(seed: u64) -> Self {
        let mut state = seed;
        let a = splitmix64(&mut state);
        let b = splitmix64(&mut state);
        let c = splitmix64(&mut state);
        let d = splitmix64(&mut state);
        Self { s: [a, b, c, d] }
    }

    /// A generator resumed from a saved [`Self::state`]. `None` for the all-zero state, which
    /// xoshiro cannot leave.
    #[must_use]
    pub const fn from_state(state: [u64; 4]) -> Option<Self> {
        if state[0] == 0 && state[1] == 0 && state[2] == 0 && state[3] == 0 {
            None
        } else {
            Some(Self { s: state })
        }
    }

    /// The state, for a checkpoint.
    #[must_use]
    pub const fn state(&self) -> [u64; 4] {
        self.s
    }

    /// The next 64 random bits.
    pub const fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// The next 32 random bits (the high half of [`Self::next_u64`]).
    pub const fn next_u32(&mut self) -> u32 {
        let [a, b, c, d, ..] = self.next_u64().to_be_bytes();
        u32::from_be_bytes([a, b, c, d])
    }

    /// A uniform value in `0..n`, without modulo bias (Lemire's method). Zero when `n` is zero.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        let mut m = u128::from(self.next_u64()) * u128::from(n);
        let threshold = n.wrapping_neg() % n;
        while low_u64(m) < threshold {
            m = u128::from(self.next_u64()) * u128::from(n);
        }
        high_u64(m)
    }

    /// A uniform value in `lo..hi`; `lo` when the range is empty.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo { lo } else { lo + self.below(hi - lo) }
    }

    /// True with probability `numerator / denominator`; false when the denominator is zero.
    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        denominator != 0 && self.below(u64::from(denominator)) < u64::from(numerator)
    }

    /// Fills `out` with random bytes.
    pub fn fill_bytes(&mut self, out: &mut [u8]) {
        for chunk in out.chunks_mut(8) {
            let bytes = self.next_u64().to_le_bytes();
            for (slot, byte) in chunk.iter_mut().zip(bytes) {
                *slot = byte;
            }
        }
    }

    /// The 80 random bits of a ULID the core mints with `from_parts`.
    pub fn ulid_random(&mut self) -> u128 {
        let high = u128::from(self.next_u64() >> 48);
        let low = u128::from(self.next_u64());
        (high << 64) | low
    }

    /// Splits off an independent generator: the returned one continues from the current
    /// state, and `self` jumps 2^128 steps ahead, so the two streams never overlap in practice.
    #[must_use]
    pub fn fork(&mut self) -> Self {
        let child = self.clone();
        self.jump();
        child
    }

    /// Advances the state by 2^128 steps (the reference `jump` function).
    pub fn jump(&mut self) {
        const JUMP: [u64; 4] = [
            0x180E_C6D3_3CFD_0ABA,
            0xD5A6_1266_F0C9_392C,
            0xA958_2618_E03F_C9AA,
            0x39AB_DC45_29B1_661C,
        ];
        let mut acc = [0_u64; 4];
        for word in JUMP {
            for bit in 0..64 {
                if word & (1_u64 << bit) != 0 {
                    for (slot, value) in acc.iter_mut().zip(self.s) {
                        *slot ^= value;
                    }
                }
                self.next_u64();
            }
        }
        self.s = acc;
    }
}

/// The low 64 bits of `x`.
fn low_u64(x: u128) -> u64 {
    u64::try_from(x & u128::from(u64::MAX)).unwrap_or(0)
}

/// The high 64 bits of `x`.
fn high_u64(x: u128) -> u64 {
    low_u64(x >> 64)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{CoreRng, splitmix64};

    #[test]
    fn matches_the_reference_vectors() {
        let mut state = 1_234_567;
        let split: Vec<u64> = (0..3).map(|_| splitmix64(&mut state)).collect();
        assert_eq!(
            split,
            [6_457_827_717_110_365_317, 3_203_168_211_198_807_973, 9_817_491_932_198_370_423],
            "SplitMix64 with seed 1234567",
        );
        let mut rng = CoreRng::from_state([1, 2, 3, 4]).unwrap();
        let out: Vec<u64> = (0..4).map(|_| rng.next_u64()).collect();
        assert_eq!(
            out,
            [11_520, 0, 1_509_978_240, 1_215_971_899_390_074_240],
            "xoshiro256** from the state 1, 2, 3, 4",
        );
    }

    #[test]
    fn a_seed_replays_and_a_state_resumes() {
        let mut a = CoreRng::from_seed(42);
        let mut b = CoreRng::from_seed(42);
        assert_eq!(a.next_u64(), b.next_u64(), "same seed, same first value");
        let saved = a.state();
        let next = a.next_u64();
        assert_eq!(CoreRng::from_state(saved).unwrap().next_u64(), next, "resumed");
        assert!(CoreRng::from_state([0; 4]).is_none(), "the all-zero state is refused");
    }

    #[test]
    fn a_fork_takes_a_different_stream() {
        let mut parent = CoreRng::from_seed(7);
        let mut child = parent.fork();
        assert_ne!(parent.next_u64(), child.next_u64(), "streams differ after a fork");
    }

    #[test]
    fn fill_bytes_covers_a_partial_word() {
        let mut rng = CoreRng::from_seed(9);
        let mut words = rng.clone();
        let mut out = [0_u8; 11];
        rng.fill_bytes(&mut out);
        // Eight bytes from the first word, then the first three of the second, little-endian.
        let mut expected = words.next_u64().to_le_bytes().to_vec();
        expected.extend_from_slice(&words.next_u64().to_le_bytes()[..3]);
        assert_eq!(out.as_slice(), expected.as_slice(), "the trailing partial word is filled");
        assert_eq!(rng, words, "two words consumed, as the reference took");
        assert!(CoreRng::from_seed(3).ulid_random() < (1_u128 << 80), "80 bits");
    }

    proptest! {
        #[test]
        fn below_stays_in_range(seed: u64, n in 1_u64..) {
            let mut rng = CoreRng::from_seed(seed);
            prop_assert!(rng.below(n) < n, "below n");
        }

        #[test]
        fn range_stays_in_range(seed: u64, lo: u64, width in 0_u64..1_000) {
            let mut rng = CoreRng::from_seed(seed);
            let hi = lo.saturating_add(width);
            let v = rng.range(lo, hi);
            prop_assert!(v == lo || (lo..hi).contains(&v), "in lo..hi or lo when empty");
        }

        #[test]
        fn chance_respects_its_extremes(seed: u64, d in 1_u32..) {
            let mut rng = CoreRng::from_seed(seed);
            prop_assert!(!rng.chance(0, d), "never at zero");
            prop_assert!(rng.chance(d, d), "always at one");
        }
    }
}
