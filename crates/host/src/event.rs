//! Events represent something waiting in the host's queue.
use det::NodeId;

/// Something waiting in the host's queue. `M` is what nodes send one another.
#[derive(Debug)]
pub enum Event<M> {
    /// Step `node` with `Input::Wake`.
    Wake {
        /// The ID of the node to send the wake to.
        node: NodeId,
    },
    /// Give `message` to `to` as `Input::Message`.
    Deliver {
        /// The ID of the node which sent the message.
        from: NodeId,
        /// The ID of the node to send the message to.
        to: NodeId,
        /// The message that was sent.
        message: M,
    },
}

impl<M> Event<M> {
    /// The node this event is for.
    #[must_use]
    pub fn target(&self) -> NodeId {
        match self {
            Event::Wake { node } => *node,
            Event::Deliver { to, .. } => *to,
        }
    }
}
