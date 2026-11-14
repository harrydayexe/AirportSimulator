//! Defines a small hash function which is used for deriving seeds.
//! This function is based on `SplitMix64` and is fully deterministic.

/// Golden is roughly 2^64 divided by the golden ratio.
///
/// Multiplying by it spreads bits across the word, so inputs like 1, 2, 3 don't differ only in the
/// low bits.
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// The two multipliers of the `SplitMix64` finaliser.
const MIX_1: u64 = 0xBF58_476D_1CE4_E5B9;
const MIX_2: u64 = 0x94D0_49BB_1331_11EB;

/// Combines two `u64` values into a single, well-scrambled `u64`.
///
/// Intended for deriving independent, reproducible seeds, e.g.
/// `mix(global_seed, entity_id)`. `b` is multiplied by the 64-bit golden-ratio
/// constant to spread its bits, xor'ed with `a`, then passed through the
/// `SplitMix64` finaliser so that flipping any input bit changes about half the
/// output bits.
///
/// The result is deterministic and not symmetric: `mix(a, b) != mix(b, a)` in
/// general. It is **not** cryptographically secure. Its outputs are pinned by
/// the tests below, because every recorded seed depends on them.
pub fn mix(a: u64, b: u64) -> u64 {
    let mut z = a ^ b.wrapping_mul(GOLDEN);
    z = (z ^ (z >> 30)).wrapping_mul(MIX_1);
    z = (z ^ (z >> 27)).wrapping_mul(MIX_2);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_pinned() {
        // mix(0, 1) is the `SplitMix64` finaliser applied to GOLDEN, which is
        // the first output of `SplitMix64` seeded with 0.
        assert_eq!(mix(0, 1), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn mix_is_pinned() {
        // Every recorded seed depends on these.
        assert_eq!(mix(42, 7), 0x53ad_348a_f3dd_af4b);
        assert_eq!(mix(7, 42), 0x820c_624f_7559_8130);
        assert_eq!(mix(42, 8), 0xb434_6c5a_4ac0_89c3);
        assert_eq!(mix(u64::MAX, u64::MAX), 0xe4d9_7177_1b65_2c20);
    }

    #[test]
    fn mix_is_not_symmetric() {
        assert_ne!(mix(42, 7), mix(7, 42));
    }

    #[test]
    fn mix_of_zero_and_zero_is_zero() {
        // Harmless: `Rng::new(0)` is a valid generator.
        assert_eq!(mix(0, 0), 0);
    }

    #[test]
    fn one_flipped_bit_changes_about_half_the_output() {
        // A fair coin per bit gives 32 +- 4. Within five of those either
        // side catches a broken finaliser; the worst today is 49 (bit 45).
        let base = mix(1, 0);
        for bit in 0..64 {
            let flipped = (mix(1, 1 << bit) ^ base).count_ones();
            assert!(
                (12..=52).contains(&flipped),
                "bit {bit} flipped {flipped} output bits"
            );
        }
    }
}
