//! [`ProdHost`]: the host for one real replica on one machine.
//!
//! It is the same shape as the simulated hosts: it never decides what time
//! it is, never opens a socket, and hands everything it cannot do itself up
//! to its owner as a [`Report`]. What differs is what the owner is. In a
//! simulation the owner is another host with a fake clock; in production it
//! is the `airport-server` binary, which reads the real clock, calls
//! [`Host::step`] for every event that has come due, sends each
//! [`Report::Outbound`] over TCP, and calls [`Host::inject`] for every
//! message that arrives.
//!
//! The one job a `ProdHost` has that a simulated host does not is the disk.
//! After every step of its node, and before the reports of that step are
//! returned, it writes whatever changed in the node's durable state to its
//! [`Storage`]. So a vote is on disk before the reply that grants it is
//! sent, and an entry is on disk before it is acknowledged. A failed write
//! stops the process: a replica that cannot remember must not speak.

use std::collections::BTreeSet;
use std::fmt;

use det::objects::assert_log_shape;
use det::{Config, Durable, Effect, Entry, Index, Input, Ms, NodeId, Rng, Status, Term};
use raft::node::Node;
use raft::prod_node::ProdNode;

use crate::event::{Event, Report};
use crate::host::Host;
use crate::queue::EventQueue;
use crate::storage::Storage;

/// Why a [`ProdHost`] could not start.
#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    /// The list of cluster members is unusable.
    Cluster(String),
    /// The store could not be read.
    Storage(String),
    /// The store was read, but what it holds is not a state this node could
    /// have written.
    Corrupt(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cluster(why) => write!(f, "bad cluster: {why}"),
            Self::Storage(why) => write!(f, "cannot read durable state: {why}"),
            Self::Corrupt(why) => write!(f, "durable state is damaged: {why}"),
        }
    }
}

/// Runs one [`ProdNode`] and keeps its durable state on a [`Storage`].
pub struct ProdHost<C, S> {
    /// The node this host runs.
    node: ProdNode<C>,
    /// Where the node's durable state is kept.
    storage: S,
    /// Events waiting for their time.
    queue: EventQueue<Event<C>>,
    /// The `Wake` time last scheduled, so a new `Wake` is added only when
    /// the node's `next_wake` changes.
    scheduled_wake: Option<Ms>,
    /// The time of the last `step`, to check time never goes backwards.
    last_now: Ms,
    /// The term and vote last written to `storage`.
    saved_vote: (Term, Option<NodeId>),
}

/// Checks durable state read from a disk, which unlike state handed over in
/// memory may be damaged. Mirrors O1 to O3 without panicking.
fn check_durable<C>(durable: &Durable<C>, id: NodeId, peers: &[NodeId]) -> Result<(), String> {
    let mut previous = Term(1);
    for (i, entry) in durable.log.iter().enumerate() {
        if entry.term < previous {
            return Err(format!(
                "log entry {} has term {} after term {previous}",
                i + 1,
                entry.term
            ));
        }
        previous = entry.term;
    }
    if durable
        .log
        .last()
        .is_some_and(|last| last.term > durable.term)
    {
        return Err(format!("the log goes beyond term {}", durable.term));
    }
    if durable.term == Term(0) && durable.voted_for.is_some() {
        return Err("a vote in term 0".to_owned());
    }
    if let Some(vote) = durable.voted_for
        && vote != id
        && !peers.contains(&vote)
    {
        return Err(format!("a vote for {vote}, which is not in the cluster"));
    }
    Ok(())
}

impl<C: Clone + Eq, S: Storage<C>> ProdHost<C, S> {
    /// Starts node `id` from whatever `storage` holds: a brand new node if
    /// it is empty, a restarted one otherwise. `cluster` is every member,
    /// `id` included; the other members run under other hosts, normally on
    /// other machines. `rng` drives the node's election timeouts and `now`
    /// is the owner's current time.
    ///
    /// # Errors
    /// If `cluster` repeats a member or lacks `id`, or the stored state
    /// cannot be read or is damaged.
    pub fn new(
        id: NodeId,
        cluster: &[NodeId],
        config: Config,
        mut storage: S,
        rng: Rng,
        now: Ms,
    ) -> Result<Self, StartError> {
        let members: BTreeSet<NodeId> = cluster.iter().copied().collect();
        if members.len() != cluster.len() {
            return Err(StartError::Cluster("a member is listed twice".to_owned()));
        }
        if !members.contains(&id) {
            return Err(StartError::Cluster(format!("node {id} is not a member")));
        }
        let peers: Vec<NodeId> = members.into_iter().filter(|m| *m != id).collect();
        let durable = storage
            .load()
            .map_err(|e| StartError::Storage(e.to_string()))?;
        check_durable(&durable, id, &peers).map_err(StartError::Corrupt)?;
        assert_log_shape(&durable.log, durable.term);

        let saved_vote = (durable.term, durable.voted_for);
        let node = ProdNode::new(id, peers, config, durable, rng, now);
        let mut host = Self {
            node,
            storage,
            queue: EventQueue::new(),
            scheduled_wake: None,
            last_now: now,
            saved_vote,
        };
        host.reschedule_wake(now);
        Ok(host)
    }

    /// The node this host runs, for its role, term and positions.
    #[must_use]
    pub fn node(&self) -> &ProdNode<C> {
        &self.node
    }

    /// The store, for checks.
    #[must_use]
    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// What is left after a crash: the disk, and nothing else.
    #[must_use]
    pub fn into_storage(self) -> S {
        self.storage
    }

    /// Writes whatever the last step changed in the node's durable state.
    ///
    /// # Panics
    /// If the store fails. Carrying on would let the node send messages it
    /// might not remember sending after a restart.
    fn persist(&mut self) {
        let id = self.node.id();
        let vote = (self.node.term(), self.node.voted_for());
        if vote != self.saved_vote {
            if let Err(e) = self.storage.save_vote(vote.0, vote.1) {
                panic!("node {id}: cannot write term and vote, stopping: {e}");
            }
            self.saved_vote = vote;
        }
        if let Some(from) = self.node.take_log_change() {
            let keep = Index(from.0 - 1);
            let start = usize::try_from(keep.0).expect("log position fits in usize");
            if let Err(e) = self
                .storage
                .write_log(keep, &Node::log(&self.node)[start..])
            {
                panic!("node {id}: cannot write log from position {from}, stopping: {e}");
            }
        }
    }

    /// Steps the node, makes the result durable, and turns its effects into
    /// reports for the owner.
    fn step_node(&mut self, now: Ms, input: Input<C>) -> Vec<Report<C>> {
        let id = self.node.id();
        let effects = self.node.step(now, input);
        // Before anything is reported, and so before anything is sent.
        self.persist();
        let reports = effects
            .into_iter()
            .map(|effect| match effect {
                // The real network chooses its own delay.
                Effect::Send { to, message } => Report::Outbound {
                    at: now,
                    from: id,
                    to,
                    message,
                },
                Effect::Committed { index, entry } => Report::Committed {
                    node: id,
                    index,
                    entry,
                },
                Effect::NotLeader { hint } => Report::NotLeader { node: id, hint },
            })
            .collect();
        self.reschedule_wake(now);
        reports
    }

    /// If the node's `next_wake` differs from what is scheduled, queues a
    /// new `Wake`. Old ones stay in the queue, and the node ignores them (H5).
    fn reschedule_wake(&mut self, now: Ms) {
        let wake = self.node.next_wake();
        if wake != self.scheduled_wake {
            if let Some(at) = wake {
                self.queue.push(
                    at.max(now),
                    Event::Wake {
                        node: self.node.id(),
                    },
                );
            }
            self.scheduled_wake = wake;
        }
    }
}

impl<C: Clone + Eq, S: Storage<C>> Host<C> for ProdHost<C, S> {
    fn next_event_at(&self) -> Option<Ms> {
        self.queue.peek().map(|(at, _)| at)
    }

    fn step(&mut self, now: Ms) -> Vec<Report<C>> {
        // H1
        let (at, event) = self
            .queue
            .pop()
            .expect("ProdHost: step with nothing queued");
        assert_eq!(
            now, at,
            "ProdHost: stepped at {now} ms for an event at {at} ms"
        );
        assert!(now >= self.last_now, "ProdHost: time went back to {now} ms");
        self.last_now = now;
        let input = match event {
            Event::Wake { .. } => Input::Wake,
            Event::Deliver { from, message, .. } => Input::Message { from, message },
            Event::Propose { command, .. } => Input::Propose(command),
        };
        self.step_node(now, input)
    }

    fn inject(&mut self, at: Ms, event: Event<C>) {
        // H2
        assert!(
            at >= self.last_now,
            "ProdHost: event injected at {at} ms, before {} ms",
            self.last_now
        );
        let target = event.target();
        assert!(
            self.owns(target),
            "ProdHost: event for {target} injected into {}",
            self.node.id()
        );
        self.queue.push(at, event);
    }

    fn owns(&self, node: NodeId) -> bool {
        node == self.node.id()
    }

    fn statuses(&self) -> Vec<(NodeId, Status)> {
        vec![(self.node.id(), self.node.status())]
    }

    fn log(&self, node: NodeId) -> Option<&[Entry<C>]> {
        self.owns(node).then(|| Node::log(&self.node))
    }
}

#[cfg(test)]
mod tests {
    use det::objects::{Message, MessageCommand};
    use det::{Payload, Role};

    use super::*;
    use crate::storage::MemStorage;

    type TestHost = ProdHost<u32, MemStorage<u32>>;

    const CLUSTER: [NodeId; 3] = [NodeId(1), NodeId(2), NodeId(3)];
    /// The test network's fixed delay.
    const DELAY: Ms = Ms(5);

    fn start(id: u32, storage: MemStorage<u32>, now: Ms) -> TestHost {
        let rng = Rng::new(u64::from(id) + now.0);
        ProdHost::new(NodeId(id), &CLUSTER, Config::default(), storage, rng, now).unwrap()
    }

    /// Three hosts and an owner that plays the clock and the network.
    struct Owner {
        hosts: Vec<TestHost>,
        committed: Vec<(NodeId, Index, Entry<u32>)>,
        not_leader: Vec<(NodeId, Option<NodeId>)>,
    }

    impl Owner {
        fn new() -> Self {
            Self {
                hosts: (1..=3)
                    .map(|id| start(id, MemStorage::new(), Ms(0)))
                    .collect(),
                committed: Vec::new(),
                not_leader: Vec::new(),
            }
        }

        fn host(&mut self, id: NodeId) -> Option<&mut TestHost> {
            self.hosts.iter_mut().find(|h| h.owns(id))
        }

        /// Handles every event due up to `until`, earliest first.
        fn run_until(&mut self, until: Ms) {
            loop {
                let next = self
                    .hosts
                    .iter()
                    .enumerate()
                    .filter_map(|(i, h)| h.next_event_at().map(|at| (at, i)))
                    .min();
                let Some((at, i)) = next.filter(|(at, _)| *at <= until) else {
                    return;
                };
                let reports = self.hosts[i].step(at);
                // The disk always holds what the node would hand over.
                let host = &self.hosts[i];
                assert_eq!(host.storage().log(), Node::log(host.node()));
                assert_eq!(
                    host.storage().vote(),
                    (host.node().term(), host.node().voted_for())
                );
                for report in reports {
                    match report {
                        Report::Outbound {
                            at,
                            from,
                            to,
                            message,
                        } => {
                            if let Some(target) = self.host(to) {
                                target.inject(at + DELAY, Event::Deliver { from, to, message });
                            }
                        }
                        Report::Committed { node, index, entry } => {
                            self.committed.push((node, index, entry));
                        }
                        Report::NotLeader { node, hint } => self.not_leader.push((node, hint)),
                    }
                }
            }
        }

        fn leader(&self) -> NodeId {
            let leaders: Vec<_> = self
                .hosts
                .iter()
                .filter(|h| h.node().role() == Role::Leader)
                .map(|h| h.node().id())
                .collect();
            assert_eq!(leaders.len(), 1, "{leaders:?}");
            leaders[0]
        }
    }

    #[test]
    fn three_hosts_elect_a_leader_and_commit_a_proposal() {
        let mut owner = Owner::new();
        owner.run_until(Ms(1_000));
        let leader = owner.leader();
        for command in [7, 8] {
            let event = Event::Propose {
                to: leader,
                command,
            };
            owner.host(leader).unwrap().inject(Ms(1_000), event);
        }
        owner.run_until(Ms(2_000));
        assert_eq!(owner.leader(), leader, "the leader is kept");
        for id in CLUSTER {
            let log = owner.host(id).unwrap().log(id).unwrap().to_vec();
            let commands: Vec<_> = log.iter().map(|e| &e.payload).collect();
            assert_eq!(
                commands,
                [&Payload::Noop, &Payload::Command(7), &Payload::Command(8)]
            );
            let handed: Vec<_> = owner
                .committed
                .iter()
                .filter(|(node, ..)| *node == id)
                .map(|(_, index, _)| index.0)
                .collect();
            assert_eq!(handed, [1, 2, 3], "node {id}");
        }
        assert!(owner.not_leader.is_empty());
    }

    #[test]
    fn a_proposal_to_a_follower_is_reported_with_the_leader() {
        let mut owner = Owner::new();
        owner.run_until(Ms(1_000));
        let leader = owner.leader();
        let follower = CLUSTER.into_iter().find(|id| *id != leader).unwrap();
        let event = Event::Propose {
            to: follower,
            command: 1,
        };
        owner.host(follower).unwrap().inject(Ms(1_000), event);
        owner.run_until(Ms(1_100));
        assert_eq!(owner.not_leader, [(follower, Some(leader))]);
    }

    #[test]
    fn a_crashed_leader_comes_back_from_its_disk_as_a_follower() {
        let mut owner = Owner::new();
        owner.run_until(Ms(1_000));
        let leader = owner.leader();
        let event = Event::Propose {
            to: leader,
            command: 3,
        };
        owner.host(leader).unwrap().inject(Ms(1_000), event);
        owner.run_until(Ms(1_500));

        // Crash: everything but the disk is lost.
        let at = owner.hosts.iter().position(|h| h.owns(leader)).unwrap();
        let disk = owner.hosts.remove(at).into_storage();
        let term = disk.vote().0;
        owner.run_until(Ms(3_000));
        let successor = owner.leader();
        assert_ne!(successor, leader);

        owner.hosts.push(start(leader.0, disk, Ms(3_000)));
        owner.committed.clear();
        owner.run_until(Ms(4_000));
        assert_eq!(owner.leader(), successor);
        let back = owner.host(leader).unwrap();
        assert_eq!(back.node().role(), Role::Follower);
        assert!(back.node().term() > term);
        assert_eq!(back.node().commit(), back.node().last_index());
        let again: Vec<_> = owner
            .committed
            .iter()
            .filter(|(node, ..)| *node == leader)
            .map(|(_, index, _)| index.0)
            .collect();
        assert_eq!(again[..2], [1, 2], "handed over again from position 1");
    }

    #[test]
    fn a_vote_is_on_disk_before_the_reply_is_reported() {
        let mut host = start(1, MemStorage::new(), Ms(0));
        let request = MessageCommand::RequestVote {
            last_log_index: Index(0),
            last_log_term: Term(0),
        };
        host.inject(
            Ms(1),
            Event::Deliver {
                from: NodeId(2),
                to: NodeId(1),
                message: Message::new(Term(4), request),
            },
        );
        let reports = host.step(Ms(1));
        assert!(matches!(
            reports[..],
            [Report::Outbound { to: NodeId(2), .. }]
        ));
        assert_eq!(host.storage().vote(), (Term(4), Some(NodeId(2))));
        assert_eq!(host.storage().writes(), 1);
    }

    #[test]
    fn a_bad_cluster_or_a_damaged_disk_is_an_error_not_a_panic() {
        let new = |id, cluster: &[NodeId], storage| {
            ProdHost::<u32, _>::new(
                NodeId(id),
                cluster,
                Config::default(),
                storage,
                Rng::new(1),
                Ms(0),
            )
            .map(|_| ())
        };
        assert!(matches!(
            new(9, &CLUSTER, MemStorage::new()),
            Err(StartError::Cluster(_))
        ));
        assert!(matches!(
            new(1, &[NodeId(1), NodeId(1)], MemStorage::new()),
            Err(StartError::Cluster(_))
        ));

        let entry = |term| Entry {
            term: Term(term),
            payload: Payload::Noop,
        };
        let mut falling = MemStorage::new();
        falling.save_vote(Term(5), None).unwrap();
        falling.write_log(Index(0), &[entry(3), entry(2)]).unwrap();
        let mut ahead = MemStorage::new();
        ahead.save_vote(Term(1), None).unwrap();
        ahead.write_log(Index(0), &[entry(2)]).unwrap();
        let mut stranger = MemStorage::new();
        stranger.save_vote(Term(1), Some(NodeId(8))).unwrap();
        for storage in [falling, ahead, stranger] {
            let error = new(1, &CLUSTER, storage).unwrap_err();
            assert!(matches!(error, StartError::Corrupt(_)), "{error}");
        }
    }

    #[test]
    #[should_panic(expected = "for an event at")]
    fn stepping_at_the_wrong_time_is_a_bug() {
        let mut host = start(1, MemStorage::new(), Ms(0));
        let _ = host.step(Ms(1));
    }
}
