//! SHA-256 (FIPS 180-4), std only: `fetch-native` checks a download against its pinned hash, and
//! `lint-repo`'s check 6 hashes salted tokens against the denylist. xtask keeps to the three
//! dependencies BUILD.md section 4b.5 allows, so it carries its own.

#![expect(
    clippy::indexing_slicing,
    reason = "numeric kernel; bounds checked at the module boundary (xtask/tidy/numeric-modules.toml)"
)]

use std::fmt::Write as _;
use std::io::{self, Read};

/// The round constants.
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// The initial hash value.
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// An incremental SHA-256.
#[derive(Debug, Clone)]
pub(crate) struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    filled: usize,
    length_bytes: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self { state: H0, block: [0; 64], filled: 0, length_bytes: 0 }
    }
}

impl Sha256 {
    /// Feeds more bytes.
    pub(crate) fn update(&mut self, mut bytes: &[u8]) {
        self.length_bytes =
            self.length_bytes.wrapping_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        while !bytes.is_empty() {
            let take = (64 - self.filled).min(bytes.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&bytes[..take]);
            self.filled += take;
            bytes = &bytes[take..];
            if self.filled == 64 {
                compress(&mut self.state, &self.block);
                self.filled = 0;
            }
        }
    }

    /// The digest of everything fed so far.
    pub(crate) fn finish(mut self) -> [u8; 32] {
        let bits = self.length_bytes.wrapping_mul(8);
        self.block[self.filled] = 0x80;
        self.block[self.filled + 1..].fill(0);
        if self.filled >= 56 {
            compress(&mut self.state, &self.block);
            self.block = [0; 64];
        }
        self.block[56..].copy_from_slice(&bits.to_be_bytes());
        compress(&mut self.state, &self.block);
        let mut out = [0; 32];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *chunk = word.to_be_bytes();
        }
        out
    }
}

/// The digest of `bytes`.
pub(crate) fn digest(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::default();
    hasher.update(bytes);
    hasher.finish()
}

/// The digest of everything `reader` yields.
pub(crate) fn digest_reader(mut reader: impl Read) -> io::Result<[u8; 32]> {
    let mut hasher = Sha256::default();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(hasher.finish());
        }
        hasher.update(&buffer[..read]);
    }
}

/// A digest as 64 lowercase hex digits.
pub(crate) fn hex(digest: &[u8; 32]) -> String {
    digest.iter().fold(String::with_capacity(64), |mut text, byte| {
        // Writing to a String cannot fail.
        let _written: std::fmt::Result = write!(text, "{byte:02x}");
        text
    })
}

#[expect(clippy::many_single_char_names, reason = "the working variables' names in FIPS 180-4")]
fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0_u32; 64];
    for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes(*bytes);
    }
    for t in 16..64 {
        let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
        let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
        w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (k, wt) in K.iter().zip(w) {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(*k).wrapping_add(wt);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (word, add) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *word = word.wrapping_add(add);
    }
}

#[cfg(test)]
mod tests {
    use super::{Sha256, digest, digest_reader, hex};

    #[test]
    fn the_fips_vectors_hash_as_published() {
        assert_eq!(
            hex(&digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "the empty message"
        );
        assert_eq!(
            hex(&digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "one block"
        );
        assert_eq!(
            hex(&digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            "two blocks, the padding in the second"
        );
    }

    #[test]
    fn feeding_in_pieces_gives_the_same_digest() {
        let message = vec![0x61_u8; 1_000];
        let mut pieces = Sha256::default();
        for chunk in message.chunks(37) {
            pieces.update(chunk);
        }
        let whole = digest(&message);
        assert_eq!(pieces.finish(), whole, "37-byte pieces");
        assert_eq!(digest_reader(message.as_slice()).unwrap(), whole, "a reader");
        assert_eq!(
            hex(&digest(&vec![0x61_u8; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
            "a million a's"
        );
    }

    #[test]
    fn every_length_around_a_block_boundary_pads_correctly() {
        // Up to 55 bytes the length fits in the last block; from 56 it needs another. The
        // digests are `sha256sum`'s over the same bytes.
        let expected = [
            (55, "d5e285683cd4efc02d021a5c62014694958901005d6f71e89e0989fac77e4072"),
            (56, "04c26261370ee7541549d16dee320c723e3fd14671e66a099afe0a377c16888e"),
            (63, "75220b47218278e656f2013bb8f0c455a25eaf01e86c64924e9d48d89776d6f2"),
            (64, "7ce100971f64e7001e8fe5a51973ecdfe1ced42befe7ee8d5fd6219506b5393c"),
            (65, "9537c5fdf120482f7d58d25e9ed583f52c02b4e304ea814db1633ad565aed7e9"),
        ];
        for (length, digest_hex) in expected {
            assert_eq!(hex(&digest(&vec![b'x'; length])), digest_hex, "{length} bytes");
        }
    }
}
