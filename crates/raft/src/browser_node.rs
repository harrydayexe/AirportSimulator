//! An implementation of [`Node`], designed to run in one browser WASM program.
use std::collections::BTreeSet;

use det::{
    Config, Durable, Effect, Entry, Input, Ms, NodeId, Rng, Role, Status, Term,
    objects::{Message, MessageCommand},
};

use crate::node::Node;

/// An implementation of [`Node`], designed to run in one browser WASM program.
/// >**Warning:** this is not intended for production deployment. Demo purposes only.
pub struct BrowserNode<C: Clone + Eq> {
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
    status: Status,
    votes: BTreeSet<NodeId>,
}

/// The parts of a node one `step` is compared against (N13 to N15).
#[cfg(any(test, debug_assertions, feature = "paranoid"))]
struct Before {
    term: Term,
    voted_for: Option<NodeId>,
    role: Role,
    log_len: usize,
    last_term: Option<Term>,
}

impl<C: Clone + Eq> BrowserNode<C> {
    /// Returns the number of nodes in the cluster.
    fn cluster_size(&self) -> usize {
        self.peers.len() + 1
    }

    /// Returns the number of votes needed for a majority.
    fn majority(&self) -> usize {
        self.cluster_size() / 2 + 1
    }

    /// Whether `node` is one of the other members of the cluster.
    fn is_peer(&self, node: NodeId) -> bool {
        self.peers.contains(&node)
    }

    /// Reset the heartbeat deadline to now + `config.heartbeat_interval`.
    fn reset_heartbeat_deadline(&mut self, now: Ms) {
        assert_eq!(
            self.role,
            Role::Leader,
            "node {}: heartbeat deadline set while not leader",
            self.id
        );
        self.heartbeat_deadline = Some(now + self.config.heartbeat_interval);
    }

    /// Sets the election deadline to a fresh random time in
    /// `now + base ..= now + 2 * base` (spec 10.5).
    fn reset_election_deadline(&mut self, now: Ms) {
        let Ms(base) = self.config.base_election_timeout;
        self.election_deadline = Some(now + Ms(base + self.rng.below(base + 1)));
    }

    /// A message to `to` carrying this node's term (O7).
    fn send(&self, to: NodeId, command: MessageCommand<C>) -> Effect<C> {
        assert_ne!(to, self.id, "node {}: sending to itself", self.id);
        assert!(
            self.is_peer(to),
            "node {}: sending to stranger {to}",
            self.id
        );
        Effect::Send {
            to,
            message: Message::new(self.term, command),
        }
    }

    /// Adopts a higher term seen in a message: no vote yet in it, and no
    /// known leader, since the sender may only be a candidate (N21).
    fn adopt_term(&mut self, term: Term, now: Ms) {
        assert!(term > self.term, "node {}: adopting a lower term", self.id);
        let was_leader = self.role == Role::Leader;
        self.term = term;
        self.voted_for = None;
        self.role = Role::Follower;
        self.leader_hint = None;
        self.votes.clear();
        self.heartbeat_deadline = None;
        if was_leader || self.election_deadline.is_none() {
            self.reset_election_deadline(now);
        }
    }

    fn send_append_to_all(&self) {
        assert_eq!(self.role, Role::Leader, "node {}: N33", self.id);
        unimplemented!("please implement send_append_to_all");
    }

    // TODO(N28) POST: term == old + 1, role == Candidate, voted_for == Some(id),
    // votes == {id}, and now + base <= election_deadline <= now + 2 * base.
    fn start_election(&self, _time: Ms) {
        assert_ne!(self.role, Role::Leader, "node {}: N28", self.id);
        unimplemented!("please implement start_election");
    }

    // TODO(N31) POST: log.len() == old + 1 and the new entry's term is self.term.
    fn append_entry(&mut self, _term: Term, _command: C) {
        assert_eq!(self.role, Role::Leader, "node {}: N31", self.id);
        unimplemented!("Please implement append_entry");
    }

    // TODO(N34): also assert the message being refused was a request.
    fn send_refusal(&mut self, _to: NodeId, term: Term) {
        assert_eq!(term, self.term, "node {}: N34", self.id);
        unimplemented!("Please implement send_refusal");
    }

    // TODO(N29) POST: the last log entry is a Noop with term == self.term,
    // election_deadline == None, every matched[p] == 0 and every next[p] is
    // the position of that Noop.
    fn become_leader(&mut self) {
        assert_eq!(self.role, Role::Candidate, "node {}: N29", self.id);
        assert!(
            self.votes.len() >= self.majority(),
            "node {}: becoming leader without a majority",
            self.id
        );
        unimplemented!("Please implement become_leader");
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "AppendEntries will move its entries into the log once written"
    )]
    fn handle_message(&mut self, now: Ms, from: NodeId, message: Message<C>) -> Vec<Effect<C>> {
        // N19: a message from a stranger or from itself is a host routing bug.
        assert_ne!(from, self.id, "node {}: message from itself", self.id);
        assert!(
            self.is_peer(from),
            "node {}: message from stranger {from}",
            self.id
        );
        message.assert_valid();

        if message.term > self.term {
            self.adopt_term(message.term, now);
        } else if message.term < self.term {
            // Message is stale
            // Requests get a refusal which carries our term. Replies are dropped.
            if message.command.is_request() {
                self.send_refusal(from, self.term);
            }
            return vec![];
        }
        // N20: higher terms were adopted and lower ones answered or dropped.
        assert_eq!(message.term, self.term);

        let mut effects = vec![];
        match message.command {
            MessageCommand::RequestVote { .. } => {
                // TODO: also require the candidate's log to be at least as up
                // to date as ours (spec 10.4).
                if self.voted_for.is_none_or(|v| v == from) {
                    self.voted_for = Some(from);
                    self.reset_election_deadline(now);
                    effects.push(self.send(from, MessageCommand::VoteReply));
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
            MessageCommand::AppendEntries { .. } => {
                // N24: two leaders in one term is a safety violation.
                assert_ne!(
                    self.role,
                    Role::Leader,
                    "two leaders in term {}: {} and {from}",
                    self.term,
                    self.id
                );
                // TODO(N25): before deleting a conflicting suffix starting at
                // position i, assert i > commit.
                todo!()
            }
            MessageCommand::AppendReply { .. } => todo!(),
        }

        effects
    }

    /// Checks everything that holds whenever the node is at rest (spec
    /// section 10; N1 to N11 in `design/invariants.md`).
    #[cfg(any(test, debug_assertions, feature = "paranoid"))]
    fn check_invariants(&self) {
        let id = self.id;
        // N1: `cluster_size` and `majority` rely on this.
        assert!(!self.is_peer(id), "node {id}: listed among its own peers");
        let distinct: BTreeSet<_> = self.peers.iter().collect();
        assert_eq!(distinct.len(), self.peers.len(), "node {id}: repeated peer");
        // N2
        let (majority, size) = (self.majority(), self.cluster_size());
        assert!(2 * majority > size && 2 * (majority - 1) <= size);
        // N3, N4
        match self.role {
            Role::Leader => {
                assert!(
                    self.heartbeat_deadline.is_some() || self.peers.is_empty(),
                    "node {id}: leader without a heartbeat deadline"
                );
                assert!(self.election_deadline.is_none());
                assert_eq!(self.next_wake(), self.heartbeat_deadline);
            }
            Role::Follower | Role::Candidate => {
                assert!(
                    self.election_deadline.is_some(),
                    "node {id}: {:?} without an election deadline",
                    self.role
                );
                assert!(self.heartbeat_deadline.is_none());
                assert_eq!(self.next_wake(), self.election_deadline);
            }
        }
        // N5, N6, N7
        match self.role {
            Role::Candidate => {
                assert_eq!(self.voted_for, Some(id), "node {id}: candidate N5");
                assert!(self.votes.contains(&id), "node {id}: candidate N5");
                assert!(
                    self.votes.iter().all(|v| *v == id || self.is_peer(*v)),
                    "node {id}: vote from a stranger in {:?}",
                    self.votes
                );
            }
            Role::Leader => {
                assert_eq!(self.voted_for, Some(id), "node {id}: leader N6");
            }
            Role::Follower => {}
        }
        if self.role != Role::Candidate {
            assert!(
                self.votes.is_empty(),
                "node {id}: votes kept after election"
            );
        }
        // N8
        det::objects::assert_log_shape(&self.log, self.term);
        // N11
        if self.role != Role::Leader {
            assert_ne!(self.leader_hint, Some(id), "node {id}: names itself leader");
        }
        // TODO(N9, N10) once `commit`, `next` and `matched` exist.
    }

    #[cfg(any(test, debug_assertions, feature = "paranoid"))]
    fn before(&self) -> Before {
        Before {
            term: self.term,
            voted_for: self.voted_for,
            role: self.role,
            log_len: self.log.len(),
            last_term: self.log.last().map(|e| e.term),
        }
    }

    /// Checks what one `step` may change (N13 to N15).
    #[cfg(any(test, debug_assertions, feature = "paranoid"))]
    fn check_step(&self, before: &Before) {
        let id = self.id;
        // N13
        assert!(self.term >= before.term, "node {id}: term went down");
        // N14
        if self.term == before.term && before.voted_for.is_some() {
            assert_eq!(
                self.voted_for, before.voted_for,
                "node {id}: second vote in term {}",
                self.term
            );
        }
        // N15
        if before.role == Role::Leader && self.role == Role::Leader && self.term == before.term {
            assert!(
                self.log.len() >= before.log_len,
                "node {id}: leader log shrank"
            );
            if let Some(last_term) = before.last_term {
                assert_eq!(
                    self.log[before.log_len - 1].term,
                    last_term,
                    "node {id}: leader rewrote its log"
                );
            }
        }
        // TODO(N16) once `commit` exists: it never decreases.
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
        // C1, C2, O1, O2
        config.assert_valid();
        durable.assert_valid();
        // O3
        if let Some(v) = durable.voted_for {
            assert!(
                v == id || peers.contains(&v),
                "node {id}: durable vote for stranger {v}"
            );
        }
        let mut node = Self {
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
            status: Status::Healthy,
            votes: BTreeSet::new(),
        };
        // Spec 10.7: wait a full timeout before standing for election.
        node.reset_election_deadline(now);
        #[cfg(any(test, debug_assertions, feature = "paranoid"))]
        node.check_invariants();
        node
    }

    /// Handle one input at simulated time `now` and return what the host
    /// should do as a result.
    fn step(&mut self, now: Ms, input: Input<C>) -> Vec<Effect<C>> {
        // N12: time never goes backwards for a node.
        assert!(
            now >= self.now,
            "node {}: time went back from {} to {now}",
            self.id,
            self.now
        );
        self.now = now;
        #[cfg(any(test, debug_assertions, feature = "paranoid"))]
        let before = self.before();

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
            // N35: the deadline check above is all a wake is for.
            Input::Wake => {}
            Input::Message { from, message } => {
                effects.extend(self.handle_message(now, from, message));
            }
            Input::Propose(command) => {
                if self.role == Role::Leader {
                    self.append_entry(self.term, command);
                    self.send_append_to_all();
                    self.reset_heartbeat_deadline(now);
                } else {
                    // O8
                    assert_ne!(self.leader_hint, Some(self.id));
                    effects.push(Effect::NotLeader {
                        hint: self.leader_hint,
                    });
                }
            }
        }

        #[cfg(any(test, debug_assertions, feature = "paranoid"))]
        {
            self.check_invariants();
            self.check_step(&before);
        }
        effects
    }

    /// When the host should send the next `Wake`: a pure read of the
    /// deadline for the current role (spec 10.6).
    fn next_wake(&self) -> Option<Ms> {
        match self.role {
            Role::Leader => self.heartbeat_deadline,
            Role::Follower | Role::Candidate => self.election_deadline,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn node(term: u64, voted_for: Option<u32>) -> BrowserNode<u32> {
        BrowserNode::new(
            NodeId(1),
            vec![NodeId(2), NodeId(3)],
            Config::default(),
            Durable::new(Term(term), voted_for.map(NodeId), vec![]),
            Rng::new(7),
            Ms(1_000),
        )
    }

    fn message(from: u32, term: u64, command: MessageCommand<u32>) -> Input<u32> {
        Input::Message {
            from: NodeId(from),
            message: Message::new(Term(term), command),
        }
    }

    fn request_vote() -> MessageCommand<u32> {
        MessageCommand::RequestVote {
            last_log_index: det::Index(0),
            last_log_term: Term(0),
        }
    }

    #[test]
    fn a_new_node_has_an_election_deadline_to_wake_for() {
        let n = node(0, None);
        let wake = n.next_wake().expect("N3/N4: a new node must wake");
        assert!(wake >= Ms(1_150) && wake <= Ms(1_300), "{wake:?}");
        assert_eq!(n.next_wake(), n.next_wake());
    }

    #[test]
    fn wake_before_the_deadline_does_nothing() {
        let mut n = node(0, None);
        assert!(n.step(Ms(1_001), Input::Wake).is_empty());
        assert_eq!(n.role, Role::Follower);
    }

    #[test]
    fn a_stale_reply_is_dropped() {
        let mut n = node(5, Some(1));
        n.role = Role::Candidate;
        n.votes.insert(NodeId(1));
        assert!(
            n.step(Ms(1_001), message(2, 3, MessageCommand::VoteReply))
                .is_empty()
        );
        assert_eq!(
            n.votes.len(),
            1,
            "N20: a vote from term 3 counted in term 5"
        );
    }

    #[test]
    fn a_higher_term_names_no_leader() {
        let mut n = node(1, None);
        let effects = n.step(Ms(1_001), message(2, 4, request_vote()));
        assert_eq!(n.term, Term(4));
        assert_eq!(n.leader_hint, None, "N21: the sender is only a candidate");
        assert_eq!(n.voted_for, Some(NodeId(2)));
        assert_eq!(
            effects,
            [Effect::Send {
                to: NodeId(2),
                message: Message::new(Term(4), MessageCommand::VoteReply)
            }]
        );
    }

    #[test]
    fn one_vote_per_term() {
        let mut n = node(4, Some(2));
        assert!(n.step(Ms(1_001), message(3, 4, request_vote())).is_empty());
        assert_eq!(n.voted_for, Some(NodeId(2)));
    }

    #[test]
    #[should_panic(expected = "stranger")]
    fn a_message_from_a_stranger_is_a_routing_bug() {
        let mut n = node(1, None);
        let _ = n.step(Ms(1_001), message(99, 1, MessageCommand::VoteReply));
    }

    #[test]
    #[should_panic(expected = "time went back")]
    fn time_never_goes_backwards() {
        let mut n = node(0, None);
        let _ = n.step(Ms(999), Input::Wake);
    }

    #[test]
    #[should_panic(expected = "stranger")]
    fn a_durable_vote_names_a_cluster_member() {
        let _ = node(2, Some(9));
    }
}
