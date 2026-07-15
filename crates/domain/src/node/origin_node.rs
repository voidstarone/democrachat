//! Recover the origin node from a composite ID.

use crate::node::node_id::NodeId;
use crate::node::sequence_bits::SEQUENCE_BITS;

/// Recover the **origin node** — the node that minted `id`. This is the entity's
/// bootstrap owner, a good routing default; the *current* owner (servers rehome)
/// is authoritative only in the control plane.
#[inline]
pub const fn origin_node(id: u64) -> NodeId {
    NodeId((id >> SEQUENCE_BITS) as u16)
}
