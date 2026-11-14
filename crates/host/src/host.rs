//! [`SimHost`]: the simulated host of one node.

use det::{Ms, NodeId};
use node::{Effect, Input, Node};

/// The host of one node in a simulated run.
///
/// Every node has a host, and the host is everything the node knows of the
/// outside world: it hands the node its messages and wakes, tells it the
/// time, and (from ticket C3) keeps its durable state across a crash. A
/// production host does the same job for one node with a socket, the
/// machine's clock and a file. This one does it with the shared [`Sim`],
/// which stands in for the network and the clock between hosts.
///
/// [`Sim`]: crate::sim::Sim
pub struct SimHost<N: Node> {
    /// The node this host runs.
    node: N,
    /// The `Wake` time last scheduled for the node, so that a new `Wake` is
    /// added only when `next_wake` changes.
    scheduled_wake: Option<Ms>,
}

impl<N: Node> SimHost<N> {
    /// A host running `node`, with no `Wake` scheduled yet.
    #[must_use]
    pub fn new(node: N) -> Self {
        Self {
            node,
            scheduled_wake: None,
        }
    }

    /// The address of the node this host runs.
    #[must_use]
    pub fn id(&self) -> NodeId {
        self.node.id()
    }

    /// The node, read-only. For checkers and the web view.
    #[must_use]
    pub fn node(&self) -> &N {
        &self.node
    }

    /// Hand one input to the node at time `now` and return what it wants
    /// sent. The caller puts those messages on the network.
    pub fn step(&mut self, now: Ms, input: Input<N::Message>) -> Vec<Effect<N::Message>> {
        self.node.step(now, input)
    }

    /// The time of a `Wake` the network should now schedule: `Some` only when
    /// the node's `next_wake` differs from the one last scheduled. Old
    /// `Wake`s stay in the queue, and the node ignores them.
    pub fn wake_to_schedule(&mut self) -> Option<Ms> {
        let next = self.node.next_wake()?;
        if self.scheduled_wake == Some(next) {
            return None;
        }
        self.scheduled_wake = Some(next);
        Some(next)
    }
}
