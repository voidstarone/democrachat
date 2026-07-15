//! The largest local sequence a single node can mint.

use crate::node::sequence_mask::SEQUENCE_MASK;

/// The largest local sequence a single node can mint (2^48 − 1). Reaching it is
/// not possible in practice, but an allocator must treat exhaustion as an error
/// rather than let the sequence wrap into the node field.
pub const MAX_SEQUENCE: u64 = SEQUENCE_MASK;
