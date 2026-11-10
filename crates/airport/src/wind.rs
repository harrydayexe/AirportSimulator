//! Wind and its components along and across a runway, in whole knots and
//! whole degrees with a fixed sine table (design section 4).

use serde::{Deserialize, Serialize};

use crate::data::SIN_E4;

/// Wind as reported: the direction it blows from, and its speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Wind {
    /// Degrees true the wind blows from, 0 to 359.
    pub direction_deg: u16,
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
    u64::from(SIN_E4[usize::from(deg.min(90))])
}

/// Components of `wind` for an aircraft using a runway on `heading_deg`.
#[must_use]
pub fn components(wind: Wind, heading_deg: u16) -> Components {
    let speed = u64::from(wind.speed_kt);
    // Angle between the wind and the runway, folded into 0..=180.
    let diff = (wind.direction_deg % 360 + 360 - heading_deg % 360) % 360;
    let angle = if diff > 180 { 360 - diff } else { diff };
    let (cross, head) = if angle <= 90 {
        (sin_e4(angle), sin_e4(90 - angle) as i64)
    } else {
        (sin_e4(180 - angle), -(sin_e4(angle - 90) as i64))
    };
    Components {
        headwind_e4: head * speed as i64,
        crosswind_e4: cross * speed,
    }
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
}
