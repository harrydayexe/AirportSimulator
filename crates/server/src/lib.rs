//! `airport-server`: one real replica of the airport group.
//!
//! The deterministic crates decide everything: `raft::prod_node::ProdNode`
//! is the Raft logic, `host::prodhost::ProdHost` runs it and says what must
//! be written and sent, and [`replica::Replica`] is the airport behind it.
//! This crate is the thin, non-deterministic shell around them that a real
//! deployment needs and a simulation must not have:
//!
//! - [`server`] reads the real clock and runs the loop that owns the host;
//! - [`transport`] carries Raft messages between machines over TCP;
//! - [`storage`] keeps the node's durable state in files;
//! - [`http`] is the port clients send commands to.
//!
//! The intended deployment is three replicas on three machines. Any odd
//! number works; the cluster keeps going while a majority can talk.
//!
//! The peer port trusts whatever speaks the protocol with the right cluster
//! fingerprint, so it belongs on a private network.

pub mod config;
pub mod http;
pub mod replica;
pub mod server;
pub mod storage;
pub mod transport;
pub mod wire;
