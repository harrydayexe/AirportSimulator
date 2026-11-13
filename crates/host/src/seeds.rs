//! Contains the source of randomness for a run

use det::{NodeId, Rng};

use crate::hash::mix;

/// What a generator is used for. Each purpose gets its own stream, so that changing one (say, the
/// fault settings) does not shift the numbers another sees (say, the message delays).
#[derive(Clone, Copy, Debug)]
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

// S1: two streams sharing a tag would share their numbers.
const _: () = {
    let tags = [TAG_NETWORK, TAG_FAULTS, TAG_NODE, TAG_CHILD];
    let mut i = 0;
    while i < tags.len() {
        let mut j = i + 1;
        while j < tags.len() {
            assert!(tags[i] != tags[j], "seed stream tags must be distinct");
            j += 1;
        }
        i += 1;
    }
};

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
    ///
    /// # Panics
    /// If the child's root equals this one's (S2), which would replay this
    /// host's streams. Only a hash collision could cause it.
    #[must_use]
    pub fn child(&self, index: u32) -> Seeds {
        let root = mix(mix(self.root, TAG_CHILD), u64::from(index));
        assert_ne!(
            root, self.root,
            "Seeds::child({index}) shares its parent's root"
        );
        Seeds::new(root)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn first(seeds: Seeds, stream: Stream) -> u64 {
        seeds.rng(stream).next_u64()
    }

    fn node(id: u32, life: u32) -> Stream {
        Stream::Node {
            id: NodeId(id),
            life,
        }
    }

    #[test]
    fn same_seed_same_numbers_and_streams_are_independent() {
        let take = |seeds: Seeds, stream| {
            let mut rng = seeds.rng(stream);
            (0..10).map(|_| rng.next_u64()).collect::<Vec<_>>()
        };
        assert_eq!(
            take(Seeds::new(5), Stream::Network),
            take(Seeds::new(5), Stream::Network)
        );
        assert_ne!(
            take(Seeds::new(5), Stream::Network),
            take(Seeds::new(6), Stream::Network)
        );
        assert_ne!(
            take(Seeds::new(5), Stream::Network),
            take(Seeds::new(5), Stream::Faults)
        );
    }

    #[test]
    fn streams_are_pinned() {
        // A changed value means every recorded seed replays differently.
        let seeds = Seeds::new(42);
        assert_eq!(first(seeds, Stream::Network), 0x19e4_79e2_aaa7_7bfb);
        assert_eq!(first(seeds, Stream::Faults), 0x3490_f387_e177_0a3d);
        assert_eq!(first(seeds, node(1, 0)), 0x359a_1bff_70b8_c9e4);
    }

    #[test]
    fn asking_for_one_stream_does_not_disturb_another() {
        let seeds = Seeds::new(42);
        let before = first(seeds, Stream::Network);
        let _ = seeds.rng(Stream::Faults);
        let _ = seeds.rng(node(3, 2));
        assert_eq!(first(seeds, Stream::Network), before);
    }

    #[test]
    fn no_two_streams_share_a_seed() {
        let mut seen = BTreeSet::new();
        let mut count = 0;
        for root in 0..8 {
            let seeds = Seeds::new(root);
            let mut streams = vec![Stream::Network, Stream::Faults];
            for id in 0..5 {
                for life in 0..4 {
                    streams.push(node(id, life));
                }
            }
            for stream in streams {
                seen.insert(first(seeds, stream));
                count += 1;
            }
        }
        assert_eq!(count, 176);
        assert_eq!(seen.len(), 176);
        let seeds = Seeds::new(0);
        assert_ne!(first(seeds, node(1, 0)), first(seeds, node(0, 1)));
    }

    #[test]
    fn each_life_of_a_node_gets_fresh_numbers() {
        let seeds = Seeds::new(9);
        assert_ne!(first(seeds, node(2, 0)), first(seeds, node(2, 1)));
    }

    #[test]
    fn children_get_their_own_seeds() {
        let parent = Seeds::new(42);
        let values = [
            first(parent, Stream::Network),
            first(parent.child(0), Stream::Network),
            first(parent.child(1), Stream::Network),
        ];
        assert_eq!(values.iter().collect::<BTreeSet<_>>().len(), 3);
        assert_eq!(first(parent.child(0), Stream::Network), values[1]);
    }
}
