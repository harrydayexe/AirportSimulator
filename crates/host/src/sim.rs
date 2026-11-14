//! [`Sim`]: the simulated network and clock that every [`SimHost`] shares.

use std::collections::BTreeMap;

use det::{Ms, NodeId};
use node::{Effect, Input, Node};

use crate::{clock::Clock, event::Event, host::SimHost, queue::EventQueue, seeds::Seeds};

/// What sits between the hosts of a simulated run: one clock, one queue of
/// everything that is going to happen, and the fake network.
///
/// In production nothing like this exists as code. Its place is taken by the
/// real network and each machine's own clock. Here it is one object, because
/// a run can only be replayed from its seed if a single place decides the
/// order of events.
///
/// The hosts sit side by side, the way a real network sees separate machines.
/// Any node can address any other; which ones it does address is the nodes'
/// business.
pub struct Sim<N: Node> {
    /// The one simulated time.
    clock: Clock,
    /// The host of every running node, by the node's address.
    hosts: BTreeMap<NodeId, SimHost<N>>,
    /// Everything that is going to happen, in order.
    queue: EventQueue<Event<N::Message>>,
    /// The source of randomness for this run.
    seeds: Seeds,
    /// The perfect network's fixed delay.
    delay: Ms,
}

impl<N: Node> Sim<N> {
    /// A run at time 0 with one host per node, each of which gets a first
    /// `Wake` at its node's `next_wake`. The caller builds the nodes, giving
    /// each its own generator from `seeds.rng(Stream::Node { id, life: 0 })`.
    ///
    /// # Panics
    /// If two nodes share an id (H1).
    #[must_use]
    pub fn new(seeds: Seeds, delay: Ms, nodes: Vec<N>) -> Self {
        let mut sim = Self {
            clock: Clock::default(),
            hosts: BTreeMap::new(),
            queue: EventQueue::new(),
            seeds,
            delay,
        };
        for node in nodes {
            let host = SimHost::new(node);
            let id = host.id();
            assert!(
                sim.hosts.insert(id, host).is_none(),
                "Sim: two nodes share id {id}"
            );
        }
        let ids: Vec<NodeId> = sim.hosts.keys().copied().collect();
        for id in ids {
            sim.reschedule_wake(id);
        }
        sim
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

    /// Move the clock to the earliest event and hand it to its host. Returns
    /// `false` when nothing is pending.
    pub fn step(&mut self) -> bool {
        let Some((at, event)) = self.queue.pop() else {
            return false;
        };
        self.clock.advance_to(at);
        match event {
            Event::Wake { node } => self.step_host(node, Input::Wake),
            Event::Deliver { from, to, message } => {
                self.step_host(to, Input::Message { from, message });
            }
        }
        true
    }

    /// Add an event to the queue.
    ///
    /// # Panics
    /// If `at` is earlier than `now`, or the event's target is not a node of
    /// this run (H3).
    pub fn inject(&mut self, at: Ms, event: Event<N::Message>) {
        let now = self.clock.now();
        assert!(at >= now, "Sim: event injected at {at} ms, before {now} ms");
        let target = event.target();
        assert!(
            self.hosts.contains_key(&target),
            "Sim: event injected for unknown node {target}"
        );
        self.queue.push(at, event);
    }

    /// One node, if it is running. For checkers and the web view.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&N> {
        self.hosts.get(&id).map(SimHost::node)
    }

    /// Every running node, in `NodeId` order. For checkers and the web view.
    pub fn nodes(&self) -> impl Iterator<Item = &N> {
        self.hosts.values().map(SimHost::node)
    }

    /// Hand one input to a node's host and put what it sends on the network.
    fn step_host(&mut self, id: NodeId, input: Input<N::Message>) {
        let now = self.clock.now();
        // Crashed (from C3 on): drop the input.
        let Some(host) = self.hosts.get_mut(&id) else {
            return;
        };
        for effect in host.step(now, input) {
            match effect {
                Effect::Send { to, message } => {
                    // H4. Once nodes can crash (C3) this must become "a node
                    // this run was built with", running or not.
                    assert!(
                        to != id && self.hosts.contains_key(&to),
                        "Sim: node {id} sent to {to}, which is itself or unknown"
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

    /// Schedule a `Wake` for the node if its host asks for a new one.
    fn reschedule_wake(&mut self, id: NodeId) {
        let Some(host) = self.hosts.get_mut(&id) else {
            return;
        };
        if let Some(at) = host.wake_to_schedule() {
            self.queue.push(at, Event::Wake { node: id });
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
        let mut sim = Sim::new(Seeds::new(1), Ms(5), nodes);
        assert_eq!(sim.next_event_at(), Some(Ms(10)));
        while sim.step() {}
        // Woken at 10; each hop takes the fixed 5 ms.
        assert_eq!(sim.now(), Ms(30));
        let seen = |id| sim.node(NodeId(id)).unwrap().seen.clone();
        assert_eq!(seen(2), [(Ms(15), 0), (Ms(25), 2)]);
        assert_eq!(seen(1), [(Ms(20), 1), (Ms(30), 3)]);
    }

    #[test]
    #[should_panic(expected = "share id")]
    fn two_nodes_cannot_share_an_address() {
        let _ = Sim::new(
            Seeds::new(1),
            Ms(5),
            vec![counter(1, 2, None), counter(1, 2, None)],
        );
    }

    #[test]
    #[should_panic(expected = "unknown node")]
    fn an_event_for_a_stranger_is_refused() {
        let mut sim = Sim::new(Seeds::new(1), Ms(5), vec![counter(1, 2, None)]);
        sim.inject(Ms(0), Event::Wake { node: NodeId(9) });
    }
}
