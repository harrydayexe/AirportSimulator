//! The [`Host`]: the simulated runtime that drives every [`Node`] of a run.

use std::collections::BTreeMap;

use det::{Ms, NodeId};
use node::{Effect, Input, Node};

use crate::{clock::Clock, event::Event, queue::EventQueue, seeds::Seeds};

/// Runs every node of a run on simulated time, and plays the network between
/// them.
///
/// The nodes form a tree (a controller owns the nodes below it), but the host
/// does not: it keeps them side by side in one map with one queue and one
/// clock, the way a real network sees separate machines. A node can address
/// any other node; which ones it does address is the nodes' business.
pub struct Host<N: Node> {
    /// The one simulated time.
    clock: Clock,
    /// Every running node, by address.
    nodes: BTreeMap<NodeId, N>,
    /// Everything that is going to happen, in order.
    queue: EventQueue<Event<N::Message>>,
    /// The `Wake` time last scheduled for each node, so that a new `Wake` is added only when
    /// `next_wake` changes.
    scheduled_wake: BTreeMap<NodeId, Ms>,
    /// The source of randomness for this run.
    seeds: Seeds,
    /// The perfect network's fixed delay.
    delay: Ms,
}

impl<N: Node> Host<N> {
    /// A host at time 0 running `nodes`, each of which gets a first `Wake` at
    /// its `next_wake`. The caller builds the nodes, giving each its own
    /// generator from `seeds.rng(Stream::Node { id, life: 0 })`.
    ///
    /// # Panics
    /// If two nodes share an id (H1).
    #[must_use]
    pub fn new(seeds: Seeds, delay: Ms, nodes: Vec<N>) -> Self {
        let mut host = Self {
            clock: Clock::default(),
            nodes: BTreeMap::new(),
            queue: EventQueue::new(),
            scheduled_wake: BTreeMap::new(),
            seeds,
            delay,
        };
        for node in nodes {
            let id = node.id();
            assert!(
                host.nodes.insert(id, node).is_none(),
                "Host: two nodes share id {id}"
            );
        }
        let ids: Vec<NodeId> = host.nodes.keys().copied().collect();
        for id in ids {
            host.reschedule_wake(id);
        }
        host
    }

    /// The seeds of this run, for building a node's generator on restart.
    #[must_use]
    pub fn seeds(&self) -> Seeds {
        self.seeds
    }

    /// The current simulated time.
    #[must_use]
    pub fn now(&self) -> Ms {
        self.clock.now()
    }

    /// The time of the earliest pending event, if there is one.
    #[must_use]
    pub fn next_event_at(&self) -> Option<Ms> {
        self.queue.peek().map(|(at, _)| at)
    }

    /// Move the clock to the earliest event and handle it. Returns `false`
    /// when nothing is pending.
    pub fn step(&mut self) -> bool {
        let Some((at, event)) = self.queue.pop() else {
            return false;
        };
        self.clock.advance_to(at);
        match event {
            Event::Wake { node } => self.step_node(node, Input::Wake),
            Event::Deliver { from, to, message } => {
                self.step_node(to, Input::Message { from, message });
            }
        }
        true
    }

    /// Add an event to the queue, for example the first message of a
    /// scenario.
    ///
    /// # Panics
    /// If `at` is earlier than `now`, or the event's target is not a node of
    /// this host (H3).
    pub fn inject(&mut self, at: Ms, event: Event<N::Message>) {
        let now = self.clock.now();
        assert!(
            at >= now,
            "Host: event injected at {at} ms, before {now} ms"
        );
        let target = event.target();
        assert!(
            self.nodes.contains_key(&target),
            "Host: event injected for unknown node {target}"
        );
        self.queue.push(at, event);
    }

    /// One node, if it is running. For checkers and the web view.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&N> {
        self.nodes.get(&id)
    }

    /// Every running node, in `NodeId` order. For checkers and the web view.
    pub fn nodes(&self) -> impl Iterator<Item = &N> {
        self.nodes.values()
    }

    /// Step one node and turn what it returns into events.
    fn step_node(&mut self, id: NodeId, input: Input<N::Message>) {
        let now = self.clock.now();
        // Crashed (from C3 on): drop the input.
        let Some(node) = self.nodes.get_mut(&id) else {
            return;
        };
        for effect in node.step(now, input) {
            match effect {
                Effect::Send { to, message } => {
                    // H4. Once nodes can crash (C3) this must become "a node
                    // this host was built with", running or not.
                    assert!(
                        to != id && self.nodes.contains_key(&to),
                        "Host: node {id} sent to {to}, which is itself or unknown"
                    );
                    self.queue.push(
                        now + self.delay,
                        Event::Deliver {
                            from: id,
                            to,
                            message,
                        },
                    );
                }
            }
        }
        self.reschedule_wake(id);
    }

    /// If the node's `next_wake` differs from what is scheduled, push a new
    /// `Wake`. Old `Wake`s stay in the queue, and the node ignores them.
    fn reschedule_wake(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        // Node wants no wake.
        let Some(next) = node.next_wake() else { return };
        if self.scheduled_wake.get(&id) != Some(&next) {
            self.queue.push(next, Event::Wake { node: id });
            self.scheduled_wake.insert(id, next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wakes once at `wake_at` to send a 0 to `peer`, and answers every
    /// number below `limit` with the next one.
    struct Counter {
        id: NodeId,
        peer: NodeId,
        wake_at: Option<Ms>,
        limit: u32,
        seen: Vec<(Ms, u32)>,
    }

    impl Node for Counter {
        type Message = u32;

        fn id(&self) -> NodeId {
            self.id
        }

        fn step(&mut self, now: Ms, input: Input<u32>) -> Vec<Effect<u32>> {
            let next = match input {
                Input::Wake if self.wake_at.is_some_and(|at| now >= at) => {
                    self.wake_at = None;
                    0
                }
                Input::Wake => return Vec::new(),
                Input::Message { from, message } => {
                    assert_eq!(from, self.peer);
                    self.seen.push((now, message));
                    message + 1
                }
            };
            if next >= self.limit {
                return Vec::new();
            }
            vec![Effect::Send {
                to: self.peer,
                message: next,
            }]
        }

        fn next_wake(&self) -> Option<Ms> {
            self.wake_at
        }
    }

    fn counter(id: u32, peer: u32, wake_at: Option<u64>) -> Counter {
        Counter {
            id: NodeId(id),
            peer: NodeId(peer),
            wake_at: wake_at.map(Ms),
            limit: 4,
            seen: Vec::new(),
        }
    }

    #[test]
    fn two_nodes_pass_a_message_back_and_forth() {
        let nodes = vec![counter(1, 2, Some(10)), counter(2, 1, None)];
        let mut host = Host::new(Seeds::new(1), Ms(5), nodes);
        assert_eq!(host.next_event_at(), Some(Ms(10)));
        while host.step() {}
        // Woken at 10; each hop takes the fixed 5 ms.
        assert_eq!(host.now(), Ms(30));
        let seen = |id| host.node(NodeId(id)).unwrap().seen.clone();
        assert_eq!(seen(2), [(Ms(15), 0), (Ms(25), 2)]);
        assert_eq!(seen(1), [(Ms(20), 1), (Ms(30), 3)]);
    }

    #[test]
    #[should_panic(expected = "share id")]
    fn two_nodes_cannot_share_an_address() {
        let _ = Host::new(
            Seeds::new(1),
            Ms(5),
            vec![counter(1, 2, None), counter(1, 2, None)],
        );
    }

    #[test]
    #[should_panic(expected = "unknown node")]
    fn an_event_for_a_stranger_is_refused() {
        let mut host = Host::new(Seeds::new(1), Ms(5), vec![counter(1, 2, None)]);
        host.inject(Ms(0), Event::Wake { node: NodeId(9) });
    }
}
