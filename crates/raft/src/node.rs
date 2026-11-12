//! One Raft node: inputs in, effects out (Phase 2 spec, ticket B1).

use det::{Config, Durable, Effect, Entry, Input, Ms, NodeId, Rng, Status};

/// One member of the cluster, storing commands of type `C`. Its first three
/// fields are what [`Durable`] keeps across a crash; the rest is lost.
pub trait Node<C: Clone + Eq> {
    /// A follower in the term `durable` records, with its vote and log. `peers`
    /// are the other nodes; `rng` drives its random election timeouts.
    #[must_use]
    fn new(
        id: NodeId,
        peers: Vec<NodeId>,
        config: Config,
        durable: Durable<C>,
        rng: Rng,
        now: Ms,
    ) -> Self;

    /// Handle one input at simulated time `now` and return what the host
    /// should do as a result.
    fn step(&mut self, _now: Ms, _input: Input<C>) -> Vec<Effect<C>>;

    /// When the host should send the next `Wake`.
    #[must_use]
    fn next_wake(&self) -> Option<Ms>;

    /// What is left after a crash.
    #[must_use]
    fn into_durable(self) -> Durable<C>;

    /// Role, term and positions, for checkers and the web view.
    #[must_use]
    fn status(&self) -> Status;

    /// The whole log, read-only, for checkers.
    #[must_use]
    fn log(&self) -> &[Entry<C>];
}
