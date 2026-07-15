//! Persistence for direct messages.

use domain::{DmId, DmMessage, UserId};

/// Persistence for direct messages.
pub trait DmStore: Send + Sync {
    fn next_dm_id(&self) -> DmId;
    fn insert_dm(&self, message: DmMessage);
    /// The full conversation between two users, in id order (which is send order).
    fn conversation(&self, a: UserId, b: UserId) -> Vec<DmMessage>;
    /// Every distinct user `who` has exchanged a DM with, most-recent first.
    fn partners(&self, who: UserId) -> Vec<UserId>;
}
