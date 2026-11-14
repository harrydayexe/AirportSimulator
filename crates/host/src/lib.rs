//! The host (Phase 2).
//!
//! A host runs Raft nodes: it plays the part of their network, disks and
//! clock. There are two families, both behind the [`host::Host`] trait:
//!
//! - the simulated hosts ([`childhost`], [`browserhost`]), which run every
//!   replica inside one deterministic event loop for the browser and for
//!   bulk seeded runs;
//! - [`prodhost::ProdHost`], which runs one real replica on one machine and
//!   writes its durable state to a [`storage::Storage`] before anything
//!   leaves the machine. The `airport-server` binary drives it with the real
//!   clock and real sockets.

mod hash;
pub use airport;
pub use raft;
pub mod browserhost;
pub mod childhost;
pub mod clock;
pub mod event;
pub mod host;
pub mod prodhost;
pub mod queue;
pub mod report;
pub mod seeds;
pub mod storage;
