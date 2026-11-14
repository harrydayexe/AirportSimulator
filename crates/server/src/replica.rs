//! The replica's state machine: one [`Airport`] and the table that makes
//! every client request change it exactly once (Phase 2 spec, D1 and D2).
//!
//! Everything here is decided by committed log entries alone, so every
//! replica that has applied the same position is in the same state.

use std::collections::BTreeMap;

use det::{ClientId, Entry, Index, Payload, fingerprint};
use host::airport::{Airport, AirportConfig, Command, ConfigError, Output, Snapshot};
use serde::{Deserialize, Serialize};

/// What travels through Raft: a command, who sent it, and which of that
/// client's commands it is. A client numbers its commands 1, 2, 3, ... and
/// resends with the same number until it has an answer.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Request {
    /// Who sent the command.
    pub client: ClientId,
    /// Which of the client's commands this is, counting from 1.
    pub seq: u64,
    /// The command for the airport.
    pub command: Command,
}

/// What became of a request when its log entry was applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It was applied (now, or when an earlier copy of it was), and these
    /// are the airport's outputs.
    Done(Vec<Output>),
    /// The client has already moved on to a later command.
    Stale {
        /// The latest command applied for this client.
        latest: u64,
    },
    /// The client skipped a number. Nothing was applied.
    Gap {
        /// The number the client's next command must carry.
        expected: u64,
    },
}

/// One client's row: its latest applied command and what that produced.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Row {
    /// The highest `seq` applied for the client.
    seq: u64,
    /// The outputs of applying it, kept to answer a resend.
    outputs: Vec<Output>,
}

/// An airport fed by committed entries, with the duplicate table.
#[derive(Debug)]
pub struct Replica {
    /// The airport. Only [`Replica::apply`] changes it.
    airport: Airport,
    /// The last log position applied.
    applied: Index,
    /// The duplicate table, one row per client.
    table: BTreeMap<ClientId, Row>,
}

impl Replica {
    /// A replica that has applied nothing.
    ///
    /// # Errors
    /// If the airport configuration is invalid.
    pub fn new(config: AirportConfig) -> Result<Self, ConfigError> {
        Ok(Self {
            airport: Airport::new(config)?,
            applied: Index(0),
            table: BTreeMap::new(),
        })
    }

    /// The last log position applied.
    #[must_use]
    pub fn applied(&self) -> Index {
        self.applied
    }

    /// A hash of the whole replicated state: the airport and the table.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        fingerprint(&(self.airport.fingerprint(), &self.table))
    }

    /// The airport as it stands on this replica.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        self.airport.snapshot()
    }

    /// What is already known about request `seq` of `client` without going
    /// through the log: its outcome if it (or a later one) has been applied.
    #[must_use]
    pub fn known(&self, client: ClientId, seq: u64) -> Option<Outcome> {
        let row = self.table.get(&client)?;
        match seq.cmp(&row.seq) {
            std::cmp::Ordering::Less => Some(Outcome::Stale { latest: row.seq }),
            std::cmp::Ordering::Equal => Some(Outcome::Done(row.outputs.clone())),
            std::cmp::Ordering::Greater => None,
        }
    }

    /// Applies the committed entry at `index`. Returns the request it
    /// carried and what became of it, or `None` for a leader's empty entry.
    ///
    /// # Panics
    /// If `index` is not the position after the last one applied (H9). The
    /// node hands entries over in order, so that is a bug.
    pub fn apply(
        &mut self,
        index: Index,
        entry: &Entry<Request>,
    ) -> Option<(ClientId, u64, Outcome)> {
        assert_eq!(
            index.0,
            self.applied.0 + 1,
            "replica: applying out of order"
        );
        self.applied = index;
        let Payload::Command(request) = &entry.payload else {
            return None;
        };
        let latest = self.table.get(&request.client).map_or(0, |row| row.seq);
        let outcome = if request.seq == latest.saturating_add(1) {
            let outputs = self.airport.apply(&request.command);
            let row = Row {
                seq: request.seq,
                outputs: outputs.clone(),
            };
            self.table.insert(request.client, row);
            Outcome::Done(outputs)
        } else if request.seq == latest {
            // A resend that was stored twice. `latest` is at least 1 here.
            Outcome::Done(self.table[&request.client].outputs.clone())
        } else if request.seq < latest {
            Outcome::Stale { latest }
        } else {
            // A client is outside the cluster, so its mistake is refused
            // (the same way on every replica), not asserted.
            Outcome::Gap {
                expected: latest.saturating_add(1),
            }
        };
        Some((request.client, request.seq, outcome))
    }
}

#[cfg(test)]
mod tests {
    use det::Term;
    use host::airport::RejectReason;

    use super::*;

    fn landing(client: u32, seq: u64, flight: &str) -> Entry<Request> {
        Entry {
            term: Term(1),
            payload: Payload::Command(Request {
                client: ClientId(client),
                seq,
                command: Command::RequestLanding {
                    flight: flight.into(),
                    aircraft: "A320".into(),
                    earliest_ms: 0,
                },
            }),
        }
    }

    fn replica() -> Replica {
        Replica::new(AirportConfig::demo()).unwrap()
    }

    #[test]
    fn a_request_stored_twice_changes_the_airport_once() {
        let mut r = replica();
        let first = r.apply(Index(1), &landing(1, 1, "BAW1")).unwrap();
        let state = r.fingerprint();
        let second = r.apply(Index(2), &landing(1, 1, "BAW1")).unwrap();
        assert_eq!(first, second, "the resend gets the first answer");
        let (_, _, Outcome::Done(outputs)) = second else {
            panic!("{second:?}");
        };
        assert!(
            matches!(outputs[..], [Output::Accepted { .. }]),
            "{outputs:?}"
        );
        assert_eq!(r.fingerprint(), state, "and changes nothing");
        assert_eq!(r.snapshot().stats.rejected, 0, "no DuplicateFlight");
        assert_eq!(r.applied(), Index(2));
    }

    #[test]
    fn without_the_table_the_resend_would_have_been_rejected() {
        let mut r = replica();
        r.apply(Index(1), &landing(1, 1, "BAW1"));
        let (_, _, Outcome::Done(outputs)) = r.apply(Index(2), &landing(2, 1, "BAW1")).unwrap()
        else {
            panic!();
        };
        assert!(matches!(
            outputs[..],
            [Output::Rejected {
                reason: RejectReason::DuplicateFlight,
                ..
            }]
        ));
    }

    #[test]
    fn stale_and_skipped_numbers_are_refused_and_change_nothing() {
        let mut r = replica();
        r.apply(Index(1), &landing(1, 1, "A1"));
        r.apply(Index(2), &landing(1, 2, "A2"));
        let state = r.fingerprint();
        let old = r.apply(Index(3), &landing(1, 1, "A1")).unwrap();
        assert_eq!(old.2, Outcome::Stale { latest: 2 });
        let gap = r.apply(Index(4), &landing(1, 4, "A4")).unwrap();
        assert_eq!(gap.2, Outcome::Gap { expected: 3 });
        let first = r.apply(Index(5), &landing(7, 2, "B2")).unwrap();
        assert_eq!(
            first.2,
            Outcome::Gap { expected: 1 },
            "a new client starts at 1"
        );
        assert_eq!(r.fingerprint(), state);
        assert_eq!(r.known(ClientId(1), 1), Some(Outcome::Stale { latest: 2 }));
        assert!(matches!(r.known(ClientId(1), 2), Some(Outcome::Done(_))));
        assert_eq!(r.known(ClientId(1), 3), None);
        assert_eq!(r.known(ClientId(9), 1), None);
    }

    #[test]
    fn an_empty_entry_moves_the_position_only() {
        let mut r = replica();
        let state = r.fingerprint();
        let noop = Entry {
            term: Term(1),
            payload: Payload::Noop,
        };
        assert_eq!(r.apply(Index(1), &noop), None);
        assert_eq!((r.applied(), r.fingerprint()), (Index(1), state));
    }

    #[test]
    fn two_replicas_given_the_same_log_agree() {
        let log = [
            landing(1, 1, "A1"),
            landing(2, 1, "B1"),
            landing(1, 1, "A1"),
            landing(1, 2, "A2"),
        ];
        let (mut a, mut b) = (replica(), replica());
        for (i, entry) in log.iter().enumerate() {
            let index = Index(u64::try_from(i).unwrap() + 1);
            assert_eq!(a.apply(index, entry), b.apply(index, entry));
            assert_eq!(a.fingerprint(), b.fingerprint());
        }
        assert_ne!(a.fingerprint(), replica().fingerprint());
    }

    #[test]
    #[should_panic(expected = "out of order")]
    fn applying_out_of_order_is_a_bug() {
        replica().apply(Index(2), &landing(1, 1, "A1"));
    }
}
