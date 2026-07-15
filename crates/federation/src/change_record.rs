//! One row captured in a node's change-capture outbox.

use serde::{Deserialize, Serialize};

use crate::change_op::ChangeOp;

/// A single mutation recorded in a node's outbox, before it is signed for the
/// feed. The scope is **not** stored — it is derived from the payload by
/// [`classify`](crate::classify::classify), the same way the consumer derives it,
/// so producer and consumer can never disagree about which scope a row belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeRecord {
    /// This node's monotonic outbox sequence — the consumer's replay cursor.
    pub seq: u64,
    /// The entity/store the row belongs to (e.g. `"messages"`, `"votes"`, `"dms"`).
    pub entity: String,
    pub op: ChangeOp,
    /// The row as JSON (ciphertext for an E2EE entity — carried opaquely).
    pub payload: serde_json::Value,
}
