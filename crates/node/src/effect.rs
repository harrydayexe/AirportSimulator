//! An effect is something that a node asks its host to do
use det::NodeId;

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
