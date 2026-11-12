use crate::{Index, NodeId, Term};

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Payload<C> {
    Noop,
    Command(C),
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Entry<C> {
    pub term: Term,
    pub payload: Payload<C>,
}

/// Everything that survives a crash. Nothing else does.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Durable<C> {
    pub term: Term,
    pub voted_for: Option<NodeId>,
    pub log: Vec<Entry<C>>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Input<C> {
    /// A time you asked to be woken at has passed
    Wake,
    Message {
        from: NodeId,
        message: Message<C>,
    },
    Propose(C),
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Effect<C> {
    Send { to: NodeId, message: Message<C> },
    Committed { index: Index, entry: Entry<C> },
    NotLeader { hint: Option<NodeId> },
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum Role {
    Follower,
    Candidate,
    Leader,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Message<C> {
    Append { command: C },
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Healthy,
}
