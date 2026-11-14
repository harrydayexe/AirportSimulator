//! Pure logic, in the same shape as the airport crate: messages and timer
//! wakes in, messages and committed entries out. No clock, network or disk of
//! its own: the node that runs a [`Member`] provides all three, whether that
//! node sits in the simulated host or in a process of its own.

pub mod member;

pub use member::Member;
