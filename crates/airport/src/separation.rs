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
}
