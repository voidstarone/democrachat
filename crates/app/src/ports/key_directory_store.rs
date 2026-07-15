//! Persistence for the server-blind key directory.

use domain::{UserId, UserKeys};
use crate::StoreError;
use async_trait::async_trait;

/// Stores each user's published device keys ([`UserKeys`]) — one entry per user,
/// replacing any previous one when re-published. The store treats the entry as
/// opaque: the wrapped secret is never inspected, only held and handed back.
#[async_trait]
pub trait KeyDirectoryStore: Send + Sync {
    /// Publish (or replace) a user's directory entry.
    async fn put_keys(&self, keys: UserKeys) -> Result<(), StoreError>;
    /// The user's current directory entry, if they have published one.
    async fn get_keys(&self, user: UserId) -> Result<Option<UserKeys>, StoreError>;
}
