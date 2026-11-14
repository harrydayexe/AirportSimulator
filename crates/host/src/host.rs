//! The [`Host`]: the simulated runtime that drives every [`Node`] of a run.

use std::collections::BTreeMap;

use det::{Ms, NodeId};
use node::{Input, Node};

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
    /// If two nodes share an id.
    #[must_use]
    pub fn new(_seeds: Seeds, _delay: Ms, _nodes: Vec<N>) -> Self {
        todo!()
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
        // 1. Pop the earliest event; if there is none, return false.
        // 2. clock.advance_to(its time)
        // 3. Wake { node }               => step_node(node, Input::Wake)
        //    Deliver { from, to, message } => step_node(to, Input::Message { from, message })
        todo!()
    }

    /// Add an event to the queue, for example the first message of a
    /// scenario. `at` must not be earlier than `now`, and the event's target
    /// must be a node of this host.
    pub fn inject(&mut self, _at: Ms, _event: Event<N::Message>) {
        todo!()
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
    fn step_node(&mut self, _id: NodeId, _input: Input<N::Message>) {
        // 1. If `id` is not in `nodes` (crashed, from C3 on), drop the input.
        // 2. effects = node.step(clock.now(), input)
        // 3. For each Send { to, message }: `to` must be a node this host was
        //    built with; queue.push(now + delay, Deliver { from: id, to, message }).
        //    Whether `to` is running is decided on delivery, not here.
        // 4. self.reschedule_wake(id)
        todo!()
    }

    /// If the node's `next_wake` differs from what is scheduled, push a new
    /// `Wake`. Old `Wake`s stay in the queue, and the node ignores them.
    fn reschedule_wake(&mut self, _id: NodeId) {
        todo!()
    }
}
