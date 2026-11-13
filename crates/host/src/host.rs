//! Host defines a trait which is capable as acting as the host for one or more [`crate::raft::node::Node`]

use det::{Entry, Ms, NodeId, Status};

use crate::event::{Event, Report};

/// Something that runs Raft nodes on simulated time it is given.
///
/// A host never decides what time it is. Its owner asks when it next has
/// work (`next_event_at`), and later tells it to do that work (`step`).
pub trait Host<C> {
    /// The time of this host's earliest pending event, if it has one.
    #[must_use]
    fn next_event_at(&self) -> Option<Ms>;

    /// Handle this host's earliest event. `now` must equal `next_event_at()`;
    /// a host asked to run early or late panics, because its owner has a bug.
    fn step(&mut self, now: Ms) -> Vec<Report<C>>;

    /// Add an event to this host's queue, for example a message routed here
    /// from another host. `at` must not be earlier than the last `now`.
    fn inject(&mut self, at: Ms, event: Event<C>);

    /// Whether `node` lives in this host or anywhere below it.
    #[must_use]
    fn owns(&self, node: NodeId) -> bool;

    /// Every node below this host with its status, in `NodeId` order.
    /// For checkers and the web view.
    #[must_use]
    fn statuses(&self) -> Vec<(NodeId, Status)>;

    /// One node's log, if the node lives below this host and is running.
    /// For checkers.
    #[must_use]
    fn log(&self, node: NodeId) -> Option<&[Entry<C>]>;
}
