//! Contains the definition of [`Clock`], the one true time.
use det::Ms;

/// The one simulated time. Only the top-levl host owns a `Clock`.
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
