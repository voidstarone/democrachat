//! Recover the per-node sequence from a composite ID.

use crate::node::sequence_mask::SEQUENCE_MASK;

/// Recover the per-node sequence encoded in `id` (the low 48 bits).
#[inline]
pub const fn local_sequence(id: u64) -> u64 {
    id & SEQUENCE_MASK
}
