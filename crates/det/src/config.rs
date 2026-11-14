//! Settings for a Raft member.

use crate::Ms;

/// A Raft member's settings, such as its election timeout and heartbeat
/// interval. Empty until the timing rules are written.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct Config {
    /// The expected interval between heartbeats from the leader.
    pub heartbeat_interval: Ms,

    /// The base amount of time to wait after an election.
    pub base_election_timeout: Ms,
}

impl Config {
    /// Checks the settings can work at all. They come from code, not from a
    /// user, so a bad value is a bug.
    ///
    /// # Panics
    /// If either interval is zero, if heartbeats are not more frequent than
    /// election timeouts (followers would time out between heartbeats and no
    /// leader would last), or if twice the election timeout overflows.
    pub fn assert_valid(&self) {
        let Self {
            heartbeat_interval: Ms(heartbeat),
            base_election_timeout: Ms(election),
        } = *self;
        assert!(heartbeat > 0, "Config: heartbeat_interval is zero");
        assert!(election > 0, "Config: base_election_timeout is zero");
        assert!(
            heartbeat < election,
            "Config: heartbeat_interval {heartbeat} ms is not below base_election_timeout {election} ms"
        );
        assert!(
            election.checked_mul(2).is_some(),
            "Config: twice base_election_timeout overflows"
        );
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            heartbeat_interval: Ms(50),
            base_election_timeout: Ms(150),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid() {
        Config::default().assert_valid();
    }

    #[test]
    fn default_config_leaves_room_for_heartbeats() {
        let config = Config::default();
        assert_eq!(config.heartbeat_interval, Ms(50));
        assert_eq!(config.base_election_timeout, Ms(150));
        assert!(config.heartbeat_interval < config.base_election_timeout);
    }

    #[test]
    #[should_panic(expected = "is not below")]
    fn heartbeat_must_beat_the_election_timeout() {
        Config {
            heartbeat_interval: Ms(150),
            base_election_timeout: Ms(150),
        }
        .assert_valid();
    }
}
