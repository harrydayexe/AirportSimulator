//! The values that pass between a Raft node and the host: log entries, the
//! state that survives a crash, and a node's inputs and effects (Phase 2
//! spec, ticket B1). `C` is the command type stored in the log.

use serde::{Deserialize, Serialize};

use crate::{Index, NodeId, Term};

/// What a log entry carries.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Payload<C> {
    /// An empty entry a new leader adds in its own term so that entries left
    /// from earlier terms can be committed. Skipped when applying.
    Noop,
    /// A client command to apply once committed.
    Command(C),
}

/// One position in the log.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
impl<C> Durable<C> {
    /// What a node keeps across a crash.
    #[must_use]
    pub fn new(term: Term, voted_for: Option<NodeId>, log: Vec<Entry<C>>) -> Self {
        Self {
            term,
            voted_for,
            log,
        }
    }

    /// Checks the shape a node's durable state always has (O1, O2). Which
    /// node `voted_for` may name is the node's own check (O3).
    ///
    /// # Panics
    /// If the log breaks [`assert_log_shape`], or a node that has never seen
    /// an election (term 0) has a vote or a log.
    pub fn assert_valid(&self) {
        assert_log_shape(&self.log, self.term);
        if self.term == Term(0) {
            assert!(
                self.voted_for.is_none(),
                "Durable: term 0 with a vote for {:?}",
                self.voted_for
            );
            assert!(
                self.log.is_empty(),
                "Durable: term 0 with {} log entries",
                self.log.len()
            );
        }
    }
}

/// Checks a log is one a node could have built by `term` (O1, N8): every
/// entry is from term 1 or later, terms never fall along the log, and none is
/// later than `term`.
///
/// # Panics
/// If any of those fails, naming the first position that breaks it.
pub fn assert_log_shape<C>(log: &[Entry<C>], term: Term) {
    let mut previous = Term(1);
    for (i, entry) in log.iter().enumerate() {
        let position = i + 1;
        assert!(
            entry.term >= previous,
            "log: entry {position} has term {} after term {previous}",
            entry.term
        );
        previous = entry.term;
    }
    if let Some(last) = log.last() {
        assert!(
            last.term <= term,
            "log: last entry has term {} beyond the node's term {term}",
            last.term
        );
    }
}

/// Something the host hands to a node.
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Input<C> {
    /// A time you asked to be woken at has passed. May arrive early or late,
    /// so the node checks the time itself.
    Wake,
    /// A message from another node.
    Message {
        /// Where the message came from.
        from: NodeId,
        /// What it sent.
        message: Message<C>,
    },
    /// A client wants this command stored.
    Propose(C),
}

/// Something a node asks the host to do.
/// TODO: Add `EffectBuilder` which can build the effect by doing `.to`, `.from` etc
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
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Message<C> {
    /// The term number of the sender.
    pub term: Term,
    /// The acctual message sent.
    pub command: MessageCommand<C>,
}

impl<C> Message<C> {
    /// Create a new [`Message`].
    #[must_use]
    pub fn new(term: Term, command: MessageCommand<C>) -> Self {
        Self { term, command }
    }

    /// Checks the shape every message has, whoever sent it (O4, O5). Every
    /// node runs this code, so a malformed message is a bug in the sender.
    ///
    /// # Panics
    /// If the term is 0, or an `AppendEntries` has `prev_index` and
    /// `prev_term` disagreeing about the start of the log, or entries whose
    /// terms fall, start before `prev_term` or end after the message's term.
    pub fn assert_valid(&self) {
        assert!(self.term >= Term(1), "message carries term 0");
        if let MessageCommand::AppendEntries {
            prev_index,
            prev_term,
            entries,
            leader_commit: _,
        } = &self.command
        {
            assert_eq!(
                *prev_index == Index(0),
                *prev_term == Term(0),
                "AppendEntries: prev_index {prev_index} with prev_term {prev_term}"
            );
            if let Some(first) = entries.first() {
                assert!(
                    first.term >= *prev_term,
                    "AppendEntries: first entry term {} before prev_term {prev_term}",
                    first.term
                );
            }
            assert_log_shape(entries, self.term);
        }
    }
}

/// The type of message sent between two nodes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageCommand<C> {
    /// Requesting a vote for leader.
    /// Should grant if we have voted for them last, or not voted for anyone.
    RequestVote {
        /// The position of the last entry in the candidate's log; 0 when it
        /// is empty.
        last_log_index: Index,
        /// The term of that entry; 0 when the log is empty.
        last_log_term: Term,
    },
    /// A vote in our favour. Ignore if not a candidate, add the voter to a set and on majority
    /// become the leader
    VoteReply,
    /// Append the new entries, if checks match.
    AppendEntries {
        /// The index of the entry just before these new ones; 0 when they
        /// start the log.
        prev_index: Index,
        /// The term of the entry at `prev_index`; 0 when they start the log.
        prev_term: Term,
        /// The list of entries from the leader.
        entries: Vec<Entry<C>>,
        /// The leader's commit position. May be beyond what this message
        /// covers (O5).
        leader_commit: Index,
    },
    /// Reply from a follower to a leader after appending messages to it's log
    AppendReply {
        /// Whether the append was successful or not
        success: bool,
        /// On success, how far its log is known to match the leader's, i.e. the position of the
        /// last entry the message covered.
        last_match_index: Index,
    },
}

impl<C> MessageCommand<C> {
    /// Returns whether the message command is a request or not.
    #[must_use]
    pub fn is_request(&self) -> bool {
        match self {
            MessageCommand::RequestVote { .. } | MessageCommand::AppendEntries { .. } => true,
            MessageCommand::VoteReply | MessageCommand::AppendReply { .. } => false,
        }
    }
}

/// A node's role, term and positions, for checkers and the web view. A
/// placeholder until those are filled in.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum Status {
    /// Placeholder value.
    Healthy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_replies_are_told_apart() {
        let append = MessageCommand::<u8>::AppendEntries {
            prev_index: Index(0),
            prev_term: Term(0),
            entries: Vec::new(),
            leader_commit: Index(0),
        };
        let vote = MessageCommand::<u8>::RequestVote {
            last_log_index: Index(0),
            last_log_term: Term(0),
        };
        let reply = MessageCommand::<u8>::AppendReply {
            success: true,
            last_match_index: Index(0),
        };
        assert!(vote.is_request());
        assert!(!MessageCommand::<u8>::VoteReply.is_request());
        assert!(append.is_request());
        assert!(!reply.is_request());
    }

    #[test]
    fn durable_and_message_keep_what_they_are_given() {
        let log = vec![Entry {
            term: Term(2),
            payload: Payload::Command(7u8),
        }];
        let durable = Durable::new(Term(3), Some(NodeId(2)), log);
        assert_eq!(durable.term, Term(3));
        assert_eq!(durable.voted_for, Some(NodeId(2)));
        assert_eq!(
            durable.log,
            [Entry {
                term: Term(2),
                payload: Payload::Command(7)
            }]
        );
        durable.assert_valid();

        let message = Message::new(Term(4), MessageCommand::<u8>::VoteReply);
        assert_eq!(message.term, Term(4));
        assert_eq!(message.command, MessageCommand::VoteReply);
        message.assert_valid();
    }
}
