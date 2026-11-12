//! Pure logic: no clock, network, disk, threads or floats. It knows nothing
//! about replicas, the host or browsers. An [`Airport`] is given one
//! [`Command`] at a time and answers with [`Output`]s; the same commands
//! always produce the same state and the same [`Airport::fingerprint`].

pub mod command;
pub mod config;
pub mod data;
pub mod policy;
pub mod scenario;
pub mod separation;
pub mod state;
pub mod traffic;
pub mod wind;

// The seeded generator and the fingerprint live in `det` so that `raft` can use
// them without depending on airports. Re-exported so `airport::rng` and
// `airport::fingerprint` still work.
pub use det::{fingerprint, rng};

/// Simulated time in whole milliseconds since the start of the run.
pub type Ms = u64;

/// Milliseconds in one minute.
pub const MS_PER_MINUTE: Ms = 60_000;
/// Milliseconds in one hour.
pub const MS_PER_HOUR: Ms = 3_600_000;
/// Milliseconds in one day.
pub const MS_PER_DAY: Ms = 24 * MS_PER_HOUR;

pub use command::{Command, Emergency, Output, Reason, RejectReason};
pub use config::{AirportConfig, ConfigError, RunwayConfig};
pub use separation::gap_ms;
pub use state::{Airport, Snapshot};
pub use wind::Wind;
