//! The values that pass between a Raft node and the host: log entries, the
//! state that survives a crash, and a node's inputs and effects (Phase 2
//! spec, ticket B1). `C` is the command type stored in the log.

use crate::{Index, NodeId, Term};

/// What a log entry carries.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Payload<C> {
    /// An empty entry a new leader adds in its own term so that entries left
    /// from earlier terms can be committed. Skipped when applying.
    Noop,
    /// A client command to apply once committed.
    Command(C),
}

/// One position in the log.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Entry<C> {
    /// The election term of the leader that added the entry.
    pub term: Term,
    /// What the entry carries.
    pub payload: Payload<C>,
}

/// Everything that survives a crash. Nothing else does.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Durable<C> {
    /// The latest election term the node has seen; 0 for a brand new node.
    pub term: Term,
    /// The node this one voted for in `term`, if any.
    pub voted_for: Option<NodeId>,
    /// The whole log, committed or not.
    pub log: Vec<Entry<C>>,
}

/// Something the host hands to a node.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Input<C> {
    /// A time you asked to be woken at has passed. May arrive early or late,
    /// so the node checks the time itself.
    Wake,
    /// A message from another node.
    Message {
        /// The node that sent it.
        from: NodeId,
        /// What it sent.
        message: Message<C>,
    },
    /// A client wants this command stored.
    Propose(C),
}

/// Something a node asks the host to do.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Effect<C> {
    /// Deliver a message to another node.
    Send {
        /// The node to deliver it to.
        to: NodeId,
        /// What to deliver.
        message: Message<C>,
    },
    /// An entry is now committed. Emitted in order, each position once per
    /// life of the node.
    Committed {
        /// The entry's position in the log.
        index: Index,
        /// The committed entry.
        entry: Entry<C>,
    },
    /// Answer to a `Propose` this node cannot accept.
    NotLeader {
        /// The leader this node last heard from, if it knows one.
        hint: Option<NodeId>,
    },
}

/// The part a node plays in its current election term.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum Role {
    /// Accepts entries from a leader. Every node starts here.
    Follower,
    /// Asking the other nodes for votes.
    Candidate,
    /// Accepts proposals and copies its log to the followers.
    Leader,
}

/// A message between nodes. A placeholder until the election and log-copying
/// messages are added.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Message<C> {
    /// Placeholder: ask a follower to append a command.
    Append {
        /// The command to append.
        command: C,
    },
}

/// A node's role, term and positions, for checkers and the web view. A
/// placeholder until those are filled in.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// Placeholder value.
    Healthy,
}
