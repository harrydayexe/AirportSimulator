//! Turning the separation tables into time gaps at the runway threshold.

use crate::data::{AircraftType, approach_separation_nm};
use crate::{MS_PER_HOUR, Ms};

/// Time in whole milliseconds for an aircraft at `speed_kt` knots to cover
/// `distance_nm` nautical miles, rounded up so that a gap is never shorter
/// than the rule requires.
///
/// Returns `None` for a speed of zero.
#[must_use]
pub fn gap_ms(distance_nm: u32, speed_kt: u32) -> Option<Ms> {
    let speed = u64::from(speed_kt);
    if speed == 0 {
        return None;
    }
    Some((u64::from(distance_nm) * MS_PER_HOUR).div_ceil(speed))
}

/// Required time between two arrivals crossing the same runway threshold.
///
/// The separation in miles must hold along the whole final approach, which
/// is `final_approach_nm` long.
///
/// - If the follower is at least as fast as the leader, the two are closest
///   when the leader reaches the threshold, so the gap is the separation
///   flown at the follower's approach speed.
/// - If the follower is slower, the two are closest when the leader starts
///   its final approach and the gap opens up from there. The follower needs
///   the time to fly the approach plus the separation, less the time the
///   leader took to fly the approach.
///
/// # Panics
/// If either approach speed is zero (`data.rs` checks this at build time).
#[must_use]
pub fn approach_gap_ms(
    leader: &AircraftType,
    follower: &AircraftType,
    final_approach_nm: u32,
) -> Ms {
    let sep = u64::from(approach_separation_nm(leader.wake, follower.wake));
    let path = u64::from(final_approach_nm);
    let vl = u64::from(leader.approach_speed_kt);
    let vf = u64::from(follower.approach_speed_kt);
    // P-S1: also a build-time check on the table in `data.rs`.
    assert!(vl > 0 && vf > 0, "approach speed of zero");
    let plain = (sep * MS_PER_HOUR).div_ceil(vf);
    let gap = if vf >= vl {
        plain
    } else {
        // (path + sep) / vf - path / vl, over a common denominator.
        (((path + sep) * vl - path * vf) * MS_PER_HOUR).div_ceil(vf * vl)
    };
    // P-S2: a slow follower never gets a shorter gap than the plain one.
    assert!(
        gap >= plain && gap > 0,
        "approach gap {gap} ms below the plain {plain} ms"
    );
    gap
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{aircraft, aircraft_index};

    fn ty(icao: &str) -> &'static AircraftType {
        aircraft(aircraft_index(icao).unwrap())
    }

    #[test]
    fn five_miles_at_140_knots_is_about_129_seconds() {
        // 5 / 140 h = 128 571.43 ms, rounded up.
        assert_eq!(gap_ms(5, 140), Some(128_572));
    }

    #[test]
    fn zero_speed_has_no_gap() {
        assert_eq!(gap_ms(5, 0), None);
    }

    #[test]
    fn zero_distance_is_zero() {
        assert_eq!(gap_ms(0, 140), Some(0));
    }

    #[test]
    fn same_type_gap_is_separation_at_approach_speed() {
        // A320 behind A320: 3 nm at 137 kt = 78 832.1 ms.
        assert_eq!(approach_gap_ms(ty("A320"), ty("A320"), 6), 78_833);
        // B77W behind B77W: 4 nm at 149 kt = 96 644.3 ms.
        assert_eq!(approach_gap_ms(ty("B77W"), ty("B77W"), 6), 96_645);
    }

    #[test]
    fn light_behind_heavy_needs_six_miles() {
        // PC12 (85 kt) behind B77W (149 kt), 6 nm final: the slow follower
        // case. (6 + 6) / 85 h - 6 / 149 h = 508 235.29 - 144 966.44 ms.
        assert_eq!(approach_gap_ms(ty("B77W"), ty("PC12"), 6), 363_269);
    }

    #[test]
    fn slow_behind_fast_opens_the_gap() {
        // AT76 (113 kt) behind B738 (144 kt), both medium, 3 nm.
        // Closest at the start of a 6 nm final:
        // 9 / 113 h - 6 / 144 h = 286 725.66 - 150 000 ms.
        assert_eq!(approach_gap_ms(ty("B738"), ty("AT76"), 6), 136_726);
        // The other way round the gap is just 3 nm at 144 kt = 75 000 ms.
        assert_eq!(approach_gap_ms(ty("AT76"), ty("B738"), 6), 75_000);
        // So fast-then-slow costs more than the same pair in the other order.
        assert!(
            approach_gap_ms(ty("B738"), ty("AT76"), 6) > gap_ms(3, 113).unwrap(),
            "the gap at the threshold is wider than 3 nm at the follower's speed"
        );
    }

    #[test]
    fn equal_speeds_use_the_simple_gap() {
        // B788 behind B763, both 140 kt, heavy behind heavy: 4 nm.
        // 4 / 140 h = 102 857.1 ms.
        assert_eq!(approach_gap_ms(ty("B763"), ty("B788"), 6), 102_858);
    }

    #[test]
    fn with_no_final_approach_a_slow_follower_needs_only_the_plain_gap() {
        // 3 / 113 h = 95 575.2 ms.
        assert_eq!(approach_gap_ms(ty("B738"), ty("AT76"), 0), 95_576);
        assert_eq!(gap_ms(3, 113), Some(95_576));
    }

    #[test]
    fn the_slowest_behind_the_biggest() {
        // C172 (65 kt) behind A388 (138 kt): 8 nm on a 6 nm final.
        // 14 / 65 h - 6 / 138 h = 775 384.6 - 156 521.7 ms.
        assert_eq!(approach_gap_ms(ty("A388"), ty("C172"), 6), 618_863);
    }

    #[test]
    fn a_longer_final_widens_the_gap_only_for_a_slower_follower() {
        let slow: Vec<Ms> = [3, 6, 12]
            .iter()
            .map(|f| approach_gap_ms(ty("B738"), ty("AT76"), *f))
            .collect();
        assert!(slow.windows(2).all(|w| w[0] < w[1]), "{slow:?}");
        for f in [3, 6, 12] {
            assert_eq!(approach_gap_ms(ty("AT76"), ty("B738"), f), 75_000);
        }
    }

    #[test]
    fn no_gap_is_shorter_than_the_separation_at_the_followers_speed() {
        use crate::data::AIRCRAFT_TYPES;
        for leader in &AIRCRAFT_TYPES {
            for follower in &AIRCRAFT_TYPES {
                let sep = approach_separation_nm(leader.wake, follower.wake);
                let plain = gap_ms(sep, follower.approach_speed_kt).unwrap();
                for f in 0..=15 {
                    let gap = approach_gap_ms(leader, follower, f);
                    assert!(
                        gap >= plain && gap > 0,
                        "{} behind {}",
                        follower.icao,
                        leader.icao
                    );
                }
            }
        }
    }

    #[test]
    fn gap_ms_rounds_up_and_does_not_overflow() {
        // 1 / 7 h = 514 285.7 ms; 3 / 144 h is exactly 75 000 ms.
        assert_eq!(gap_ms(1, 7), Some(514_286));
        assert_eq!(gap_ms(3, 144), Some(75_000));
        assert_eq!(gap_ms(u32::MAX, 1), Some(u64::from(u32::MAX) * MS_PER_HOUR));
    }
}
