//! Persistence for permanent user blocks.

use domain::{Block, UserId};
use crate::StoreError;

/// Persistence for permanent user blocks.
pub trait BlockStore: Send + Sync {
    /// Record a block; returns `false` if `blocker` had already blocked `blocked`
    /// (idempotent — a block is permanent, so re-blocking is a no-op).
    fn add(&self, block: Block) -> Result<bool, StoreError>;
    /// Every block involving `who`, in either direction.
    fn involving(&self, who: UserId) -> Result<Vec<Block>, StoreError>;
    /// Whether a block stands between `a` and `b` in either direction.
    fn is_blocked_between(&self, a: UserId, b: UserId) -> Result<bool, StoreError>;
}
