//! The one seeded random number generator (design section 4). The algorithm
//! is written out here, xoshiro256** seeded through `SplitMix64`, so that no
//! library update can change the sequence a seed produces.

/// A seeded generator. The same seed always gives the same sequence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// A generator whose sequence is fixed by `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut x = seed;
        let mut next = || {
            x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        };
        Self {
            s: [next(), next(), next(), next()],
        }
    }

    /// The next value in the sequence, uniform over all of `u64`.
    pub fn next_u64(&mut self) -> u64 {
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

    /// A uniform value in `0..n`, without bias. Returns 0 when `n` is 0.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Lemire's method compares the low 64 bits of the product"
    )]
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        // Lemire's method: multiply into 128 bits and reject the few values
        // that would make some outcomes more likely than others.
        let threshold = n.wrapping_neg() % n;
        loop {
            let m = u128::from(self.next_u64()) * u128::from(n);
            if (m as u64) >= threshold {
                return (m >> 64) as u64;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn sequence_is_pinned() {
        // Reference values for xoshiro256** seeded with SplitMix64(0).
        let mut rng = Rng::new(0);
        assert_eq!(rng.next_u64(), 0x99ec_5f36_cb75_f2b4);
        assert_eq!(rng.next_u64(), 0xbf6e_1f78_4956_452a);
        assert_eq!(rng.next_u64(), 0x1a5f_849d_4933_e6e0);
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = Rng::new(42);
        for n in [1, 2, 3, 10, 1000, u64::MAX] {
            for _ in 0..200 {
                assert!(rng.below(n) < n);
            }
        }
        assert_eq!(rng.below(0), 0);
    }
}
