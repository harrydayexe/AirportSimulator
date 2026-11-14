//! The participants of the system (Phase 2 spec, ticket A3).
//!
//! A [`Node`] is one participant in the protocol: a replica, or a controller
//! that owns the nodes below it. Each node is written as if it ran alone on
//! its own machine. It never reads a clock, opens a connection or touches a
//! disk; it is handed inputs and returns effects, and whatever runs it (its
//! runtime) does the rest.
//!
//! Two runtimes can drive the same nodes. The `host` crate is the simulated
//! one: every node in one program on a fake network, for seeded test runs and
//! for the browser demo. A real one would run a single node per process on
//! real sockets. Nothing in this crate may depend on `host`, so the code a
//! real deployment would ship is exactly the code the simulator tests.
//!
//! The replica and the controllers arrive with tickets D1 and D3.

use det::{Ms, NodeId};

/// Something a runtime hands to a node. `M` is what nodes send one another.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Input<M> {
    /// A time the node asked to be woken at has passed. May arrive early or
    /// late, so the node checks the time itself.
    Wake,
    /// A message from another node.
    Message {
        /// The node that sent it.
        from: NodeId,
        /// What it sent.
        message: M,
    },
}

/// Something a node asks its runtime to do.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Effect<M> {
    /// Send a message to another node. It may arrive late, twice, out of
    /// order or not at all.
    Send {
        /// The node to send it to.
        to: NodeId,
        /// What to send.
        message: M,
    },
}

/// One participant in the protocol: a replica or a controller.
///
/// A node owns its state and nothing else. It never holds a reference to
/// another node, even one it owns: a controller knows its children only by
/// their [`NodeId`]s and reaches them only by [`Effect::Send`].
pub trait Node {
    /// What nodes send one another.
    type Message;

    /// This node's address.
    #[must_use]
    fn id(&self) -> NodeId;

    /// Handle one input at time `now` and return what the runtime should do
    /// as a result. `now` never goes backwards between calls.
    fn step(&mut self, now: Ms, input: Input<Self::Message>) -> Vec<Effect<Self::Message>>;

    /// When the runtime should next send [`Input::Wake`], if ever. A pure
    /// read: two calls in a row give the same answer.
    #[must_use]
    fn next_wake(&self) -> Option<Ms>;
}
