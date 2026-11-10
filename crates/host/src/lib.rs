//! The host (Phase 2).
//!
//! Runs every replica and controller inside one deterministic event loop,
//! playing the part of the network, disks and clock. Wrapped twice: by the
//! `cli` crate for bulk seeded runs, and by the `wasm` crate for the browser.

pub use airport;
pub use raft;
