//! Defines a small hash function which is used for deriving seeds.
//! This function is based on splitmix64 and is fully deterministic.

/// Golden is roughly 2^64 divided by the golden ratio.
///
/// Multiplying by it spreads bits across the word, so inputs like 1, 2, 3 don't differ only in the
/// low bits.
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// Combines two `u64` values into a single, well-scrambled `u64`.
///
/// Intended for deriving independent, reproducible seeds, e.g.
/// `mix(global_seed, entity_id)`. `b` is multiplied by the 64-bit golden-ratio
/// constant to spread its bits, xor'ed with `a`, then passed through the
/// splitMix64 finaliser so that flipping any input bit changes about half the
/// output bits.
///
/// The result is deterministic and not symmetric: `mix(a, b) != mix(b, a)` in
/// general. It is **not** cryptographically secure.
///
/// # Examples
///
/// ```
/// # fn mix(a: u64, b: u64) -> u64 { a ^ b }
/// assert_eq!(mix(42, 7), mix(42, 7));
/// assert_ne!(mix(42, 7), mix(42, 8));
/// ```
pub fn mix(a: u64, b: u64) -> u64 {
    let mut z = a ^ b.wrapping_mul(GOLDEN);
    z = (z ^ (z >> 30)).wrapping_mul(GOLDEN);
    z = (z ^ (z >> 27)).wrapping_mul(GOLDEN);
    z ^ (z >> 31)
}
