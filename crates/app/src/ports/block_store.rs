//! Persistence for permanent user blocks.

use domain::{Block, UserId};

/// Persistence for permanent user blocks.
pub trait BlockStore: Send + Sync {
    /// Record a block; returns `false` if `blocker` had already blocked `blocked`
    /// (idempotent — a block is permanent, so re-blocking is a no-op).
    fn add(&self, block: Block) -> bool;
    /// Every block involving `who`, in either direction.
    fn involving(&self, who: UserId) -> Vec<Block>;
    /// Whether a block stands between `a` and `b` in either direction.
    fn is_blocked_between(&self, a: UserId, b: UserId) -> bool;
}
