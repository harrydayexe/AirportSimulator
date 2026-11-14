//! The participants of the system
//!
//! A [`Node`] is one participant in the protocol. The replica is the first;
//! a region's air traffic control (Phase 4) will be another. Each node is
//! written as if it ran alone on its own machine. It never reads a clock,
//! opens a connection or touches a disk; it is handed inputs and returns
//! effects, and its host does the rest.
//!
//! [`Node`] is the whole interface between a node and its host, and there
//! are two kinds of host for the same nodes. The simulated one (the `host`
//! crate) puts every node in one program on a fake network and clock, for
//! seeded test runs and for the browser demo. A production one runs a single
//! node on its own machine with real sockets, the system clock and a file.
//! Nothing in this crate may depend on `host`, so the code a real deployment
//! would ship is exactly the code the simulation tests.
//!
//! The replica and the group client arrive with tickets D1 and D3.

pub mod effect;
pub mod input;
pub mod message;
pub mod node;

pub use effect::Effect;
pub use input::Input;
pub use node::Node;
