//! [`ProdNode`]: the complete Raft node that real deployments run.
//!
//! It is pure logic behind the same [`Node`] interface as
//! [`BrowserNode`](crate::browser_node::BrowserNode): inputs in, effects out,
//! no clock, network or disk of its own. `ProdHost` (in the `host` crate)
//! supplies those, and must write the node's durable state to disk before it
//! lets any effect of a step leave the machine; [`ProdNode::take_log_change`]
//! tells it which part of the log to write.
//!
//! The rules are those of the Raft paper as restated in tickets B2 to B6 of
//! `design/phase-2-spec.md`. Two things go beyond the simplest version:
//! a leader sends new entries without waiting for the previous reply
//! (`next` is advanced when entries are sent, and pulled back on a refusal),
//! and a refusal says how long the follower's log is so the leader can skip
//! back in one round trip.

use std::collections::{BTreeMap, BTreeSet};

use det::{
    Config, Durable, Effect, Entry, Index, Input, Ms, NodeId, Payload, Rng, Role, Status, Term,
    objects::{Message, MessageCommand},
};

use crate::node::Node;

/// The most entries one `AppendEntries` carries, so that a follower far
/// behind is caught up in bounded messages.
pub const MAX_BATCH: usize = 256;

/// A complete Raft node for real deployments. See the module documentation.
#[derive(Debug)]
pub struct ProdNode<C> {
    // Durable: what `into_durable` returns and the host writes to disk.
    /// The latest election term this node has seen.
    term: Term,
    /// The node voted for in `term`, if any.
    voted_for: Option<NodeId>,
    /// The whole log; position `i` is `log[i - 1]`.
    log: Vec<Entry<C>>,

    // Fixed for the life of the node.
    /// This node.
    id: NodeId,
    /// Every other member of the cluster.
    peers: Vec<NodeId>,
    /// Heartbeat and election timing.
    config: Config,
    /// Source of the random election timeouts.
    rng: Rng,

    // Volatile: lost in a crash.
    /// The time of the last `step`.
    now: Ms,
    /// The part this node plays in `term`.
    role: Role,
    /// The highest position known to be committed.
    commit: Index,
    /// The leader of `term`, once known. A leader names itself.
    leader: Option<NodeId>,
    /// Follower or candidate: when to start an election.
    election_deadline: Option<Ms>,
    /// Leader: when to send the next heartbeat.
    heartbeat_deadline: Option<Ms>,
    /// Candidate: who has voted for this node in `term`, itself included.
    votes: BTreeSet<NodeId>,
    /// Leader: the next position to send to each peer.
    next: BTreeMap<NodeId, Index>,
    /// Leader: the highest position each peer is known to hold.
    matched: BTreeMap<NodeId, Index>,
    /// The lowest log position appended or replaced since the last
    /// [`ProdNode::take_log_change`].
    log_changed_from: Option<Index>,
}

/// A log length or position as an [`Index`].
fn index(position: usize) -> Index {
    Index(u64::try_from(position).expect("log position fits in u64"))
}

/// An [`Index`] as an offset into a `Vec`.
fn offset(position: Index) -> usize {
    usize::try_from(position.0).expect("log position fits in usize")
}

impl<C: Clone + Eq> ProdNode<C> {
    /// This node's identifier.
    #[must_use]
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The other members of the cluster.
    #[must_use]
    pub fn peers(&self) -> &[NodeId] {
        &self.peers
    }

    /// The part this node plays in its current term.
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    /// The latest election term this node has seen. Durable.
    #[must_use]
    pub fn term(&self) -> Term {
        self.term
    }

    /// The node this one voted for in its current term. Durable.
    #[must_use]
    pub fn voted_for(&self) -> Option<NodeId> {
        self.voted_for
    }

    /// The highest position known to be committed.
    #[must_use]
    pub fn commit(&self) -> Index {
        self.commit
    }

    /// The leader of the current term, if this node knows it. A leader names
    /// itself.
    #[must_use]
    pub fn leader(&self) -> Option<NodeId> {
        self.leader
    }

    /// The position of the last entry in the log; 0 when it is empty.
    #[must_use]
    pub fn last_index(&self) -> Index {
        index(self.log.len())
    }

    /// The lowest log position appended or replaced since the last call, if
    /// any. Everything from that position to the end of the log must be
    /// written to disk (after cutting the stored log back to just before it)
    /// before any effect of the step is acted on.
    pub fn take_log_change(&mut self) -> Option<Index> {
        self.log_changed_from.take()
    }

    /// The number of nodes in the cluster.
    fn cluster_size(&self) -> usize {
        self.peers.len() + 1
    }

    /// The number of nodes that make a majority.
    fn majority(&self) -> usize {
        self.cluster_size() / 2 + 1
    }

    /// The term of the entry at `position`; 0 for position 0.
    fn term_at(&self, position: Index) -> Term {
        if position == Index(0) {
            Term(0)
        } else {
            self.log[offset(position) - 1].term
        }
    }

    /// Records that the log changed at `position` and after.
    fn mark_log_changed(&mut self, position: Index) {
        self.log_changed_from = Some(match self.log_changed_from {
            Some(earlier) => earlier.min(position),
            None => position,
        });
    }

    /// A message to `to` carrying this node's term (O7).
    fn send(&self, to: NodeId, command: MessageCommand<C>) -> Effect<C> {
        assert!(
            self.peers.contains(&to),
            "node {}: sending to {to}, which is not a peer",
            self.id
        );
        Effect::Send {
            to,
            message: Message::new(self.term, command),
        }
    }

    /// Sets the election deadline to a fresh random time between one and two
    /// base timeouts from `now`.
    fn reset_election_deadline(&mut self, now: Ms) {
        let Ms(base) = self.config.base_election_timeout;
        self.election_deadline = Some(now + Ms(base + self.rng.below(base + 1)));
    }

    /// Stops being a leader or candidate. The term and vote are untouched.
    fn become_follower(&mut self, now: Ms) {
        let had_deadline = self.role != Role::Leader && self.election_deadline.is_some();
        self.role = Role::Follower;
        self.votes.clear();
        self.next.clear();
        self.matched.clear();
        self.heartbeat_deadline = None;
        if !had_deadline {
            self.reset_election_deadline(now);
        }
    }

    /// Adopts a higher term seen in a message: no vote in it yet, and no
    /// known leader, since the sender may only be a candidate.
    fn adopt_term(&mut self, term: Term, now: Ms) {
        assert!(term > self.term, "node {}: adopting a lower term", self.id);
        self.term = term;
        self.voted_for = None;
        self.leader = None;
        self.become_follower(now);
    }

    /// Starts an election in the next term.
    fn start_election(&mut self, now: Ms, effects: &mut Vec<Effect<C>>) {
        assert_ne!(self.role, Role::Leader, "node {}: leader standing", self.id);
        self.term = Term(self.term.0.checked_add(1).expect("term overflow"));
        self.voted_for = Some(self.id);
        self.role = Role::Candidate;
        self.leader = None;
        self.votes.clear();
        self.votes.insert(self.id);
        self.reset_election_deadline(now);
        let last_log_index = self.last_index();
        let last_log_term = self.term_at(last_log_index);
        for &peer in &self.peers {
            effects.push(self.send(
                peer,
                MessageCommand::RequestVote {
                    last_log_index,
                    last_log_term,
                },
            ));
        }
        if self.votes.len() >= self.majority() {
            self.become_leader(now, effects);
        }
    }

    /// Takes over as leader of the current term and tells everyone.
    fn become_leader(&mut self, now: Ms, effects: &mut Vec<Effect<C>>) {
        assert_eq!(self.role, Role::Candidate, "node {}: N29", self.id);
        assert!(
            self.votes.len() >= self.majority(),
            "node {}: becoming leader without a majority",
            self.id
        );
        self.role = Role::Leader;
        self.leader = Some(self.id);
        self.votes.clear();
        self.election_deadline = None;
        let next = index(self.log.len() + 1);
        self.next = self.peers.iter().map(|&p| (p, next)).collect();
        self.matched = self.peers.iter().map(|&p| (p, Index(0))).collect();
        // An entry of its own term lets entries left from earlier terms commit.
        self.log.push(Entry {
            term: self.term,
            payload: Payload::Noop,
        });
        self.mark_log_changed(next);
        self.send_append_to_all(now, effects);
        self.advance_commit(effects);
    }

    /// Sends `peer` everything from its `next` position (at most
    /// [`MAX_BATCH`] entries, possibly none) and assumes it will arrive.
    fn send_append(&mut self, peer: NodeId, effects: &mut Vec<Effect<C>>) {
        let next = self.next[&peer];
        let prev_index = Index(next.0 - 1);
        let from = offset(prev_index);
        let to = self.log.len().min(from + MAX_BATCH);
        let entries = self.log[from..to].to_vec();
        self.next.insert(peer, index(to + 1));
        effects.push(self.send(
            peer,
            MessageCommand::AppendEntries {
                prev_index,
                prev_term: self.term_at(prev_index),
                entries,
                leader_commit: self.commit,
            },
        ));
    }

    /// Sends every peer what it is missing, or an empty heartbeat, and
    /// schedules the next heartbeat.
    fn send_append_to_all(&mut self, now: Ms, effects: &mut Vec<Effect<C>>) {
        assert_eq!(self.role, Role::Leader, "node {}: N33", self.id);
        for peer in self.peers.clone() {
            self.send_append(peer, effects);
        }
        self.heartbeat_deadline =
            (!self.peers.is_empty()).then(|| now + self.config.heartbeat_interval);
    }

    /// Moves the commit position to `to` and hands over each newly committed
    /// entry, in order (O9).
    fn commit_to(&mut self, to: Index, effects: &mut Vec<Effect<C>>) {
        assert!(to <= self.last_index(), "node {}: N9", self.id);
        while self.commit < to {
            self.commit = Index(self.commit.0 + 1);
            effects.push(Effect::Committed {
                index: self.commit,
                entry: self.log[offset(self.commit) - 1].clone(),
            });
        }
    }

    /// Leader: commits the highest position that a majority holds and that
    /// is from the current term (N30).
    fn advance_commit(&mut self, effects: &mut Vec<Effect<C>>) {
        assert_eq!(self.role, Role::Leader, "node {}: N30", self.id);
        let mut position = self.last_index();
        // Terms never fall along the log, so once an entry is from an
        // earlier term, everything before it is too.
        while position > self.commit && self.term_at(position) == self.term {
            let holders = 1 + self.matched.values().filter(|m| **m >= position).count();
            if holders >= self.majority() {
                self.commit_to(position, effects);
                return;
            }
            position = Index(position.0 - 1);
        }
    }

    /// Handles a message from `from`.
    fn handle_message(
        &mut self,
        now: Ms,
        from: NodeId,
        message: Message<C>,
        effects: &mut Vec<Effect<C>>,
    ) {
        // N19: the host only routes messages between members.
        assert!(
            self.peers.contains(&from),
            "node {}: message from {from}, which is not a peer",
            self.id
        );
        message.assert_valid();

        if message.term > self.term {
            self.adopt_term(message.term, now);
        } else if message.term < self.term {
            // Stale. A request is refused with our term so its sender
            // catches up; a reply is dropped. The refusal is never a
            // `VoteReply`, which always means a granted vote.
            if message.command.is_request() {
                effects.push(self.send(
                    from,
                    MessageCommand::AppendReply {
                        success: false,
                        last_match_index: Index(0),
                    },
                ));
            }
            return;
        }
        // N20
        assert_eq!(message.term, self.term);

        match message.command {
            MessageCommand::RequestVote {
                last_log_index,
                last_log_term,
            } => {
                let ours = self.last_index();
                let up_to_date = (last_log_term, last_log_index) >= (self.term_at(ours), ours);
                if up_to_date && self.voted_for.is_none_or(|v| v == from) {
                    self.voted_for = Some(from);
                    self.reset_election_deadline(now);
                    effects.push(self.send(from, MessageCommand::VoteReply));
                }
            }
            MessageCommand::VoteReply => {
                if self.role == Role::Candidate {
                    self.votes.insert(from);
                    if self.votes.len() >= self.majority() {
                        self.become_leader(now, effects);
                    }
                }
            }
            MessageCommand::AppendEntries {
                prev_index,
                prev_term,
                entries,
                leader_commit,
            } => self.handle_append(
                now,
                from,
                prev_index,
                prev_term,
                entries,
                leader_commit,
                effects,
            ),
            MessageCommand::AppendReply {
                success,
                last_match_index,
            } => {
                if self.role == Role::Leader {
                    self.handle_append_reply(from, success, last_match_index, effects);
                }
            }
        }
    }

    /// Follower side of log copying: the consistency check, repair, and the
    /// follower's commit rule.
    #[expect(clippy::too_many_arguments, reason = "the fields of one message")]
    fn handle_append(
        &mut self,
        now: Ms,
        from: NodeId,
        prev_index: Index,
        prev_term: Term,
        entries: Vec<Entry<C>>,
        leader_commit: Index,
        effects: &mut Vec<Effect<C>>,
    ) {
        // N24: two leaders in one term is a safety violation.
        assert_ne!(
            self.role,
            Role::Leader,
            "two leaders in term {}: {} and {from}",
            self.term,
            self.id
        );
        if self.role == Role::Candidate {
            self.become_follower(now);
        }
        self.leader = Some(from);
        self.reset_election_deadline(now);

        let last = self.last_index();
        if prev_index > last || self.term_at(prev_index) != prev_term {
            // Refuse, and say where the logs might still agree.
            let hint = last.min(Index(prev_index.0 - 1));
            effects.push(self.send(
                from,
                MessageCommand::AppendReply {
                    success: false,
                    last_match_index: hint,
                },
            ));
            return;
        }

        let covered = index(offset(prev_index) + entries.len());
        for (i, entry) in entries.into_iter().enumerate() {
            let position = index(offset(prev_index) + i + 1);
            if position <= self.last_index() {
                if self.term_at(position) == entry.term {
                    continue;
                }
                // N25: a committed entry is never removed.
                assert!(
                    position > self.commit,
                    "node {}: asked to replace committed position {position}",
                    self.id
                );
                self.log.truncate(offset(position) - 1);
            }
            self.log.push(entry);
            self.mark_log_changed(position);
        }
        // N26: never past what this message vouches for.
        let commit = self.commit.max(leader_commit.min(covered));
        self.commit_to(commit, effects);
        effects.push(self.send(
            from,
            MessageCommand::AppendReply {
                success: true,
                last_match_index: covered,
            },
        ));
    }

    /// Leader side of log copying: what `from` holds, and what to send next.
    fn handle_append_reply(
        &mut self,
        from: NodeId,
        success: bool,
        last_match_index: Index,
        effects: &mut Vec<Effect<C>>,
    ) {
        let matched = self.matched[&from];
        if success {
            // O6: a follower cannot hold more of this term's log than we do.
            assert!(
                last_match_index <= self.last_index(),
                "node {}: {from} matched {last_match_index}, beyond the log",
                self.id
            );
            // Only ever upwards, so late or repeated replies do nothing.
            let matched = matched.max(last_match_index);
            self.matched.insert(from, matched);
            let next = self.next[&from].max(Index(matched.0 + 1));
            self.next.insert(from, next);
            self.advance_commit(effects);
            if next <= self.last_index() {
                self.send_append(from, effects);
            }
        } else {
            // Step back to where the follower says the logs may agree, but
            // never below what it is known to hold.
            let retry = Index(last_match_index.0.saturating_add(1));
            let next = self.next[&from].min(retry).max(Index(matched.0 + 1));
            self.next.insert(from, next);
            self.send_append(from, effects);
        }
    }

    /// Checks everything that holds whenever the node is at rest (N1 to N11
    /// in `design/invariants.md`).
    #[cfg(any(test, debug_assertions, feature = "paranoid"))]
    fn check_invariants(&self) {
        let id = self.id;
        // N1
        assert!(!self.peers.contains(&id), "node {id}: its own peer");
        let distinct: BTreeSet<_> = self.peers.iter().collect();
        assert_eq!(distinct.len(), self.peers.len(), "node {id}: repeated peer");
        // N3, N4
        if self.role == Role::Leader {
            assert_eq!(
                self.heartbeat_deadline.is_some(),
                !self.peers.is_empty(),
                "node {id}: leader heartbeat deadline"
            );
            assert!(self.election_deadline.is_none());
        } else {
            assert!(self.election_deadline.is_some(), "node {id}: no election");
            assert!(self.heartbeat_deadline.is_none());
        }
        // N5, N6, N7
        if self.role == Role::Follower {
            assert!(self.next.is_empty() && self.matched.is_empty());
        } else {
            assert_eq!(self.voted_for, Some(id), "node {id}: N5/N6");
        }
        if self.role == Role::Candidate {
            assert!(self.votes.contains(&id), "node {id}: N5");
        } else {
            assert!(self.votes.is_empty(), "node {id}: N7");
        }
        // N8, N9
        det::objects::assert_log_shape(&self.log, self.term);
        assert!(self.commit <= self.last_index(), "node {id}: N9");
        // N10
        if self.role == Role::Leader {
            for peer in &self.peers {
                let (next, matched) = (self.next[peer], self.matched[peer]);
                assert!(
                    matched < next && next.0 <= self.last_index().0 + 1,
                    "node {id}: N10 for {peer}: matched {matched}, next {next}"
                );
            }
        }
        // N11
        assert_eq!(
            self.leader == Some(id),
            self.role == Role::Leader,
            "node {id}: N11"
        );
    }
}

impl<C: Clone + Eq> Node<C> for ProdNode<C> {
    fn new(
        id: NodeId,
        peers: Vec<NodeId>,
        config: Config,
        durable: Durable<C>,
        rng: Rng,
        now: Ms,
    ) -> Self {
        // C1, C2, O1, O2, O3
        config.assert_valid();
        durable.assert_valid();
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
            role: Role::Follower,
            commit: Index(0),
            leader: None,
            election_deadline: None,
            heartbeat_deadline: None,
            votes: BTreeSet::new(),
            next: BTreeMap::new(),
            matched: BTreeMap::new(),
            log_changed_from: None,
        };
        // Wait a full timeout before standing, so a live leader is heard first.
        node.reset_election_deadline(now);
        #[cfg(any(test, debug_assertions, feature = "paranoid"))]
        node.check_invariants();
        node
    }

    fn step(&mut self, now: Ms, input: Input<C>) -> Vec<Effect<C>> {
        // N12
        assert!(
            now >= self.now,
            "node {}: time went back from {} to {now}",
            self.id,
            self.now
        );
        self.now = now;
        #[cfg(any(test, debug_assertions, feature = "paranoid"))]
        let before = (self.term, self.voted_for, self.commit);

        let mut effects = Vec::new();

        // Time first, on every input, so a late `Wake` cannot hide an expired
        // timer and an early one is harmless.
        if self.role == Role::Leader {
            if self.heartbeat_deadline.is_some_and(|d| now >= d) {
                self.send_append_to_all(now, &mut effects);
            }
        } else if self.election_deadline.is_some_and(|d| now >= d) {
            self.start_election(now, &mut effects);
        }

        match input {
            Input::Wake => {}
            Input::Message { from, message } => {
                self.handle_message(now, from, message, &mut effects);
            }
            Input::Propose(command) => {
                if self.role == Role::Leader {
                    self.log.push(Entry {
                        term: self.term,
                        payload: Payload::Command(command),
                    });
                    self.mark_log_changed(self.last_index());
                    self.send_append_to_all(now, &mut effects);
                    // A cluster of one is its own majority.
                    self.advance_commit(&mut effects);
                } else {
                    effects.push(Effect::NotLeader { hint: self.leader });
                }
            }
        }

        #[cfg(any(test, debug_assertions, feature = "paranoid"))]
        {
            self.check_invariants();
            let (term, voted_for, commit) = before;
            // N13, N14, N16
            assert!(self.term >= term, "node {}: term went down", self.id);
            if self.term == term && voted_for.is_some() {
                assert_eq!(self.voted_for, voted_for, "node {}: N14", self.id);
            }
            assert!(self.commit >= commit, "node {}: commit went down", self.id);
        }
        effects
    }

    fn next_wake(&self) -> Option<Ms> {
        match self.role {
            Role::Leader => self.heartbeat_deadline,
            Role::Follower | Role::Candidate => self.election_deadline,
        }
    }

    fn into_durable(self) -> Durable<C> {
        Durable::new(self.term, self.voted_for, self.log)
    }

    fn status(&self) -> Status {
        Status::Healthy
    }

    fn log(&self) -> &[Entry<C>] {
        &self.log
    }
}
