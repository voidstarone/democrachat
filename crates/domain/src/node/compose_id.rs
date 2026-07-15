//! Combine a node id and a sequence into a globally-unique composite ID.

use crate::node::node_id::NodeId;
use crate::node::sequence_bits::SEQUENCE_BITS;
use crate::node::sequence_mask::SEQUENCE_MASK;

/// Combine a node id and a per-node sequence into a globally-unique composite ID.
///
/// The sequence is masked to 48 bits; a caller that overruns
/// [`MAX_SEQUENCE`](crate::node::max_sequence::MAX_SEQUENCE) would silently wrap,
/// so allocators must treat sequence exhaustion as an error. With `NodeId(0)` the
/// high bits stay clear, so `compose_id(NodeId(0), n) == n` — the single-box
/// deployment's IDs are numerically identical to the bare `1, 2, 3…` scheme.
#[inline]
pub const fn compose_id(node: NodeId, sequence: u64) -> u64 {
    ((node.0 as u64) << SEQUENCE_BITS) | (sequence & SEQUENCE_MASK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::local_sequence::local_sequence;
    use crate::node::max_sequence::MAX_SEQUENCE;
    use crate::node::origin_node::origin_node;

    #[test]
    fn single_box_ids_are_bare_sequences() {
        // Node 0 leaves the high bits clear, so an un-federated deployment's IDs
        // match the old 1, 2, 3 scheme — existing data + the JSON store keep working.
        assert_eq!(compose_id(NodeId(0), 1), 1);
        assert_eq!(compose_id(NodeId(0), 42), 42);
    }

    #[test]
    fn compose_then_decompose_round_trips() {
        for &(node, seq) in &[(0u16, 1u64), (1, 1), (7, 500), (65535, MAX_SEQUENCE)] {
            let id = compose_id(NodeId(node), seq);
            assert_eq!(origin_node(id), NodeId(node));
            assert_eq!(local_sequence(id), seq);
        }
    }

    #[test]
    fn distinct_nodes_never_collide() {
        // Same local sequence on different nodes → different global IDs, with no
        // coordination. This is the whole point of the scheme.
        let a = compose_id(NodeId(1), 5);
        let b = compose_id(NodeId(2), 5);
        assert_ne!(a, b);
        assert_eq!(local_sequence(a), local_sequence(b));
        assert_ne!(origin_node(a), origin_node(b));
    }

    #[test]
    fn sequence_is_masked_to_48_bits() {
        // A sequence at the ceiling cannot bleed into the node field.
        let id = compose_id(NodeId(3), MAX_SEQUENCE);
        assert_eq!(origin_node(id), NodeId(3));
        assert_eq!(local_sequence(id), MAX_SEQUENCE);
    }
}
