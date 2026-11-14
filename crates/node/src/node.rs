//! The [`Node`] trait: the whole interface between a node and its host.
//!
//! A node is one participant in the system's message exchange, written as if
//! it ran alone on its own machine. It has an address ([`NodeId`]), is handed
//! messages and wakes by its host, and returns the messages it wants sent.
//! It never reads a clock, opens a connection or touches a disk: the time
//! arrives with every input, and everything it wants done leaves as an
//! [`Effect`].
//!
//! The trait says nothing about Raft or airports. A replica is a node that
//! holds a Raft member and an airport; other nodes hold other things. A Raft
//! member and a group client are not nodes: they have no address of their
//! own and live inside a node.
//!
//! The host always calls the node, never the other way round, so a node
//! cannot tell what kind of host is running it. That is what lets the same
//! node run under a simulated host with a fake network and clock, or under a
//! production host with real ones.
use det::{Ms, NodeId};

use crate::{effect::Effect, input::Input};

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
