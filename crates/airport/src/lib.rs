//! The airport state machine (Phase 1).
//!
//! Pure logic: no clock, network, disk, threads or floats. It knows nothing
//! about replicas, the host or browsers. An [`Airport`] is given one
//! [`Command`] at a time and answers with [`Output`]s; the same commands
//! always produce the same state and the same [`Airport::fingerprint`].

pub mod command;
pub mod config;
pub mod data;
pub mod fingerprint;
pub mod policy;
pub mod rng;
pub mod scenario;
pub mod separation;
pub mod state;
pub mod traffic;
pub mod wind;

/// Simulated time in whole milliseconds since the start of the run.
pub type Ms = u64;

pub const MS_PER_MINUTE: Ms = 60_000;
pub const MS_PER_HOUR: Ms = 3_600_000;
pub const MS_PER_DAY: Ms = 24 * MS_PER_HOUR;

pub use command::{Command, Emergency, Output, Reason, RejectReason};
pub use config::{AirportConfig, ConfigError, RunwayConfig};
pub use separation::gap_ms;
pub use state::{Airport, Snapshot};
pub use wind::Wind;
