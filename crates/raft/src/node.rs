use det::{Config, Durable, Effect, Entry, Input, Ms, NodeId, Rng, Role, Status, Term};

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
    pub fn new(
        id: NodeId,
        peers: Vec<NodeId>,
        config: Config,
        durable: Durable<C>,
        rng: Rng,
        now: Ms,
    ) -> Self {
        return Node {
            term: durable.term,
            voted_for: durable.voted_for,
            log: durable.log,
            id,
            peers,
            config,
            rng,
            now,
            role: Role::Follower,
        };
    }

    pub fn step(&mut self, now: Ms, input: Input<C>) -> Vec<Effect<C>> {
        unimplemented!("Implement Node.step(now:input:) first.")
    }

    /// when the host should send the next Wake
    pub fn next_wake(&self) -> Option<Ms> {
        unimplemented!("Implement Node.next_wake() first.")
    }

    /// what is left after a crash
    pub fn into_durable(self) -> Durable<C> {
        return Durable {
            term: self.term,
            voted_for: self.voted_for,
            log: self.log,
        };
    }

    /// role, term, positions: for checkers and the web view
    pub fn status(&self) -> Status {
        unimplemented!("Implement Node.status() first.")
    }

    /// read-only, for checkers
    pub fn log(&self) -> &[Entry<C>] {
        unimplemented!("Implement Node.log() first.")
    }
}
