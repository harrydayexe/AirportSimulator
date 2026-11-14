//! A message is everything one node can say to someone else.
use det::{ClientId, NodeId, seq::Seq};

/// Everything one node can say to another. Grows as the tickets land.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Message<C, O> {
    /// Replica to replica message, in the same group.
    Raft(det::objects::Message<Request<C>>),
    /// Request sent from a client, to a replica.
    Request(Request<C>),
    /// Reply sent from a replica, to a client that sent the original request.
    Reply(ClientReply<O>),
}

/// What a client asks of an airport.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Request<C> {
    /// The ID of the client sending the request.
    pub client: ClientId,
    /// The sequence number of the request from the client.
    pub seq: Seq,
    /// The actual command from the client.
    pub command: C,
}

/// The reply to a client's [`Request`].
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum ClientReply<O> {
    /// The request has been processed.
    Done {
        /// The sequence of the request being answered.
        seq: Seq,
        /// The outputs from the request being processed.
        outputs: Vec<O>,
    },
    /// The replica the message was sent to was not the leader.
    NotLeader {
        /// An optional hint at who the leader may be.
        hint: Option<NodeId>,
    },
}
