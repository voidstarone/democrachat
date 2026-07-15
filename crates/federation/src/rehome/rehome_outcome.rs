//! What one rehoming evaluation of a scope concluded.

use domain::NodeId;

use crate::ownership::owned_scope::OwnedScope;

/// What one rehoming evaluation of a scope concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RehomeOutcome {
    /// Still has a live owner — nothing to do.
    StillOwned { scope: OwnedScope },
    /// This node took over as the new owner at `epoch`.
    Promoted { scope: OwnedScope, epoch: u64 },
    /// Unowned, but another node is the better (quieter) candidate — leave it.
    Yielded { scope: OwnedScope, to: NodeId },
    /// Unowned and no live standby can take it (operator attention needed).
    Stranded { scope: OwnedScope },
    /// Unowned, but the community has **disabled rehoming** for this scope, so it
    /// is deliberately left down until its home node returns (sovereignty over
    /// availability — the citizens' choice).
    RehomingDisabled { scope: OwnedScope },
}
