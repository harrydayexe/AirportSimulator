//! An input is something handed to a node by its host.
use det::NodeId;

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
