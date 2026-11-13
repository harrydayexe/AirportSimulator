//! Events represent something waiting in the host's queue.
use det::{Entry, Index, Ms};
use det::{NodeId, objects::Message};

/// Something waiting in the host's queue.
#[derive(Debug)]
pub enum Event<C> {
    /// Step `node` with `Input::Wake`.
    Wake {
        /// The ID of the node to send the wake to.
        node: NodeId,
    },
    /// Give `message` to `to` as `Input::Message`
    Deliver {
        /// The ID of the node which sent the message.
        from: NodeId,
        /// The ID of the node to send the message to.
        to: NodeId,
        /// The message that was sent.
        message: Message<C>,
    },
    /// Gives `command` to `to` as `Input::Propose`.
    Propose {
        /// The ID of the node to propose the `command` to.
        to: NodeId,
        /// The command to propose.
        command: C,
    },
}

impl<C> Event<C> {
    /// The node this event is for. Used to route an injected event to the host that owns that node.
    #[must_use]
    pub fn target(&self) -> NodeId {
        match self {
            Event::Wake { node } => *node,
            Event::Deliver {
                from: _from,
                to,
                message: _message,
            } => *to,
            Event::Propose {
                to,
                command: _command,
            } => *to,
        }
    }
}

/// Something a host hands up to the host that owns it.
#[derive(Debug)]
pub enum Report<C> {
    /// A message for a node this host does not own.
    Outbound {
        /// `at` is when the message should arrive, because the sender's network has already chosen
        /// the delay.
        at: Ms,
        /// The ID of the sending node.
        from: NodeId,
        /// The ID of the recipient node.
        to: NodeId,
        /// The message being sent.
        message: Message<C>,
    },
    /// A node emitted `Effect::Committed`.
    Committed {
        /// The ID of the sending node.
        node: NodeId,
        /// The index up to which the Node has committed.
        index: Index,
        /// The queue entry.
        entry: Entry<C>,
    },
    /// A node answered a proposal with `Effect::NotLeader`.
    NotLeader {
        /// The ID of the sending node.
        node: NodeId,
        /// A hint to who the sending node thinks the leader is.
        hint: Option<NodeId>,
    },
}
