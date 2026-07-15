//! Persistence for platform-wide user accounts.

use domain::{User, UserId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for platform-wide user accounts.
#[async_trait]
pub trait UserStore: Send + Sync {
    /// Allocate a fresh, unused user id.
    async fn next_user_id(&self) -> Result<UserId, StoreError>;
    async fn insert_user(&self, user: User) -> Result<(), StoreError>;
    /// Replace an existing account (e.g. a DM-policy change).
    async fn update_user(&self, user: User) -> Result<(), StoreError>;
    async fn get_user(&self, id: UserId) -> Result<Option<User>, StoreError>;
    async fn find_by_handle(&self, handle: &str) -> Result<Option<User>, StoreError>;
    /// Every account. Used at federation startup to claim the user-home scopes this
    /// node minted (the node that allocated a user's id homes them).
    async fn list_all(&self) -> Result<Vec<User>, StoreError>;
    /// Accounts carrying the exact tag `tag` (a plain tag — the store handles any
    /// normalization/fencing internally). Order is unspecified.
    async fn search_by_tag(&self, tag: &str) -> Result<Vec<User>, StoreError>;
}
