//! Tests for [`ProdNode`], driven without a host: a few lines here play the
//! network, the disk and the clock.
//!
//! The seeded runs lose, repeat and reorder messages and crash nodes, and
//! check Raft's safety rules after every step. `RAFT_SEEDS=0..2000 cargo test
//! --release -p raft --test prod_node` runs more seeds.

use std::collections::BTreeMap;

use det::objects::{Message, MessageCommand};
use det::{Config, Durable, Effect, Entry, Index, Input, Ms, NodeId, Payload, Rng, Role, Term};
use raft::node::Node;
use raft::prod_node::{MAX_BATCH, ProdNode};

type TestNode = ProdNode<u32>;

/// Node 1 times out well before the others, so tests know who stands first.
fn config_of(id: u32) -> Config {
    Config {
        heartbeat_interval: Ms(50),
        base_election_timeout: Ms(if id == 1 { 60 } else { 150 }),
    }
}

fn node(id: u32, size: u32, durable: Durable<u32>, now: Ms) -> TestNode {
    let peers = (1..=size).filter(|p| *p != id).map(NodeId).collect();
    let rng = Rng::new(u64::from(id));
    ProdNode::new(NodeId(id), peers, config_of(id), durable, rng, now)
}

fn fresh(id: u32, size: u32) -> TestNode {
    node(id, size, Durable::new(Term(0), None, vec![]), Ms(0))
}

fn entry(term: u64, command: u32) -> Entry<u32> {
    Entry {
        term: Term(term),
        payload: Payload::Command(command),
    }
}

/// A log holding one entry for each term listed, numbered by position.
fn log_of(terms: &[u64]) -> Vec<Entry<u32>> {
    terms
        .iter()
        .enumerate()
        .map(|(i, t)| entry(*t, u32::try_from(i).unwrap() + 1))
        .collect()
}

/// Nodes joined by a network that delivers everything, in order, at once.
struct Perfect {
    nodes: BTreeMap<NodeId, TestNode>,
    now: Ms,
    committed: BTreeMap<NodeId, Vec<(Index, Entry<u32>)>>,
    not_leader: Vec<(NodeId, Option<NodeId>)>,
}

impl Perfect {
    fn new(nodes: Vec<TestNode>) -> Self {
        Self {
            nodes: nodes.into_iter().map(|n| (n.id(), n)).collect(),
            now: Ms(0),
            committed: BTreeMap::new(),
            not_leader: Vec::new(),
        }
    }

    fn fresh(size: u32) -> Self {
        Self::new((1..=size).map(|id| fresh(id, size)).collect())
    }

    /// Steps `to` with `input`, then delivers every message that results,
    /// and every message those cause, until the network is quiet.
    fn step(&mut self, to: u32, input: Input<u32>) {
        let mut pending = vec![(NodeId(to), input)];
        while !pending.is_empty() {
            let (to, input) = pending.remove(0);
            let Some(node) = self.nodes.get_mut(&to) else {
                continue;
            };
            for effect in node.step(self.now, input) {
                match effect {
                    Effect::Send { to: next, message } => {
                        pending.push((next, Input::Message { from: to, message }));
                    }
                    Effect::Committed { index, entry } => {
                        self.committed.entry(to).or_default().push((index, entry));
                    }
                    Effect::NotLeader { hint } => self.not_leader.push((to, hint)),
                }
            }
        }
    }

    /// Moves time on by `ms` and wakes every node.
    fn pass(&mut self, ms: u64) {
        self.now = self.now + Ms(ms);
        for id in self.nodes.keys().copied().collect::<Vec<_>>() {
            self.step(id.0, Input::Wake);
        }
    }

    /// Wakes node 1 at its election deadline, which is before anyone else's.
    fn elect(&mut self) {
        self.now = self.nodes[&NodeId(1)].next_wake().unwrap();
        self.step(1, Input::Wake);
        assert_eq!(self.leader(), 1);
    }

    fn leader(&self) -> u32 {
        let leaders: Vec<_> = self
            .nodes
            .values()
            .filter(|n| n.role() == Role::Leader)
            .map(|n| n.id().0)
            .collect();
        assert_eq!(leaders.len(), 1, "leaders: {leaders:?}");
        leaders[0]
    }

    fn logs_agree(&self) {
        let first = self.nodes.values().next().unwrap().log();
        for n in self.nodes.values() {
            assert_eq!(n.log(), first, "node {}", n.id());
        }
    }
}

#[test]
fn three_nodes_elect_one_leader_and_keep_it() {
    let mut c = Perfect::fresh(3);
    c.elect();
    for n in c.nodes.values() {
        assert_eq!(n.term(), Term(1));
        assert_eq!(n.leader(), Some(NodeId(1)));
    }
    for _ in 0..100 {
        c.pass(25);
    }
    assert_eq!(c.leader(), 1);
    assert_eq!(c.nodes[&NodeId(2)].term(), Term(1), "no further elections");
}

#[test]
fn a_cluster_of_one_elects_itself_and_commits_alone() {
    let mut c = Perfect::fresh(1);
    c.elect();
    assert_eq!(c.nodes[&NodeId(1)].next_wake(), None);
    c.step(1, Input::Propose(7));
    assert_eq!(c.nodes[&NodeId(1)].commit(), Index(2));
    assert_eq!(c.committed[&NodeId(1)][1], (Index(2), entry(1, 7)));
}

#[test]
fn a_proposal_reaches_every_log_and_is_committed_everywhere() {
    for size in [3, 5] {
        let mut c = Perfect::fresh(size);
        c.elect();
        c.step(1, Input::Propose(41));
        c.step(1, Input::Propose(42));
        c.pass(50); // a heartbeat carries the commit position to followers
        c.logs_agree();
        for n in c.nodes.values() {
            assert_eq!(n.log().len(), 3);
            assert_eq!(n.commit(), Index(3));
            let got: Vec<_> = c.committed[&n.id()].iter().map(|(i, _)| i.0).collect();
            assert_eq!(got, [1, 2, 3], "in order, each once");
        }
    }
}

#[test]
fn a_follower_answers_not_leader_with_a_hint() {
    let mut c = Perfect::fresh(3);
    c.step(3, Input::Propose(1));
    assert_eq!(c.not_leader, [(NodeId(3), None)]);
    c.elect();
    c.step(3, Input::Propose(1));
    assert_eq!(c.not_leader[1], (NodeId(3), Some(NodeId(1))));
    assert_eq!(
        c.nodes[&NodeId(3)].log().len(),
        1,
        "only the leader's no-op"
    );
}

#[test]
fn a_leader_cut_off_from_everyone_commits_nothing() {
    let mut c = Perfect::fresh(3);
    c.elect();
    let (n2, n3) = (c.nodes.remove(&NodeId(2)), c.nodes.remove(&NodeId(3)));
    c.step(1, Input::Propose(5));
    assert_eq!(c.nodes[&NodeId(1)].commit(), Index(1));
    // One follower back makes a majority.
    c.nodes.insert(NodeId(2), n2.unwrap());
    drop(n3);
    c.pass(50);
    c.pass(50);
    assert_eq!(c.nodes[&NodeId(1)].commit(), Index(2));
}

#[test]
fn a_vote_is_given_once_per_term_and_only_to_an_up_to_date_log() {
    let request = |from: u32, term: u64, last: (u64, u64)| Input::Message {
        from: NodeId(from),
        message: Message::new(
            Term(term),
            MessageCommand::RequestVote {
                last_log_index: Index(last.0),
                last_log_term: Term(last.1),
            },
        ),
    };
    let granted = |effects: Vec<Effect<u32>>| {
        effects
            .iter()
            .any(|e| matches!(e, Effect::Send { message, .. } if message.command == MessageCommand::VoteReply))
    };

    let mut n = node(1, 3, Durable::new(Term(2), None, log_of(&[1, 2, 2])), Ms(0));
    // Shorter log, then older last term: both refused, without a vote spent.
    assert!(!granted(n.step(Ms(1), request(2, 3, (2, 2)))));
    assert!(!granted(n.step(Ms(1), request(2, 3, (9, 1)))));
    assert_eq!(n.voted_for(), None);
    // As good as ours: granted, and again when the request is repeated.
    assert!(granted(n.step(Ms(1), request(2, 3, (3, 2)))));
    assert!(granted(n.step(Ms(1), request(2, 3, (3, 2)))));
    // A second candidate in the same term is refused.
    assert!(!granted(n.step(Ms(1), request(3, 3, (8, 3)))));
    assert_eq!(n.voted_for(), Some(NodeId(2)));

    // The vote survives a crash.
    let mut n = node(1, 3, n.into_durable(), Ms(10));
    assert!(!granted(n.step(Ms(11), request(3, 3, (8, 3)))));
}

#[test]
fn a_stale_request_is_refused_with_our_term_and_never_with_a_vote() {
    let mut n = node(1, 3, Durable::new(Term(5), None, vec![]), Ms(0));
    let effects = n.step(
        Ms(1),
        Input::Message {
            from: NodeId(2),
            message: Message::new(
                Term(3),
                MessageCommand::RequestVote {
                    last_log_index: Index(0),
                    last_log_term: Term(0),
                },
            ),
        },
    );
    let [Effect::Send { to, message }] = &effects[..] else {
        panic!("{effects:?}");
    };
    assert_eq!((*to, message.term), (NodeId(2), Term(5)));
    assert!(matches!(
        message.command,
        MessageCommand::AppendReply { success: false, .. }
    ));
    assert_eq!(n.voted_for(), None);
}

/// Figure 7 of the Raft paper: a leader of term 8 and six followers whose
/// logs are short, long, or different. All end up equal to the leader's.
#[test]
fn the_followers_of_figure_7_are_repaired() {
    let leader = [1, 1, 1, 4, 4, 5, 5, 6, 6, 6];
    let followers: [&[u64]; 6] = [
        &[1, 1, 1, 4, 4, 5, 5, 6, 6],
        &[1, 1, 1, 4],
        &[1, 1, 1, 4, 4, 5, 5, 6, 6, 6, 6],
        &[1, 1, 1, 4, 4, 5, 5, 6, 6, 6, 7, 7],
        &[1, 1, 1, 4, 4, 4, 4],
        &[1, 1, 1, 2, 2, 2, 3, 3, 3, 3, 3],
    ];
    let mut nodes = vec![node(
        1,
        7,
        Durable::new(Term(7), None, log_of(&leader)),
        Ms(0),
    )];
    for (i, terms) in followers.iter().enumerate() {
        let id = u32::try_from(i).unwrap() + 2;
        nodes.push(node(
            id,
            7,
            Durable::new(Term(7), None, log_of(terms)),
            Ms(0),
        ));
    }
    let mut c = Perfect::new(nodes);
    c.elect();
    c.pass(50);
    c.logs_agree();
    let log = c.nodes[&NodeId(1)].log();
    assert_eq!(&log[..10], &log_of(&leader)[..]);
    assert_eq!(
        log[10],
        Entry {
            term: Term(8),
            payload: Payload::Noop
        }
    );
    assert_eq!(log.len(), 11);
    assert!(c.nodes.values().all(|n| n.commit() == Index(11)));
}

#[test]
fn a_follower_far_behind_is_caught_up_in_batches() {
    let mut c = Perfect::fresh(3);
    c.elect();
    let behind = c.nodes.remove(&NodeId(3)).unwrap();
    let count = u32::try_from(MAX_BATCH * 2 + 10).unwrap();
    for command in 0..count {
        c.step(1, Input::Propose(command));
    }
    c.nodes.insert(NodeId(3), behind);
    c.pass(50);
    c.pass(50);
    c.logs_agree();
    assert_eq!(c.nodes[&NodeId(3)].commit().0, u64::from(count) + 1);
}

#[test]
fn a_restarted_leader_is_a_follower_and_hands_everything_over_again() {
    let mut c = Perfect::fresh(3);
    c.elect();
    c.step(1, Input::Propose(9));
    let old = c.nodes.remove(&NodeId(1)).unwrap();
    while !c.nodes.values().any(|n| n.role() == Role::Leader) {
        c.pass(10);
    }
    let mut back = node(1, 3, old.into_durable(), c.now);
    assert_eq!(back.role(), Role::Follower);
    assert_eq!(back.commit(), Index(0));
    assert_eq!(back.take_log_change(), None, "nothing new to write");
    c.nodes.insert(NodeId(1), back);
    c.committed.clear();
    c.pass(50);
    c.pass(50);
    c.logs_agree();
    let again: Vec<_> = c.committed[&NodeId(1)].iter().map(|(i, _)| i.0).collect();
    assert_eq!(again, [1, 2, 3], "from position 1, in order");
}

#[test]
fn the_log_change_names_the_first_position_to_write() {
    let mut c = Perfect::fresh(3);
    c.elect();
    let leader = c.nodes.get_mut(&NodeId(1)).unwrap();
    assert_eq!(leader.take_log_change(), Some(Index(1)));
    assert_eq!(leader.take_log_change(), None);
    c.step(1, Input::Propose(1));
    c.step(1, Input::Propose(2));
    assert_eq!(
        c.nodes.get_mut(&NodeId(1)).unwrap().take_log_change(),
        Some(Index(2))
    );
    assert_eq!(
        c.nodes.get_mut(&NodeId(2)).unwrap().take_log_change(),
        Some(Index(1))
    );
}

// ---------------------------------------------------------------------------
// Seeded runs on a bad network.

struct InFlight {
    at: Ms,
    from: NodeId,
    to: NodeId,
    message: Message<u32>,
}

struct Chaos {
    size: u32,
    rng: Rng,
    now: Ms,
    live: BTreeMap<NodeId, TestNode>,
    down: BTreeMap<NodeId, Durable<u32>>,
    lives: u64,
    network: Vec<InFlight>,
    calm: bool,
    proposing: bool,
    /// Who was leader in each term, for the whole run.
    leaders: BTreeMap<Term, NodeId>,
    /// What was committed at each position, by anyone, ever.
    committed: BTreeMap<Index, Entry<u32>>,
    /// The last position each node handed over in its current life.
    handed: BTreeMap<NodeId, Index>,
    next_command: u32,
}

impl Chaos {
    fn new(seed: u64, size: u32) -> Self {
        Self {
            size,
            rng: Rng::new(seed),
            now: Ms(0),
            live: (1..=size).map(|id| (NodeId(id), fresh(id, size))).collect(),
            down: BTreeMap::new(),
            lives: 0,
            network: Vec::new(),
            calm: false,
            proposing: true,
            leaders: BTreeMap::new(),
            committed: BTreeMap::new(),
            handed: BTreeMap::new(),
            next_command: 0,
        }
    }

    fn chance(&mut self, percent: u64) -> bool {
        !self.calm && self.rng.below(100) < percent
    }

    fn step(&mut self, id: NodeId, input: Input<u32>) {
        let Some(node) = self.live.get_mut(&id) else {
            return;
        };
        let was_leader = node.role() == Role::Leader;
        let log_before = was_leader.then(|| node.log().to_vec());
        let effects = node.step(self.now, input);
        // Leaders only append.
        if let Some(before) = log_before
            && node.role() == Role::Leader
        {
            assert_eq!(&node.log()[..before.len()], &before[..]);
        }
        // One leader per term.
        if node.role() == Role::Leader {
            let known = *self.leaders.entry(node.term()).or_insert(id);
            assert_eq!(known, id, "two leaders in term {}", node.term());
        }
        for effect in effects {
            match effect {
                Effect::Send { to, message } => {
                    if self.chance(15) {
                        continue;
                    }
                    let copies = if self.chance(10) { 2 } else { 1 };
                    for _ in 0..copies {
                        let delay = if self.calm {
                            5
                        } else {
                            1 + self.rng.below(120)
                        };
                        self.network.push(InFlight {
                            at: self.now + Ms(delay),
                            from: id,
                            to,
                            message: message.clone(),
                        });
                    }
                }
                Effect::Committed { index, entry } => {
                    // In order, each once per life.
                    let handed = self.handed.entry(id).or_insert(Index(0));
                    assert_eq!(index.0, handed.0 + 1, "node {id} skipped or repeated");
                    *handed = index;
                    // Same entry at each position, for everyone, for ever.
                    let known = self.committed.entry(index).or_insert_with(|| entry.clone());
                    assert_eq!(*known, entry, "position {index} committed twice");
                }
                Effect::NotLeader { .. } => {}
            }
        }
    }

    fn tick(&mut self) {
        self.now = self.now + Ms(1 + self.rng.below(10));
        // Deliver what is due, in the order it was sent.
        let now = self.now;
        let mut due = Vec::new();
        let mut i = 0;
        while i < self.network.len() {
            if self.network[i].at <= now {
                due.push(self.network.remove(i));
            } else {
                i += 1;
            }
        }
        for InFlight {
            from, to, message, ..
        } in due
        {
            self.step(to, Input::Message { from, message });
        }
        for id in self.live.keys().copied().collect::<Vec<_>>() {
            if self.live[&id].next_wake().is_some_and(|at| at <= now) {
                self.step(id, Input::Wake);
            }
        }
        if self.proposing && self.rng.below(100) < 30 {
            let to = NodeId(1 + u32::try_from(self.rng.below(u64::from(self.size))).unwrap());
            self.next_command += 1;
            self.step(to, Input::Propose(self.next_command));
        }
        if self.chance(1) {
            let id = NodeId(1 + u32::try_from(self.rng.below(u64::from(self.size))).unwrap());
            if let Some(node) = self.live.remove(&id) {
                self.down.insert(id, node.into_durable());
                self.handed.remove(&id);
            } else {
                self.restart(id);
            }
        }
    }

    fn restart(&mut self, id: NodeId) {
        let durable = self.down.remove(&id).unwrap();
        self.lives += 1;
        let peers = (1..=self.size).map(NodeId).filter(|p| *p != id).collect();
        let rng = Rng::new(self.rng.next_u64() ^ self.lives);
        self.live.insert(
            id,
            ProdNode::new(id, peers, Config::default(), durable, rng, self.now),
        );
    }

    fn heal(&mut self) {
        self.calm = true;
        for id in self.down.keys().copied().collect::<Vec<_>>() {
            self.restart(id);
        }
    }
}

fn chaos(seed: u64, size: u32) {
    let mut c = Chaos::new(seed, size);
    for _ in 0..4_000 {
        c.tick();
    }
    c.heal();
    for _ in 0..1_500 {
        c.tick();
    }
    c.proposing = false;
    for _ in 0..300 {
        c.tick();
    }
    // Progress once the faults stop: one leader, and everyone agrees.
    let leaders: Vec<_> = c
        .live
        .values()
        .filter(|n| n.role() == Role::Leader)
        .collect();
    assert_eq!(
        leaders.len(),
        1,
        "seed {seed}: no single leader after the heal"
    );
    let leader = leaders[0];
    assert!(
        leader.log().len() > 1,
        "seed {seed}: nothing was ever stored"
    );
    for n in c.live.values() {
        assert_eq!(
            n.log(),
            leader.log(),
            "seed {seed}: node {} differs",
            n.id()
        );
        assert_eq!(
            n.commit(),
            leader.last_index(),
            "seed {seed}: node {}",
            n.id()
        );
    }
    for (position, entry) in &c.committed {
        let at = usize::try_from(position.0).unwrap() - 1;
        assert_eq!(
            leader.log()[at],
            *entry,
            "seed {seed}: lost position {position}"
        );
    }
}

/// The seeds to try: `RAFT_SEEDS=first..last` if set, otherwise `0..60`.
#[expect(
    clippy::disallowed_methods,
    reason = "the seed range is test configuration, not simulation input"
)]
fn seeds() -> std::ops::Range<u64> {
    let range = std::env::var("RAFT_SEEDS").ok().and_then(|v| {
        let (a, b) = v.split_once("..")?;
        Some(a.parse().ok()?..b.parse().ok()?)
    });
    range.unwrap_or(0..60)
}

#[test]
fn safety_holds_on_a_bad_network_and_progress_returns_after_it() {
    for seed in seeds() {
        chaos(seed, 3);
        chaos(seed, 5);
    }
}
