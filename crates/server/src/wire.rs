//! What replicas say to each other over TCP: one JSON value per line.
//!
//! A connection carries messages one way. The node that opens it sends a
//! [`Frame::Hello`] and then [`Frame::Message`]s; the node that accepted it
//! only reads. Replies travel on the connection the other node opened.
//!
//! Everything read from a socket is checked here before the node sees it,
//! because the node treats a malformed message as a bug and stops.

use std::io::{self, BufRead, Read as _};

use det::objects::{Message, MessageCommand};
use det::{Entry, Index, NodeId, Term, fingerprint};
use host::airport::AirportConfig;
use serde::{Deserialize, Serialize};

use crate::replica::Request;

/// The longest line accepted, so a stream of rubbish cannot use up memory.
pub const MAX_LINE_BYTES: u64 = 32 * 1024 * 1024;

/// One line on a connection between replicas.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frame {
    /// First on every connection: who is calling, and for which cluster.
    Hello {
        /// The node that opened the connection.
        from: NodeId,
        /// See [`cluster_fingerprint`].
        cluster: u64,
    },
    /// A Raft message from the node named in the `Hello`.
    Message(Message<Request>),
}

impl Frame {
    /// The frame as one line, newline included.
    ///
    /// # Panics
    /// Never: a frame always serialises.
    #[must_use]
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).expect("a frame serialises");
        line.push('\n');
        line
    }
}

/// A hash of what every member must agree on before they exchange a single
/// message: who the members are, and the airport they all start from. Two
/// replicas built from different airports would apply the same log and end
/// up in different states, so they refuse to talk.
#[must_use]
pub fn cluster_fingerprint(members: &[NodeId], airport: &AirportConfig) -> u64 {
    let mut members = members.to_vec();
    members.sort_unstable();
    fingerprint(&(members, airport))
}

/// Reads one line, without its newline. `None` at the end of the stream.
///
/// # Errors
/// If the read fails, or the line is longer than [`MAX_LINE_BYTES`] or ends
/// without a newline.
pub fn read_line(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    reader.take(MAX_LINE_BYTES).read_until(b'\n', &mut line)?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.pop() != Some(b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "line too long or cut short",
        ));
    }
    Ok(Some(line))
}

/// Checks the entries of an `AppendEntries` (O5): terms start at 1, never
/// fall, begin at or after `prev_term` and end at or before `term`.
fn check_entries(entries: &[Entry<Request>], prev_term: Term, term: Term) -> Result<(), String> {
    let mut previous = prev_term.max(Term(1));
    for entry in entries {
        if entry.term < previous {
            return Err(format!("entry term {} after term {previous}", entry.term));
        }
        previous = entry.term;
    }
    if entries.last().is_some_and(|last| last.term > term) {
        return Err(format!("entries beyond the message's term {term}"));
    }
    Ok(())
}

/// Checks a message has the shape every message has (O4, O5), so that
/// [`Message::assert_valid`] cannot fail on it.
///
/// # Errors
/// Says what is wrong with the message.
pub fn validate(message: &Message<Request>) -> Result<(), String> {
    if message.term < Term(1) {
        return Err("term 0".to_owned());
    }
    match &message.command {
        MessageCommand::RequestVote {
            last_log_index,
            last_log_term,
        } => {
            if (*last_log_index == Index(0)) != (*last_log_term == Term(0)) {
                return Err(format!(
                    "last log index {last_log_index} with term {last_log_term}"
                ));
            }
        }
        MessageCommand::AppendEntries {
            prev_index,
            prev_term,
            entries,
            leader_commit: _,
        } => {
            if (*prev_index == Index(0)) != (*prev_term == Term(0)) {
                return Err(format!(
                    "prev_index {prev_index} with prev_term {prev_term}"
                ));
            }
            check_entries(entries, *prev_term, message.term)?;
        }
        MessageCommand::VoteReply | MessageCommand::AppendReply { .. } => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use det::{ClientId, Payload};
    use host::airport::Command;

    use super::*;

    fn entry(term: u64) -> Entry<Request> {
        let command = Command::AdvanceTime { now_ms: 5 };
        let request = Request {
            client: ClientId(1),
            seq: 1,
            command,
        };
        Entry {
            term: Term(term),
            payload: Payload::Command(request),
        }
    }

    fn append(term: u64, prev: (u64, u64), entries: Vec<Entry<Request>>) -> Message<Request> {
        Message::new(
            Term(term),
            MessageCommand::AppendEntries {
                prev_index: Index(prev.0),
                prev_term: Term(prev.1),
                entries,
                leader_commit: Index(0),
            },
        )
    }

    #[test]
    fn frames_survive_the_wire() {
        let frames = [
            Frame::Hello {
                from: NodeId(2),
                cluster: u64::MAX,
            },
            Frame::Message(append(3, (1, 1), vec![entry(2), entry(3)])),
            Frame::Message(Message::new(Term(1), MessageCommand::VoteReply)),
        ];
        let text: String = frames.iter().map(Frame::to_line).collect();
        let mut reader = text.as_bytes();
        for frame in &frames {
            let line = read_line(&mut reader).unwrap().unwrap();
            assert_eq!(serde_json::from_slice::<Frame>(&line).unwrap(), *frame);
        }
        assert_eq!(read_line(&mut reader).unwrap(), None);
    }

    #[test]
    fn a_line_without_its_newline_is_an_error() {
        assert!(read_line(&mut &b"{\"hello\""[..]).is_err());
    }

    #[test]
    fn what_passes_validation_passes_the_nodes_own_check() {
        let good = [
            append(3, (0, 0), vec![]),
            append(3, (0, 0), vec![entry(1), entry(3)]),
            append(3, (4, 2), vec![entry(2), entry(3)]),
            Message::new(Term(1), MessageCommand::VoteReply),
        ];
        for message in good {
            validate(&message).unwrap();
            message.assert_valid();
        }
    }

    #[test]
    fn malformed_messages_are_refused() {
        let vote = |index, term| {
            Message::new(
                Term(2),
                MessageCommand::RequestVote {
                    last_log_index: Index(index),
                    last_log_term: Term(term),
                },
            )
        };
        let bad = [
            Message::new(Term(0), MessageCommand::VoteReply),
            append(3, (0, 1), vec![]),
            append(3, (1, 0), vec![]),
            append(3, (0, 0), vec![entry(0)]),
            append(3, (0, 0), vec![entry(2), entry(1)]),
            append(3, (4, 2), vec![entry(1)]),
            append(3, (0, 0), vec![entry(4)]),
            vote(0, 1),
            vote(1, 0),
        ];
        for message in bad {
            assert!(validate(&message).is_err(), "{message:?}");
        }
    }

    #[test]
    fn the_cluster_fingerprint_covers_members_and_airport() {
        let (demo, single) = (AirportConfig::demo(), AirportConfig::single_runway());
        let abc = [NodeId(1), NodeId(2), NodeId(3)];
        let cba = [NodeId(3), NodeId(2), NodeId(1)];
        assert_eq!(
            cluster_fingerprint(&abc, &demo),
            cluster_fingerprint(&cba, &demo)
        );
        assert_ne!(
            cluster_fingerprint(&abc, &demo),
            cluster_fingerprint(&abc, &single)
        );
        assert_ne!(
            cluster_fingerprint(&abc, &demo),
            cluster_fingerprint(&abc[..2], &demo)
        );
    }
}
