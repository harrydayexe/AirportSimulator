//! One Raft node: inputs in, effects out (Phase 2 spec, ticket B1).

use det::{Config, Durable, Effect, Entry, Input, Ms, NodeId, Rng, Role, Status, Term};

/// One member of the cluster, storing commands of type `C`. Its first three
/// fields are what [`Durable`] keeps across a crash; the rest is lost.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Node<C> {
    term: Term,
    voted_for: Option<NodeId>,
    log: Vec<Entry<C>>,

    id: NodeId,
    peers: Vec<NodeId>,
    config: Config,
    rng: Rng,
    now: Ms,
    role: Role,
}

impl<C: Clone + Eq> Node<C> {
    /// A follower in the term `durable` records, with its vote and log. `peers`
    /// are the other nodes; `rng` drives its random election timeouts.
    #[must_use]
    pub fn new(
        id: NodeId,
        peers: Vec<NodeId>,
        config: Config,
        durable: Durable<C>,
        rng: Rng,
        now: Ms,
    ) -> Self {
        Node {
            term: durable.term,
            voted_for: durable.voted_for,
            log: durable.log,
            id,
            peers,
            config,
            rng,
            now,
            role: Role::Follower,
        }
    }

    /// Handle one input at simulated time `now` and return what the host
    /// should do as a result.
    pub fn step(&mut self, _now: Ms, _input: Input<C>) -> Vec<Effect<C>> {
        unimplemented!("Implement Node.step(now:input:) first.")
    }

    /// When the host should send the next `Wake`.
    #[must_use]
    pub fn next_wake(&self) -> Option<Ms> {
        unimplemented!("Implement Node.next_wake() first.")
    }

    /// What is left after a crash.
    #[must_use]
    pub fn into_durable(self) -> Durable<C> {
        Durable {
            term: self.term,
            voted_for: self.voted_for,
            log: self.log,
        }
    }

    /// Role, term and positions, for checkers and the web view.
    #[must_use]
    pub fn status(&self) -> Status {
        unimplemented!("Implement Node.status() first.")
    }

    /// The whole log, read-only, for checkers.
    #[must_use]
    pub fn log(&self) -> &[Entry<C>] {
        unimplemented!("Implement Node.log() first.")
    }
}
