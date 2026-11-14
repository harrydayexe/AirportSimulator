//! Seq is an ordered, sequence identifier, which only ever increases monotonically.

/// The sequence identifier for an object. [`Seq`] can be compared to find the order of
/// construction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seq(u64);

impl Seq {
    /// The `Seq` after this one.
    ///
    /// # Panics
    /// If the counter overflows.
    #[must_use]
    pub fn next(&self) -> Self {
        Self(self.0.checked_add(1).expect("Seq overflow"))
    }
}
