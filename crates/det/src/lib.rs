//! Deterministic tools shared by every crate ("det" for deterministic).
//!
//! The seeded random number generator, the state fingerprint, the
//! identifier types, and the values a Raft node exchanges with the host. Nothing
//! here knows about airports or the host, so any crate can depend on it.

pub mod config;
pub mod fingerprint;
pub mod ids;
pub mod objects;
pub mod rng;

pub use config::Config;
pub use fingerprint::{Fnv1a, fingerprint};
pub use ids::{ClientId, Index, Ms, NodeId, Term};
pub use objects::{Durable, Effect, Entry, Input, Payload, Role, Status};
pub use rng::Rng;
