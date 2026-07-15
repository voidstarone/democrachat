//! The result of trying to claim a scope.

use domain::NodeId;

/// The result of trying to
/// [`OwnershipRegistry::claim`](crate::ownership::ownership_registry::OwnershipRegistry::claim)
/// a scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// This node now owns the scope, at the freshly bumped `epoch`.
    Claimed { epoch: u64 },
    /// Another live node already holds it; not claimed.
    Held { by: NodeId, epoch: u64 },
}
