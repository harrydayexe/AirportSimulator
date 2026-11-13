//! Wind and its components along and across a runway, in whole knots and
//! whole degrees with a fixed sine table (design section 4).

use serde::{Deserialize, Serialize};

use crate::data::SIN_E4;

/// Wind as reported: the direction it blows from, and its speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Wind {
    /// Degrees true the wind blows from, 0 to 359.
    pub direction_deg: u16,
    /// Speed in knots.
    pub speed_kt: u16,
}

/// Wind components for one runway direction, in knots times 10 000.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Components {
    /// Positive is a headwind, negative a tailwind.
    pub headwind_e4: i64,
    /// Always positive.
    pub crosswind_e4: u64,
}

fn sin_e4(deg: u16) -> u64 {
    // W1: callers fold the angle into 0..=90 first. Clamping instead would
    // turn a geometry bug into a wrong but plausible crosswind.
    assert!(deg <= 90, "sin_e4({deg}): angle outside 0..=90");
    u64::from(SIN_E4[usize::from(deg)])
}

/// Components of `wind` for an aircraft using a runway on `heading_deg`.
///
/// # Panics
/// Never, unless the angle folding below is broken.
#[must_use]
pub fn components(wind: Wind, heading_deg: u16) -> Components {
    let speed = u64::from(wind.speed_kt);
    // Angle between the wind and the runway, folded into 0..=180.
    let diff = (wind.direction_deg % 360 + 360 - heading_deg % 360) % 360;
    let angle = if diff > 180 { 360 - diff } else { diff };
    let (cross, head) = if angle <= 90 {
        (sin_e4(angle), sin_e4(90 - angle).cast_signed())
    } else {
        (sin_e4(180 - angle), -sin_e4(angle - 90).cast_signed())
    };
    let result = Components {
        headwind_e4: head * i64::from(wind.speed_kt),
        crosswind_e4: cross * speed,
    };
    // W2: neither component is more than the wind itself.
    assert!(result.crosswind_e4 <= speed * 10_000);
    assert!(result.headwind_e4.unsigned_abs() <= speed * 10_000);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wind(direction_deg: u16, speed_kt: u16) -> Wind {
        Wind {
            direction_deg,
            speed_kt,
        }
    }

    #[test]
    fn wind_thirty_degrees_off_the_runway() {
        // Runway 09, wind 120 at 20 kt: headwind 20 cos 30 = 17.32 kt,
        // crosswind 20 sin 30 = 10 kt.
        let c = components(wind(120, 20), 90);
        assert_eq!(c.headwind_e4, 173_200);
        assert_eq!(c.crosswind_e4, 100_000);
    }

    #[test]
    fn straight_down_the_runway_and_straight_across() {
        assert_eq!(components(wind(270, 15), 270).headwind_e4, 150_000);
        assert_eq!(components(wind(270, 15), 270).crosswind_e4, 0);
        assert_eq!(components(wind(90, 15), 270).headwind_e4, -150_000);
        assert_eq!(components(wind(0, 15), 270).headwind_e4, 0);
        assert_eq!(components(wind(0, 15), 270).crosswind_e4, 150_000);
    }

    #[test]
    fn wraps_around_north() {
        // Runway 36 (heading 360), wind 010 and wind 350 are both 10 degrees off.
        let a = components(wind(10, 10), 360);
        let b = components(wind(350, 10), 0);
        assert_eq!(a, b);
        assert_eq!(a.crosswind_e4, 17_360);
    }

    #[test]
    fn forty_five_degrees_splits_the_wind_evenly() {
        let c = components(wind(45, 20), 90);
        assert_eq!(c.headwind_e4, 141_420);
        assert_eq!(c.crosswind_e4, 141_420);
    }

    #[test]
    fn a_wind_from_behind_is_a_negative_headwind() {
        // 150 degrees off: headwind -10 cos 30, crosswind 10 sin 30.
        let c = components(wind(240, 10), 90);
        assert_eq!(c.headwind_e4, -86_600);
        assert_eq!(c.crosswind_e4, 50_000);
    }

    #[test]
    fn the_opposite_end_sees_the_same_crosswind_and_the_opposite_headwind() {
        for direction in 0..360 {
            for heading in (0..360).step_by(5) {
                let w = wind(direction, 17);
                let a = components(w, heading);
                let b = components(w, (heading + 180) % 360);
                assert_eq!(a.crosswind_e4, b.crosswind_e4, "{direction} on {heading}");
                assert_eq!(a.headwind_e4, -b.headwind_e4, "{direction} on {heading}");
                assert!(a.crosswind_e4 <= 170_000);
                assert!(a.headwind_e4.unsigned_abs() <= 170_000);
            }
        }
    }

    #[test]
    fn the_strongest_wind_does_not_overflow() {
        let c = components(wind(45, u16::MAX), 90);
        assert_eq!(c.headwind_e4, 463_397_985);
        assert_eq!(c.crosswind_e4, 463_397_985);
    }

    #[test]
    fn calm_has_no_components() {
        for direction in [0, 90, 181, 359] {
            let c = components(wind(direction, 0), 90);
            assert_eq!((c.headwind_e4, c.crosswind_e4), (0, 0));
        }
    }
}
