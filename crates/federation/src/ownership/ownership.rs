//! Who owns a scope, and under which epoch.

use domain::NodeId;

use crate::ownership::owned_scope::OwnedScope;

/// Who owns a scope (a server or a user-home), and under which epoch. The epoch is
/// monotonic per scope and survives handoff, so a returning stale owner is fenced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ownership {
    pub scope: OwnedScope,
    pub owner: NodeId,
    pub epoch: u64,
}
