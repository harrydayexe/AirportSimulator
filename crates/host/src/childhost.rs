//! A [`ChildHost`] is a host which runs a group of nodes, but never moves time by itself.
//! This is never the top level node used by a program.
use std::collections::BTreeMap;

use det::{Config, Input, Ms, NodeId};
use raft::{browser_node::BrowserNode, node::Node};

use crate::{
    event::{Event, Report},
    host::Host,
    queue::EventQueue,
    seeds::Seeds,
};

/// Runs a group of nodes and their queue. Never moves time by itself.
pub struct ChildHost<C, N = BrowserNode<C>> {
    /// The nodes which this host is responsible for.
    nodes: BTreeMap<NodeId, N>,
    /// The queue of events this node has to process.
    queue: EventQueue<Event<C>>,
    /// The `Wake` time last scheduled for each node, so that a new `Wake` is added only when
    /// `next_wake` changes.
    scheduled_wake: BTreeMap<NodeId, Ms>,
    /// The time of the last `step`, kept only to check time never goes backwards.
    last_now: Ms,
    /// The source of randomness for this host.
    seeds: Seeds,
    /// The configuration this host must follow.
    config: Config,
    /// The perfect network's fixed delay.
    delay: Ms,
}

impl<C: Clone + Eq, N: Node<C>> ChildHost<C, N> {
    /// Runs the nodes `here`. `cluster` is every node in their Raft group,
    /// which may include nodes run by other hosts. Each node is brand new
    /// (term 0, empty log), gets its own generator from
    /// `seeds.rng(Stream::Node { id, life: 0 })`, and a first `Wake` at its
    /// `next_wake`.
    #[must_use]
    pub fn new(
        seeds: Seeds,
        config: Config,
        delay: Ms,
        here: &[NodeId],
        cluster: &[NodeId],
        now: Ms,
    ) -> Self {
        todo!()
    }

    /// Step one node and turn what it returns into events and reports.
    fn step_node(&mut self, now: Ms, id: NodeId, input: Input<C>, reports: Vec<Report<C>>) {
        // 1. If `id` is not in `nodes` (crashed, from C3 on), drop the input.
        // 2. effects = node.step(now, input)
        // 3. For each effect:
        //    Send { to, message } => if `to` is in `nodes`: queue.push(now + delay, Deliver { .. })
        //                            else: reports.push(Outbound { at: now + delay, .. })
        //    Committed { .. }     => reports.push(Committed { node: id, .. })
        //    NotLeader { hint }   => reports.push(NotLeader { node: id, hint })
        // 4. self.reschedule_wake(id)
        todo!()
    }

    /// If the node's `next_wake` differs from what is scheduled, push a new
    /// `Wake`. Old `Wake`s stay in the queue, and the node ignores them.
    fn reschedule_wake(&mut self, id: NodeId) {
        todo!()
    }
}

impl<C: Clone + Eq, N: Node<C>> Host<C> for ChildHost<C, N> {
    fn next_event_at(&self) -> Option<Ms> {
        self.queue.peek().map(|(at, _)| at)
    }

    fn step(&mut self, now: Ms) -> Vec<Report<C>> {
        todo!()
    }

    fn inject(&mut self, at: Ms, event: Event<C>) {
        todo!()
    }

    fn owns(&self, node: NodeId) -> bool {
        self.nodes.contains_key(&node)
    }

    fn statuses(&self) -> Vec<(NodeId, det::Status)> {
        self.nodes.iter().map(|(id, n)| (*id, n.status())).collect()
    }

    fn log(&self, node: NodeId) -> Option<&[det::Entry<C>]> {
        self.nodes.get(&node).map(|n| Node::log(n))
    }
}
