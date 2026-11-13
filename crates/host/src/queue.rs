//! The [`EventQueue`] holds events ordered by time, then by the order they were added.

use std::collections::BTreeMap;

use det::Ms;

/// The order an event was added in, within one queue. Breaks ties between events at the same time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seq(u64);

impl Seq {
    /// The `Seq` after this one.
    ///
    /// # Panics
    /// If the counter overflows (Q5).
    #[must_use]
    pub fn next(&self) -> Self {
        Self(self.0.checked_add(1).expect("EventQueue: Seq overflow"))
    }
}

/// Events ordered by time, then by the order they were added.
///
/// The tie rule is part of the key, not a side effect of the container.
#[derive(Debug)]
pub struct EventQueue<E> {
    /// A tree of events, sorted by time and then [`Seq`].
    events: BTreeMap<(Ms, Seq), E>,
    /// The next [`Seq`] to use. Only ever counts up, so every key is unique.
    next_seq: Seq,
    /// The key of the last `pop`. Nothing may be pushed before its time, and
    /// popped keys are strictly increasing.
    floor: Option<(Ms, Seq)>,
}

impl<E> Default for EventQueue<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E> EventQueue<E> {
    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self {
            events: BTreeMap::new(),
            next_seq: Seq(0),
            floor: None,
        }
    }

    /// Insert a new event into the queue.
    ///
    /// # Arguments:
    ///
    /// * `at` - the time the event is due.
    /// * `event` - the event to be inserted.
    ///
    /// # Panics
    ///
    /// If `at` is before the time of the last `pop` (nothing may be scheduled in the past), or
    /// the key is already present. Both are bugs.
    pub fn push(&mut self, at: Ms, event: E) -> Seq {
        if let Some((floor, _)) = self.floor {
            assert!(
                at >= floor,
                "EventQueue: event pushed at {at} ms, before the last pop at {floor} ms"
            );
        }
        let s = self.next_seq;
        assert!(
            self.events.insert((at, s), event).is_none(),
            "EventQueue: key ({at}, {s:?}) pushed twice"
        );
        self.next_seq = s.next();
        s
    }

    /// Peek at the key of the first event in the queue
    #[must_use]
    pub fn peek(&self) -> Option<(Ms, Seq)> {
        self.events.first_key_value().map(|(key, _)| *key)
    }

    /// Remove and return the earliest event.
    ///
    /// # Panics
    /// If the popped key is not after the previous one. Keys only grow, and nothing is pushed
    /// before the last pop, so this is a bug.
    pub fn pop(&mut self) -> Option<(Ms, E)> {
        let ((at, seq), event) = self.events.pop_first()?;
        if let Some(floor) = self.floor {
            assert!(
                (at, seq) > floor,
                "EventQueue: popped ({at}, {seq:?}) after {floor:?}"
            );
        }
        self.floor = Some((at, seq));
        Some((at, event))
    }

    /// Number of events waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether no event is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_push_works() {
        let mut q = EventQueue::new();
        assert_eq!(q.push(Ms(5), 'a'), Seq(0));
        assert_eq!(q.pop(), Some((Ms(5), 'a')));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn ties_go_in_push_order() {
        let mut q = EventQueue::new();
        q.push(Ms(10), 'b');
        q.push(Ms(5), 'a');
        q.push(Ms(10), 'c');
        assert_eq!(q.peek(), Some((Ms(5), Seq(1))));
        let order: Vec<char> = std::iter::from_fn(|| q.pop().map(|(_, e)| e)).collect();
        assert_eq!(order, ['a', 'b', 'c']);
        assert!(q.is_empty());
    }

    #[test]
    fn pushing_at_the_last_pop_time_is_allowed() {
        let mut q = EventQueue::new();
        q.push(Ms(5), 'a');
        q.pop();
        q.push(Ms(5), 'b');
        assert_eq!(q.pop(), Some((Ms(5), 'b')));
    }

    #[test]
    #[should_panic(expected = "before the last pop")]
    fn nothing_is_scheduled_in_the_past() {
        let mut q = EventQueue::new();
        q.push(Ms(5), 'a');
        q.pop();
        q.push(Ms(4), 'b');
    }
}
