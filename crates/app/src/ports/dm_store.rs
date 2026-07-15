//! Persistence for direct messages.

use domain::{DmId, DmMessage, UserId};
use crate::StoreError;

/// Persistence for direct messages.
pub trait DmStore: Send + Sync {
    fn next_dm_id(&self) -> Result<DmId, StoreError>;
    fn insert_dm(&self, message: DmMessage) -> Result<(), StoreError>;
    /// The full conversation between two users, in id order (which is send order).
    fn conversation(&self, a: UserId, b: UserId) -> Result<Vec<DmMessage>, StoreError>;
    /// Every distinct user `who` has exchanged a DM with, most-recent first.
    fn partners(&self, who: UserId) -> Result<Vec<UserId>, StoreError>;
}
