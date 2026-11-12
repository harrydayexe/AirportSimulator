//! An implementation of [`Node`], designed to run in one browser WASM program.
use std::collections::BTreeSet;

use det::{
    Config, Durable, Effect, Entry, Input, Ms, NodeId, Rng, Role, Status, Term,
    objects::{Message, MessageCommand},
};

use crate::node::Node;

/// An implementation of [`Node`], designed to run in one browser WASM program.
/// >**Warning:** this is not intended for production deployment. Demo purposes only.
struct BrowserNode<C: Clone + Eq> {
    // Durable
    term: Term,
    voted_for: Option<NodeId>,
    log: Vec<Entry<C>>,

    // Inherent
    id: NodeId,
    peers: Vec<NodeId>,
    config: Config,
    rng: Rng,
    now: Ms,
    role: Role,

    // Variable
    leader_hint: Option<NodeId>,
    heartbeat_deadline: Option<Ms>,
    election_deadline: Option<Ms>,
    last_reset_time: Option<Ms>,
    next_wake: Option<Ms>,
    status: Status,
    votes: BTreeSet<NodeId>,
}
impl<C: Clone + Eq> BrowserNode<C> {
    /// Returns the number of nodes in the cluster.
    fn cluster_size(&self) -> usize {
        self.peers.len() + 1
    }

    /// Returns the number of votes needed for a majority.
    fn majority(&self) -> usize {
        let cs = self.cluster_size();
        assert_ne!(cs, 0, "cluster size can never be 0");
        cs / 2 + 1
    }

    /// Reset the heartbeat deadline to now + `config.heartbeat_interval`.
    fn reset_heartbeat_deadline(&mut self, now: Ms) {
        self.heartbeat_deadline = Some(now + self.config.heartbeat_interval);
    }
    fn send_append_to_all(&self) {
        unimplemented!("please implement send_append_to_all");
    }

    fn start_election(&self, _time: Ms) {
        unimplemented!("please implement start_election");
    }

    fn append_entry(&mut self, _term: Term, _command: C) {
        unimplemented!("Please implement append_entry");
    }

    fn send_refusal(&mut self, _to: NodeId, _term: Term) {
        unimplemented!("Please implement send_refusal");
    }

    fn become_leader(&mut self) {
        unimplemented!("Please implement become_leader");
    }

    fn handle_message(&mut self, from: NodeId, message: Message<C>) -> Vec<Effect<C>> {
        if message.term > self.term {
            self.term = message.term;
            self.voted_for = None;
            self.role = Role::Follower;
            self.leader_hint = Some(from);
        } else if message.term < self.term {
            // Message is stale
            // Requests get a refusal which carries our term. Replies are dropped.
            if message.command.is_request() {
                self.send_refusal(from, self.term);
                return vec![];
            }
        }

        let mut effects = vec![];
        match message.command {
            MessageCommand::RequestVote => {
                if self.voted_for.is_none_or(|v| v == from) {
                    let response_message: Message<C> =
                        Message::new(self.term, MessageCommand::VoteReply);
                    effects.push(Effect::Send {
                        to: from,
                        message: response_message,
                    });
                    self.voted_for = Some(from);
                }
            }
            MessageCommand::VoteReply => {
                if self.role == Role::Candidate {
                    self.votes.insert(from);
                    if self.votes.len() >= self.majority() {
                        self.become_leader();
                    }
                }
            }
            MessageCommand::AppendEntries {
                prev_index: _,
                prev_term: _,
                entries: _,
            } => todo!(),
            MessageCommand::AppendReply {
                success: _,
                last_match_index: _,
            } => todo!(),
            _ => todo!(),
        }

        effects
    }
}

impl<C: Clone + Eq> Node<C> for BrowserNode<C> {
    /// A follower in the term `durable` records, with its vote and log. `peers`
    /// are the other nodes; `rng` drives its random election timeouts.
    fn new(
        id: NodeId,
        peers: Vec<NodeId>,
        config: Config,
        durable: Durable<C>,
        rng: Rng,
        now: Ms,
    ) -> Self {
        Self {
            term: durable.term,
            voted_for: durable.voted_for,
            log: durable.log,
            id,
            peers,
            config,
            rng,
            now,
            leader_hint: None,
            heartbeat_deadline: None,
            election_deadline: None,
            role: Role::Follower,
            last_reset_time: None,
            next_wake: None,
            status: Status::Healthy,
            votes: BTreeSet::new(),
        }
    }

    /// Handle one input at simulated time `now` and return what the host
    /// should do as a result.
    fn step(&mut self, now: Ms, input: Input<C>) -> Vec<Effect<C>> {
        let mut effects: Vec<Effect<C>> = vec![];

        // Handle tim first, a wake that is early or late is harmless as we only compare agains our
        // own deadlines. Checking on every input, not just [`Input::Wake`], means a late wake can
        // never hide an expired timer.
        if self.role == Role::Leader && self.heartbeat_deadline.is_some_and(|d| now >= d) {
            self.send_append_to_all();
            self.reset_heartbeat_deadline(now);
        } else if self.role != Role::Leader && self.election_deadline.is_some_and(|d| now >= d) {
            self.start_election(now);
        }

        match input {
            Input::Wake => todo!(),
            Input::Message { from, message } => effects.extend(self.handle_message(from, message)),
            Input::Propose(command) => {
                if self.role == Role::Leader {
                    self.append_entry(self.term, command);
                    self.send_append_to_all();
                    self.reset_heartbeat_deadline(now);
                } else {
                    effects.push(Effect::NotLeader {
                        hint: self.leader_hint,
                    });
                }
            }
            _ => todo!("What happens if the enum doesn't match (ie due to a version mismatch)"),
        }

        effects
    }

    /// When the host should send the next `Wake`.
    fn next_wake(&self) -> Option<Ms> {
        self.next_wake
    }

    /// What is left after a crash.
    fn into_durable(self) -> Durable<C> {
        Durable::new(self.term, self.voted_for, self.log)
    }

    /// Role, term and positions, for checkers and the web view.
    fn status(&self) -> Status {
        self.status
    }

    /// The whole log, read-only, for checkers.
    fn log(&self) -> &[Entry<C>] {
        &self.log
    }
}
