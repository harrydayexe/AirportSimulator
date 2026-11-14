//! The state fingerprint: a short hash of an entire state, used to check that
//! two copies given the same commands are in the same state.

use std::hash::{Hash, Hasher};

// `write_usize` widens to `u64`; that loses nothing only if a `usize` fits.
const _: () = assert!(size_of::<usize>() <= 8);

/// 64-bit FNV-1a, with every integer written as a fixed-width little-endian
/// value so that a 32-bit browser and a 64-bit laptop agree.
pub struct Fnv1a(u64);

impl Fnv1a {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    /// A hasher in the standard FNV-1a starting state.
    #[must_use]
    pub const fn new() -> Self {
        Self(Self::OFFSET)
    }
}

impl Default for Fnv1a {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for Fnv1a {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(Self::PRIME);
        }
    }

    fn write_u16(&mut self, i: u16) {
        self.write(&i.to_le_bytes());
    }
    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }
    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }
    fn write_u128(&mut self, i: u128) {
        self.write(&i.to_le_bytes());
    }
    // Lengths and enum discriminants arrive as `usize`/`isize`, whose width
    // depends on the machine. Widen them so every machine hashes eight bytes.
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
    fn write_i16(&mut self, i: i16) {
        self.write(&i.to_le_bytes());
    }
    fn write_i32(&mut self, i: i32) {
        self.write(&i.to_le_bytes());
    }
    fn write_i64(&mut self, i: i64) {
        self.write(&i.to_le_bytes());
    }
    fn write_i128(&mut self, i: i128) {
        self.write(&i.to_le_bytes());
    }
    fn write_isize(&mut self, i: isize) {
        self.write_i64(i as i64);
    }
}

/// Fingerprint of any hashable value.
#[must_use]
pub fn fingerprint<T: Hash>(value: &T) -> u64 {
    let mut hasher = Fnv1a::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_published_fnv1a_vectors() {
        let mut h = Fnv1a::new();
        h.write(b"");
        assert_eq!(h.finish(), 0xcbf2_9ce4_8422_2325);
        let mut h = Fnv1a::new();
        h.write(b"a");
        assert_eq!(h.finish(), 0xaf63_dc4c_8601_ec8c);
        let mut h = Fnv1a::new();
        h.write(b"foobar");
        assert_eq!(h.finish(), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn usize_hashes_like_u64() {
        assert_eq!(fingerprint(&7usize), fingerprint(&7u64));
    }

    #[test]
    fn integers_hash_at_their_own_width() {
        assert_eq!(fingerprint(&1u32), 0xad2a_ca77_4798_5764);
        assert_ne!(fingerprint(&1u32), fingerprint(&1u64));
    }

    #[test]
    fn a_tuple_hashes_its_fields_in_order() {
        // FNV-1a of the bytes 01 02 00.
        assert_eq!(fingerprint(&(1u8, 2u16)), 0xd0aa_6318_672c_f75e);
    }

    #[test]
    fn isize_hashes_like_i64() {
        assert_eq!(fingerprint(&-1isize), 0x8cf5_1a8b_fca3_883d);
        assert_eq!(fingerprint(&-1isize), fingerprint(&-1i64));
    }

    #[test]
    fn a_vec_hashes_its_length_as_eight_bytes() {
        // This is the value a 32-bit machine would get wrong if
        // `write_usize` were ever removed.
        assert_eq!(fingerprint(&vec![1u8, 2]), 0x9b19_e1d3_2794_4d90);
    }
}
