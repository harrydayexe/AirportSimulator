//! An implementation of [`Node`], designed to run in one browser WASM program.
use det::{Config, Durable, Effect, Entry, Input, Ms, NodeId, Rng, Role, Status, Term};

use crate::node::Node;

/// An implementation of [`Node`], designed to run in one browser WASM program.
/// >**Warning:** this is not intended for production deployment. Demo purposes only.
struct BrowserNode<C> {
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
            role: Role::Follower,
        }
    }

    /// Handle one input at simulated time `now` and return what the host
    /// should do as a result.
    fn step(&mut self, _now: Ms, _input: Input<C>) -> Vec<Effect<C>> {
        unimplemented!("Implement Node.step(now:input:) first.")
    }

    /// When the host should send the next `Wake`.
    fn next_wake(&self) -> Option<Ms> {
        unimplemented!("Implement Node.next_wake() first.")
    }

    /// What is left after a crash.
    fn into_durable(self) -> Durable<C> {
        Durable {
            term: self.term,
            voted_for: self.voted_for,
            log: self.log,
        }
    }

    /// Role, term and positions, for checkers and the web view.
    fn status(&self) -> Status {
        unimplemented!("Implement Node.status() first.")
    }

    /// The whole log, read-only, for checkers.
    fn log(&self) -> &[Entry<C>] {
        unimplemented!("Implement Node.log() first.")
    }
}
