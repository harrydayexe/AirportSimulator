//! A [`ChildHost`] is a host which runs a group of nodes, but never moves time by itself.
//! This is never the top level node used by a program.
use std::collections::{BTreeMap, BTreeSet};

use det::{Config, Durable, Effect, Input, Ms, NodeId, Term};
use raft::{browser_node::BrowserNode, node::Node};

use crate::{
    event::{Event, Report},
    host::Host,
    queue::EventQueue,
    seeds::{Seeds, Stream},
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
        let mut nodes: BTreeMap<NodeId, N> = BTreeMap::new();
        let queue: EventQueue<Event<C>> = EventQueue::new();
        let scheduled_wake: BTreeMap<NodeId, Ms> = BTreeMap::new();
        let cluster_set: BTreeSet<NodeId> = cluster.iter().copied().collect();

        for id in here {
            let peers: Vec<NodeId> = cluster_set.iter().copied().filter(|n| n != id).collect();

            let rng = seeds.rng(Stream::Node { id: *id, life: 0 });
            let node = N::new(
                *id,
                peers,
                config,
                Durable::new(Term(0), None, Vec::new()),
                rng,
                now,
            );
            nodes.insert(*id, node);
        }

        let mut host = Self {
            nodes,
            queue,
            scheduled_wake,
            last_now: now,
            seeds,
            config,
            delay,
        };

        for id in here {
            host.reschedule_wake(*id);
        }

        host
    }

    /// Step one node and turn what it returns into events and reports.
    fn step_node(&mut self, now: Ms, id: NodeId, input: Input<C>, reports: &mut Vec<Report<C>>) {
        // Crashed: drop the input
        let Some(node) = self.nodes.get_mut(&id) else {
            return;
        };
        for effect in node.step(now, input) {
            match effect {
                Effect::Send { to, message } => {
                    if self.nodes.contains_key(&to) {
                        self.queue.push(
                            now + self.delay,
                            Event::Deliver {
                                from: id,
                                to,
                                message,
                            },
                        );
                    } else {
                        reports.push(Report::Outbound {
                            at: now + self.delay,
                            from: id,
                            to,
                            message,
                        });
                    }
                }
                Effect::Committed { index, entry } => reports.push(Report::Committed {
                    node: id,
                    index,
                    entry,
                }),
                Effect::NotLeader { hint } => reports.push(Report::NotLeader { node: id, hint }),
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

impl<C: Clone + Eq, N: Node<C>> Host<C> for ChildHost<C, N> {
    fn next_event_at(&self) -> Option<Ms> {
        self.queue.peek().map(|(at, _)| at)
    }

    fn step(&mut self, now: Ms) -> Vec<Report<C>> {
        assert!(now >= self.last_now);
        assert_eq!(Some(now), self.next_event_at());
        let (_, event) = self.queue.pop().unwrap();
        let mut reports: Vec<Report<C>> = vec![];
        match event {
            Event::Wake { node } => self.step_node(now, node, Input::Wake, &mut reports),
            Event::Deliver { from, to, message } => {
                self.step_node(now, to, Input::Message { from, message }, &mut reports);
            }
            Event::Propose { to, command } => {
                self.step_node(now, to, Input::Propose(command), &mut reports);
            }
        }

        reports
    }

    fn inject(&mut self, at: Ms, event: Event<C>) {
        assert!(at >= self.last_now);
        self.queue.push(at, event);
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
