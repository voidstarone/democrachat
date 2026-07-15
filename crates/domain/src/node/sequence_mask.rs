//! Mask selecting the sequence portion of a composite ID.

use crate::node::sequence_bits::SEQUENCE_BITS;

/// Mask selecting the sequence portion of a composite ID (the low 48 bits).
pub const SEQUENCE_MASK: u64 = (1u64 << SEQUENCE_BITS) - 1;
