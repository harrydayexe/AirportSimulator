//! Deterministic tools shared by every crate ("det" for deterministic).
//!
//! The seeded random number generator, the state fingerprint, and the
//! identifier types. Nothing here knows about airports, Raft or the host, so
//! any crate can depend on it.

pub mod fingerprint;
pub mod rng;

pub use fingerprint::{Fnv1a, fingerprint};
pub use rng::Rng;
