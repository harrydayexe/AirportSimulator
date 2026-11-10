//! The airport state machine (Phase 1).
//!
//! Pure logic: no clock, network, disk, threads or floats. It knows nothing
//! about replicas, the host or browsers.

/// Milliseconds in one hour.
const MS_PER_HOUR: u64 = 3_600_000;

/// Time in whole milliseconds for an aircraft at `speed_kt` knots to cover
/// `distance_nm` nautical miles, rounded to the nearest millisecond.
///
/// Returns `None` for a speed of zero.
#[must_use]
pub fn gap_ms(distance_nm: u32, speed_kt: u32) -> Option<u64> {
    let speed = u64::from(speed_kt);
    if speed == 0 {
        return None;
    }
    let numerator = u64::from(distance_nm) * MS_PER_HOUR;
    Some((numerator + speed / 2) / speed)
}

#[cfg(test)]
mod tests {
    use super::gap_ms;

    #[test]
    fn five_miles_at_140_knots_is_about_129_seconds() {
        assert_eq!(gap_ms(5, 140), Some(128_571));
    }

    #[test]
    fn zero_speed_has_no_gap() {
        assert_eq!(gap_ms(5, 0), None);
    }

    #[test]
    fn zero_distance_is_zero() {
        assert_eq!(gap_ms(0, 140), Some(0));
    }
}
