//! Persistence for direct messages.

use domain::{DmId, DmMessage, UserId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for direct messages.
#[async_trait]
pub trait DmStore: Send + Sync {
    async fn next_dm_id(&self) -> Result<DmId, StoreError>;
    async fn insert_dm(&self, message: DmMessage) -> Result<(), StoreError>;
    /// The full conversation between two users, in id order (which is send order).
    async fn conversation(&self, a: UserId, b: UserId) -> Result<Vec<DmMessage>, StoreError>;
    /// Every distinct user `who` has exchanged a DM with, most-recent first.
    async fn partners(&self, who: UserId) -> Result<Vec<UserId>, StoreError>;
}
