//! Settings for a Raft node.

use crate::Ms;

/// A Raft node's settings, such as its election timeout and heartbeat
/// interval. Empty until the timing rules are written.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Config {
    /// The expected interval between heartbeats from the leader.
    pub heartbeat_interval: Ms,

    /// The base amount of time to wait after an election.
    pub base_election_timeout: Ms,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            heartbeat_interval: Ms(50),
            base_election_timeout: Ms(150),
        }
    }
}
