//! Bits reserved for the per-node sequence.

/// Bits reserved for the per-node sequence — the low part of a composite ID.
/// 48 bits is ~281 trillion IDs per node; the remaining 16 name the node.
pub const SEQUENCE_BITS: u32 = 48;
