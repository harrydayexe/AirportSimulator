//! Contains the source of randomness for a run

use det::{NodeId, Rng};

use crate::hash::mix;

/// What a generator is used for. Each purpose gets its own stream, so that changing one (say, the
/// fault settings) does not shift the numbers another sees (say, the message delays).
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum Stream {
    /// Message delays, loss and duplication.
    Network,
    /// The fault schedule.
    Faults,
    /// One life of one node.
    Node {
        /// The ID of the node.
        id: NodeId,
        /// `life` goes up by on one each restart.
        life: u32,
    },
}

const TAG_NETWORK: u64 = 1;
const TAG_FAULTS: u64 = 2;
const TAG_NODE: u64 = 3;
const TAG_CHILD: u64 = 4;

/// Gives every generator in a run, all from one number.
#[derive(Clone, Copy, Debug)]
pub struct Seeds {
    /// The root number which drives all randomness.
    root: u64,
}

impl Seeds {
    /// Seeds for a whole run.
    #[must_use]
    pub fn new(root: u64) -> Self {
        Self { root }
    }

    /// A fresh generator for `stream`. The same root and stream always give
    /// the same generator, whatever else has been asked for before.
    #[must_use]
    pub fn rng(&self, stream: Stream) -> Rng {
        let seed = match stream {
            Stream::Network => mix(self.root, TAG_NETWORK),
            Stream::Faults => mix(self.root, TAG_FAULTS),
            Stream::Node { id, life } => mix(
                mix(mix(self.root, TAG_NODE), u64::from(id.0)),
                u64::from(life),
            ),
        };

        Rng::new(seed)
    }

    /// The seeds for child host `index`.
    #[must_use]
    pub fn child(&self, index: u32) -> Seeds {
        Seeds::new(mix(mix(self.root, TAG_CHILD), u64::from(index)))
    }
}
