//! The values that pass between a Raft node and the host: log entries, the
//! state that survives a crash, and a node's inputs and effects (Phase 2
//! spec, ticket B1). `C` is the command type stored in the log.

use crate::{Index, NodeId, Term};

/// What a log entry carries.
#[derive(Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Payload<C> {
    /// An empty entry a new leader adds in its own term so that entries left
    /// from earlier terms can be committed. Skipped when applying.
    Noop,
    /// A client command to apply once committed.
    Command(C),
}

/// One position in the log.
#[derive(Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Entry<C> {
    /// The election term of the leader that added the entry.
    pub term: Term,
    /// What the entry carries.
    pub payload: Payload<C>,
}

/// Everything that survives a crash. Nothing else does.
#[derive(Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Durable<C> {
    /// The latest election term the node has seen; 0 for a brand new node.
    pub term: Term,
    /// The node this one voted for in `term`, if any.
    pub voted_for: Option<NodeId>,
    /// The whole log, committed or not.
    pub log: Vec<Entry<C>>,
}
impl<C> Durable<C> {
    pub fn new(term: Term, voted_for: Option<NodeId>, log: Vec<Entry<C>>) -> Self {
        Self {
            term,
            voted_for,
            log,
        }
    }
}

/// Something the host hands to a node.
#[derive(Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Input<C> {
    /// A time you asked to be woken at has passed. May arrive early or late,
    /// so the node checks the time itself.
    Wake,
    /// A message from another node.
    Message {
        /// What it sent.
        message: Message<C>,
    },
    /// A client wants this command stored.
    Propose(C),
}

/// Something a node asks the host to do.
/// TODO: Add EffectBuilder which can build the effect by doing .to, .from etc
#[derive(Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
#[non_exhaustive]
pub struct Message<C> {
    /// The term number of the sender.
    pub term: Term,
    /// The ID of the sender node.
    pub from: NodeId,
    /// The ID of the intended recipient node.
    pub to: NodeId,
    /// The acctual message sent.
    pub command: MessageCommand<C>,
}

impl<C> Message<C> {
    /// Create a new [`Message`].
    pub fn new(term: Term, from: NodeId, to: NodeId, command: MessageCommand<C>) -> Self {
        Self {
            term,
            from,
            to,
            command,
        }
    }
}

/// The type of message sent between two nodes.
#[derive(Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MessageCommand<C> {
    /// Requesting a vote for leader.
    /// Should grant if we have voted for them last, or not voted for anyone.
    RequestVote,
    /// A vote in our favour. Ignore if not a candidate, add the voter to a set and on majority
    /// become the leader
    VoteReply,
    /// Append the new entries, if checks match.
    AppendEntries {
        /// The index of the entry just before these new ones.
        prev_index: usize,
        /// The term of the previous entry before these new ones.
        prev_term: C,
        /// The list of entries from the leader.
        entries: Vec<C>,
    },
    /// Reply from a follower to a leader after appending messages to it's log
    AppendReply {
        /// Whether the append was successful or not
        success: bool,
        /// On success, how far its log is known to match the leader's, i.e. the position of the
        /// last entry the message covered.
        last_match_index: usize,
    },
}

impl<C> MessageCommand<C> {
    /// Returns whether the message command is a request or not.
    pub fn is_request(&self) -> bool {
        match self {
            MessageCommand::RequestVote
            | MessageCommand::AppendEntries {
                prev_index: _,
                prev_term: _,
                entries: _,
            } => true,
            MessageCommand::VoteReply
            | MessageCommand::AppendReply {
                success: _,
                last_match_index: _,
            } => false,
        }
    }
}

/// A node's role, term and positions, for checkers and the web view. A
/// placeholder until those are filled in.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[non_exhaustive]
pub enum Status {
    /// Placeholder value.
    Healthy,
}
