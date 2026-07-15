//! The lifecycle status of a proposal.

use serde::{Deserialize, Serialize};

use crate::Timestamp;

/// The lifecycle status of a proposal.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ProposalStatus {
    /// Accepting votes until `closes_at`.
    Open,
    /// Passed. For constitutional changes, `effective_at` is in the future
    /// (the timelock); for everything else it equals the close time.
    Passed { effective_at: Timestamp },
    /// Did not meet its threshold.
    Failed,
}
