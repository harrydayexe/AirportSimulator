//! Where a production node keeps what must survive a crash.
//!
//! [`ProdHost`](crate::prodhost::ProdHost) writes through a [`Storage`]
//! before it lets any effect of a step leave the machine. This crate only
//! has the in-memory [`MemStorage`] for tests; the file-backed one lives in
//! the `airport-server` crate, because this crate does no real I/O.

use std::convert::Infallible;
use std::fmt::Display;

use det::{Durable, Entry, Index, NodeId, Term};

/// A disk for one node's [`Durable`] state. Every write must be safe against
/// a crash (flushed all the way down) by the time it returns.
pub trait Storage<C> {
    /// Why a read or write failed.
    type Error: Display;

    /// Reads back everything written so far. A store never written to gives
    /// the state of a brand new node: term 0, no vote, an empty log.
    ///
    /// # Errors
    /// If the store cannot be read or what it holds is damaged.
    fn load(&mut self) -> Result<Durable<C>, Self::Error>;

    /// Records the node's term and its vote in that term.
    ///
    /// # Errors
    /// If the write did not reach the disk.
    fn save_vote(&mut self, term: Term, voted_for: Option<NodeId>) -> Result<(), Self::Error>;

    /// Cuts the stored log back to its first `keep` entries, then adds
    /// `entries` after them.
    ///
    /// # Errors
    /// If the write did not reach the disk.
    fn write_log(&mut self, keep: Index, entries: &[Entry<C>]) -> Result<(), Self::Error>;
}

/// A [`Storage`] that only remembers, for tests. It never fails.
#[derive(Debug)]
pub struct MemStorage<C> {
    /// The stored term.
    term: Term,
    /// The stored vote.
    voted_for: Option<NodeId>,
    /// The stored log.
    log: Vec<Entry<C>>,
    /// How many writes have been made, so tests can see when they happen.
    writes: u64,
}

impl<C> Default for MemStorage<C> {
    fn default() -> Self {
        Self {
            term: Term(0),
            voted_for: None,
            log: Vec::new(),
            writes: 0,
        }
    }
}

impl<C> MemStorage<C> {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The stored term and vote.
    #[must_use]
    pub fn vote(&self) -> (Term, Option<NodeId>) {
        (self.term, self.voted_for)
    }

    /// The stored log.
    #[must_use]
    pub fn log(&self) -> &[Entry<C>] {
        &self.log
    }

    /// How many writes have been made.
    #[must_use]
    pub fn writes(&self) -> u64 {
        self.writes
    }
}

impl<C: Clone> Storage<C> for MemStorage<C> {
    type Error = Infallible;

    fn load(&mut self) -> Result<Durable<C>, Infallible> {
        Ok(Durable::new(self.term, self.voted_for, self.log.clone()))
    }

    fn save_vote(&mut self, term: Term, voted_for: Option<NodeId>) -> Result<(), Infallible> {
        self.term = term;
        self.voted_for = voted_for;
        self.writes += 1;
        Ok(())
    }

    fn write_log(&mut self, keep: Index, entries: &[Entry<C>]) -> Result<(), Infallible> {
        let keep = usize::try_from(keep.0).expect("log position fits in usize");
        assert!(
            keep <= self.log.len(),
            "MemStorage: keeping {keep} entries of {}",
            self.log.len()
        );
        self.log.truncate(keep);
        self.log.extend_from_slice(entries);
        self.writes += 1;
        Ok(())
    }
}
