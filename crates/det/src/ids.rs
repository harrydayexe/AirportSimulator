//! Identifier types. Each wraps a fixed-width integer so that one kind of
//! number cannot be passed where another is expected:
//!
//! ```compile_fail
//! use det::{Index, Term};
//! fn wants_term(_: Term) {}
//! wants_term(Index(3)); // a log position is not an election term
//! ```
//!
//! ```compile_fail
//! use det::{ClientId, NodeId};
//! fn wants_node(_: NodeId) {}
//! wants_node(ClientId(1)); // a client is not a node
//! ```
//!
//! All of them serialise as the bare number.

use std::{fmt, ops::Add};

use serde::{Deserialize, Serialize};

macro_rules! id {
    ($(#[$doc:meta])* $name:ident($inner:ty)) => {
        $(#[$doc])*
        #[derive(
            Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub $inner);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id! {
    /// One participant in Raft.
    NodeId(u32)
}

id! {
    /// Anything that sends commands to the airport group and waits for replies.
    ClientId(u32)
}

id! {
    /// An election term (Raft's "term"). 0 is the term of a brand new node,
    /// before any election.
    Term(u64)
}

id! {
    /// A position in a log. Positions start at 1; 0 means "before the first
    /// entry".
    Index(u64)
}

id! {
    /// A timestamp provided with each message.
    Ms(u64)
}

impl From<Ms> for u64 {
    fn from(val: Ms) -> Self {
        val.0
    }
}

impl Add for Ms {
    fn add(self, rhs: Self) -> Self::Output {
        Ms(self.0 + rhs.0)
    }

    type Output = Ms;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint;

    #[test]
    fn serialise_as_bare_numbers() {
        assert_eq!(serde_json::to_string(&NodeId(2)).unwrap(), "2");
        assert_eq!(serde_json::to_string(&ClientId(9)).unwrap(), "9");
        assert_eq!(serde_json::to_string(&Term(7)).unwrap(), "7");
        assert_eq!(serde_json::from_str::<Index>("41").unwrap(), Index(41));
    }

    #[test]
    fn order_and_hash_follow_the_number() {
        assert!(Term(1) < Term(2));
        assert!(Index(10) > Index(9));
        assert_eq!(fingerprint(&Term(5)), fingerprint(&5u64));
        assert_eq!(fingerprint(&NodeId(5)), fingerprint(&5u32));
    }
}
