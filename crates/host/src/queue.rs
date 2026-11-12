//! The [`EventQueue`] holds events ordered by time, then by the order they were added.

use std::collections::{BTreeMap, BTreeSet};

use det::Ms;

/// The order an event was added in, within one queue. Breaks ties between events at the same time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seq(u64);

impl Seq {
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

/// Events ordered by time, then by the order they were added.
///
/// The tie rule is part of the key, note a side effect of the container.
#[derive(Debug)]
pub struct EventQueue<E> {
    /// A tree of events, sorted by time and then [`Seq`].
    events: BTreeMap<(Ms, Seq), E>,
    /// A set of the sequences in `events`.
    seqs: BTreeSet<Seq>,
    /// The next [`Seq`] to use.
    next_seq: Seq,
    /// The time of the last `pop`.
    floor: Option<Ms>,
}

impl<E> Default for EventQueue<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E> EventQueue<E> {
    fn max_seq(&self) -> Seq {
        if let Some(seq) = self.seqs.last() {
            *seq
        } else {
            Seq::default()
        }
    }

    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self {
            events: BTreeMap::new(),
            seqs: BTreeSet::new(),
            next_seq: Seq(0),
            floor: None,
        }
    }

    /// Insert a new event into the queue.
    ///
    /// # Arguments:
    ///
    /// * `at` - the timestamp of the insert.
    /// * `event` - the event to be inserted.
    ///
    /// # Panics
    ///
    /// If the next [`Seq`] is not greater than the biggest [`Seq`] in the queue, this is a bug.
    pub fn push(&mut self, at: Ms, event: E) -> Seq {
        let s = self.next_seq;
        assert!(
            s > self.max_seq(),
            "EventQueue: Next `seq` is not greater than largest `seq` in queue"
        );

        self.events.insert((at, s), event);
        self.seqs.insert(s);
        self.next_seq = s.next();
        s
    }

    /// Peek at the key of the first event in the queue
    #[must_use]
    pub fn peek(&self) -> Option<(Ms, Seq)> {
        if let Some(((at, seq), _)) = self.events.first_key_value() {
            Some((*at, *seq))
        } else {
            None
        }
    }

    /// Remove and return the earliest event.
    ///
    /// # Panics
    /// If peek has found a key, and pop subsequently fails to, there is a bug in the program as
    /// peek and pop should always return the same thing.
    pub fn pop(&mut self) -> Option<(Ms, E)> {
        let peeked = self.peek();
        match peeked {
            Some((at, seq)) => {
                let popped = self.events.pop_first();
                assert!(popped.is_some());

                let ((at_pop, seq_pop), event) = popped.unwrap();
                assert_eq!(at, at_pop);
                assert_eq!(seq, seq_pop);

                Some((at, event))
            }
            None => None,
        }
    }
}
