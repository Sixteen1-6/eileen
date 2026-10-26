//! The trace hash `--doublecheck` compares: FNV-1a over 64 bits, the same on every OS and in
//! every build, unlike std's `DefaultHasher`, whose algorithm may change between releases.

/// The FNV-1a 64-bit offset basis.
const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// The FNV-1a 64-bit prime.
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// An FNV-1a 64-bit hasher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceHash(u64);

impl Default for TraceHash {
    fn default() -> Self {
        Self(OFFSET)
    }
}

impl TraceHash {
    /// Feeds `bytes`.
    pub fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(PRIME);
        }
    }

    /// Feeds `bytes` followed by a separator, so `["ab", "c"]` and `["a", "bc"]` differ.
    pub fn record(&mut self, bytes: &[u8]) {
        self.update(bytes);
        self.update(&[0xff]);
    }

    /// The hash so far.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::TraceHash;

    #[test]
    fn the_published_vectors_hash_as_expected() {
        let hash = |text: &[u8]| {
            let mut hasher = TraceHash::default();
            hasher.update(text);
            hasher.value()
        };
        assert_eq!(hash(b""), 0xcbf2_9ce4_8422_2325, "the empty input is the offset basis");
        assert_eq!(hash(b"a"), 0xaf63_dc4c_8601_ec8c, "FNV-1a 64 of `a`");
        assert_eq!(hash(b"foobar"), 0x8594_4171_f739_67e8, "FNV-1a 64 of `foobar`");
    }

    #[test]
    fn records_are_separated() {
        let mut one = TraceHash::default();
        one.record(b"ab");
        one.record(b"c");
        let mut two = TraceHash::default();
        two.record(b"a");
        two.record(b"bc");
        assert_ne!(one.value(), two.value(), "the separator keeps the boundaries");
    }
}
