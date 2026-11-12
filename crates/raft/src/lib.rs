//! Pure logic, in the same shape as the airport crate: messages and timer
//! ticks in, messages and committed entries out. No clock or network of its
//! own; the host provides both.

pub mod browser_node;
pub mod node;
