//! The participants of the system (Phase 2 spec, ticket A3).
//!
//! A [`Node`] is one participant in the protocol. The replica is the first;
//! a region's air traffic control (Phase 4) will be another. Each node is
//! written as if it ran alone on its own machine. It never reads a clock,
//! opens a connection or touches a disk; it is handed inputs and returns
//! effects, and its host does the rest.
//!
//! [`Node`] is the whole interface between a node and its host, and there
//! are two kinds of host for the same nodes. The simulated one (the `host`
//! crate) puts every node in one program on a fake network and clock, for
//! seeded test runs and for the browser demo. A production one runs a single
//! node on its own machine with real sockets, the system clock and a file.
//! Nothing in this crate may depend on `host`, so the code a real deployment
//! would ship is exactly the code the simulation tests.
//!
//! The replica and the group client arrive with tickets D1 and D3.

use det::{Ms, NodeId};

/// Something a host hands to its node. `M` is what nodes send one another.
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

/// Something a node asks its host to do.
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

/// One participant in the protocol, as its host sees it.
///
/// A node owns its state and nothing else. It never holds a reference to
/// another node: it knows the others only by their [`NodeId`]s and reaches
/// them only by [`Effect::Send`].
pub trait Node {
    /// What nodes send one another.
    type Message;

    /// This node's address.
    #[must_use]
    fn id(&self) -> NodeId;

    /// Handle one input at time `now` and return what the host should do
    /// as a result. `now` never goes backwards between calls.
    fn step(&mut self, now: Ms, input: Input<Self::Message>) -> Vec<Effect<Self::Message>>;

    /// When the host should next send [`Input::Wake`], if ever. A pure
    /// read: two calls in a row give the same answer.
    #[must_use]
    fn next_wake(&self) -> Option<Ms>;
}
