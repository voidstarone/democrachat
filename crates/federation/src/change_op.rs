//! Whether an event records a row written/updated or deleted on the owner.

use serde::{Deserialize, Serialize};

/// Whether an event records a row written/updated or deleted on the owner.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeOp {
    Upsert,
    Delete,
}
