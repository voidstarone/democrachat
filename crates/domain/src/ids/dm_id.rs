//! Identifies a direct message.

use serde::{Deserialize, Serialize};

/// Identifies a direct message.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct DmId(pub u64);

impl std::fmt::Display for DmId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
