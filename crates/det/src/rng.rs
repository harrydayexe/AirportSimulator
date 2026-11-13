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
    ///
    /// # Panics
    /// Never, unless the seeding code is broken and produces the all-zero
    /// state, from which xoshiro never leaves.
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
        let s = [next(), next(), next(), next()];
        // The all-zero state is the one xoshiro never leaves. SplitMix64
        // cannot produce it, so this is a check on the code above.
        assert_ne!(s, [0; 4], "Rng::new: all-zero state from seed {seed}");
        Self { s }
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
    ///
    /// # Panics
    /// Never, unless the method itself is broken and the result is not
    /// below `n`.
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
                let result = (m >> 64) as u64;
                assert!(result < n, "Rng::below({n}) gave {result}");
                return result;
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

    #[test]
    fn below_sequence_is_pinned() {
        // Traffic is drawn with `below`, so a change here changes every
        // recorded seed's flights.
        let mut rng = Rng::new(42);
        let values: Vec<u64> = (0..8).map(|_| rng.below(10)).collect();
        assert_eq!(values, [0, 3, 6, 9, 9, 7, 7, 8]);
    }

    #[test]
    fn below_one_is_always_zero() {
        let mut rng = Rng::new(3);
        assert!((0..100).all(|_| rng.below(1) == 0));
    }

    #[test]
    fn below_is_roughly_uniform() {
        // The seed is fixed, so this cannot flake.
        let mut rng = Rng::new(7);
        let mut counts = [0u32; 3];
        for _ in 0..3_000 {
            counts[usize::try_from(rng.below(3)).unwrap()] += 1;
        }
        assert_eq!(counts, [1021, 936, 1043]);
        assert!(counts.iter().all(|c| (900..1100).contains(c)));
    }

    #[test]
    fn a_clone_continues_the_same_sequence() {
        let mut rng = Rng::new(9);
        for _ in 0..5 {
            rng.next_u64();
        }
        let mut copy = rng.clone();
        let a: Vec<u64> = (0..5).map(|_| rng.next_u64()).collect();
        let b: Vec<u64> = (0..5).map(|_| copy.next_u64()).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_give_different_sequences() {
        let firsts: std::collections::BTreeSet<u64> =
            (0..100).map(|seed| Rng::new(seed).next_u64()).collect();
        assert_eq!(firsts.len(), 100);
    }
}
