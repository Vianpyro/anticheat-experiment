//! SHA-256, written out rather than depended upon.
//!
//! `sim` depends on nothing (`docs/ARCHITECTURE.md`), and this is one of the
//! two places that policy costs something. It is worth paying here: the hash is
//! the comparison primitive the determinism suite, the replay container's
//! `rules_hash` and eventually the signed manifest all rest on, so it must
//! produce the same 32 bytes on every platform for the next several years.
//! FIPS 180-4 is frozen and the known-answer tests below pin the
//! implementation to it, which is a stronger guarantee than a version range.
//!
//! This is a hash, not cryptography the project is relying on for secrecy: no
//! secret is ever fed to it. When `replay` needs signatures at M5 it will take
//! a real, audited crate for that, because hand-rolled signature code is the
//! failure mode a security portfolio cannot afford (`docs/RISKS.md` R6 makes
//! the same argument for the transport).

use core::fmt;

/// A 32-byte digest. The only way to compare two [`crate::State`] values.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Digest([u8; 32]);

impl Digest {
    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Wraps raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parses 64 lowercase or uppercase hexadecimal characters.
    ///
    /// This exists so that a golden digest can be written into a test as the
    /// string it is printed as, and diffed by a human when it changes.
    #[must_use]
    pub fn from_hex(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (index, slot) in out.iter_mut().enumerate() {
            let high = hex_value(*bytes.get(index.saturating_mul(2))?)?;
            let low = hex_value(*bytes.get(index.saturating_mul(2).saturating_add(1))?)?;
            *slot = (high << 4) | low;
        }
        Some(Self(out))
    }
}

const fn hex_value(character: u8) -> Option<u8> {
    match character {
        b'0'..=b'9' => Some(character.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(character.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(character.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Printed as hex, because a test failure showing 32 decimal numbers is a test
/// failure nobody reads.
impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

const BLOCK_LEN: usize = 64;

const INITIAL: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

#[rustfmt::skip]
const ROUND_CONSTANTS: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5,
    0x3956_c25b, 0x59f1_11f1, 0x923f_82a4, 0xab1c_5ed5,
    0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3,
    0x72be_5d74, 0x80de_b1fe, 0x9bdc_06a7, 0xc19b_f174,
    0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc,
    0x2de9_2c6f, 0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da,
    0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967,
    0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc, 0x5338_0d13,
    0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85,
    0xa2bf_e8a1, 0xa81a_664b, 0xc24b_8b70, 0xc76c_51a3,
    0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070,
    0x19a4_c116, 0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5,
    0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208,
    0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7, 0xc671_78f2,
];

/// An incremental SHA-256.
///
/// Crate-private: everything outside `sim` that needs a digest gets one from
/// [`crate::State::digest`] or [`crate::rules_hash`]. Handing out a general
/// hasher would invite a second, differently-ordered encoding of the same state
/// to appear somewhere else, and two encodings is one more than the number that
/// can be authoritative.
pub(crate) struct Hasher {
    state: [u32; 8],
    buffer: [u8; BLOCK_LEN],
    buffered: usize,
    total_len: u64,
}

impl Hasher {
    pub(crate) const fn new() -> Self {
        Self {
            state: INITIAL,
            buffer: [0u8; BLOCK_LEN],
            buffered: 0,
            total_len: 0,
        }
    }

    pub(crate) fn update(&mut self, mut bytes: &[u8]) {
        self.total_len = self.total_len.wrapping_add(bytes.len() as u64);
        while !bytes.is_empty() {
            let free = BLOCK_LEN.saturating_sub(self.buffered);
            let taken = free.min(bytes.len());
            let (head, tail) = bytes.split_at(taken);
            let end = self.buffered.saturating_add(taken);
            if let Some(slot) = self.buffer.get_mut(self.buffered..end) {
                slot.copy_from_slice(head);
            }
            self.buffered = end;
            if self.buffered == BLOCK_LEN {
                let block = self.buffer;
                compress(&mut self.state, &block);
                self.buffered = 0;
            }
            bytes = tail;
        }
    }

    pub(crate) fn finish(mut self) -> Digest {
        let bit_len = self.total_len.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buffered != BLOCK_LEN.saturating_sub(8) {
            self.update(&[0x00]);
        }
        // `update` has already folded these into `total_len`, which is why the
        // length was captured before the padding rather than after it.
        self.update(&bit_len.to_be_bytes());

        let mut out = [0u8; 32];
        for (word, slot) in self.state.iter().zip(out.chunks_exact_mut(4)) {
            slot.copy_from_slice(&word.to_be_bytes());
        }
        Digest(out)
    }
}

fn compress(state: &mut [u32; 8], block: &[u8; BLOCK_LEN]) {
    let mut schedule = [0u32; 64];
    for (word, chunk) in schedule.iter_mut().zip(block.chunks_exact(4)) {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(chunk);
        *word = u32::from_be_bytes(bytes);
    }
    // The message schedule indexes backwards from `index`, which is at least
    // 16, so every `wrapping_sub` below is an ordinary subtraction. It is
    // written in wrapping form because `sim` denies bare arithmetic, and
    // rewriting the algorithm to avoid the indices would make it harder to
    // check against FIPS 180-4 than the noise is worth.
    for index in 16usize..64 {
        let a = schedule[index.wrapping_sub(15)];
        let b = schedule[index.wrapping_sub(2)];
        let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
        let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
        schedule[index] = schedule[index.wrapping_sub(16)]
            .wrapping_add(s0)
            .wrapping_add(schedule[index.wrapping_sub(7)])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (word, constant) in schedule.iter().zip(ROUND_CONSTANTS.iter()) {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choose = (e & f) ^ (!e & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(choose)
            .wrapping_add(*constant)
            .wrapping_add(*word);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(majority);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    let round = [a, b, c, d, e, f, g, h];
    for (slot, value) in state.iter_mut().zip(round.iter()) {
        *slot = slot.wrapping_add(*value);
    }
}

#[cfg(test)]
mod tests {
    use super::{Digest, Hasher};

    fn digest_of(bytes: &[u8]) -> String {
        let mut hasher = Hasher::new();
        hasher.update(bytes);
        hasher.finish().to_string()
    }

    /// Known answers from FIPS 180-4 and its published test vectors. These are
    /// what make the hand-written implementation trustworthy; without them it
    /// is only self-consistent, which every wrong implementation also is.
    #[test]
    fn matches_the_published_vectors() {
        assert_eq!(
            digest_of(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            digest_of(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            digest_of(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // One million 'a', which exercises many blocks and the length field.
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            digest_of(&million),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    /// The buffering path must not depend on how the input is split.
    #[test]
    fn is_independent_of_chunking() {
        let data: Vec<u8> = (0..1000u32).map(|value| value as u8).collect();
        let whole = {
            let mut hasher = Hasher::new();
            hasher.update(&data);
            hasher.finish()
        };
        for chunk in [1usize, 7, 63, 64, 65, 128] {
            let mut hasher = Hasher::new();
            for part in data.chunks(chunk) {
                hasher.update(part);
            }
            assert_eq!(hasher.finish(), whole, "chunked by {chunk}");
        }
    }

    #[test]
    fn hex_round_trips() {
        let mut hasher = Hasher::new();
        hasher.update(b"round trip");
        let digest = hasher.finish();
        assert_eq!(Digest::from_hex(&digest.to_string()), Some(digest));
        assert_eq!(Digest::from_hex("nonsense"), None);
        assert_eq!(Digest::from_hex(&"z".repeat(64)), None);
    }
}
