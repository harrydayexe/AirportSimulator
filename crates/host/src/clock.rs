//! Contains the definition of [`Clock`], the one true time.
use det::Ms;

/// The one simulated time, owned by the host. Nodes never see it: each
/// `step` is told the time.
#[derive(Debug, Default)]
pub struct Clock {
    /// The current time.
    now: Ms,
}

impl Clock {
    /// The current simulated time.
    #[must_use]
    pub fn now(&self) -> Ms {
        self.now
    }

    /// Advance the clock to the time specified.
    ///
    /// Arguments:
    ///
    /// * `to` - The target timestamp.
    ///
    /// # Panics
    ///
    /// Time never goes backwards: if `to` is less than the current time, a bug has occured.
    pub fn advance_to(&mut self, to: Ms) {
        assert!(
            to >= self.now,
            "Clock: asked to go back from {} ms to {to} ms",
            self.now
        );
        self.now = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_clock_reads_zero() {
        assert_eq!(Clock::default().now(), Ms(0));
    }

    #[test]
    fn the_clock_moves_forward_and_may_stand_still() {
        let mut clock = Clock::default();
        let mut seen = Vec::new();
        for to in [10, 10, 25] {
            clock.advance_to(Ms(to));
            seen.push(clock.now());
        }
        assert_eq!(seen, [Ms(10), Ms(10), Ms(25)]);
    }

    #[test]
    #[should_panic(expected = "go back")]
    fn time_never_goes_backwards() {
        let mut clock = Clock::default();
        clock.advance_to(Ms(10));
        clock.advance_to(Ms(5));
    }
}
