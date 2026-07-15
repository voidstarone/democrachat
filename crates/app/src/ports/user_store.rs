//! Persistence for platform-wide user accounts.

use domain::{User, UserId};
use crate::StoreError;

/// Persistence for platform-wide user accounts.
pub trait UserStore: Send + Sync {
    /// Allocate a fresh, unused user id.
    fn next_user_id(&self) -> Result<UserId, StoreError>;
    fn insert_user(&self, user: User) -> Result<(), StoreError>;
    /// Replace an existing account (e.g. a DM-policy change).
    fn update_user(&self, user: User) -> Result<(), StoreError>;
    fn get_user(&self, id: UserId) -> Result<Option<User>, StoreError>;
    fn find_by_handle(&self, handle: &str) -> Result<Option<User>, StoreError>;
    /// Every account. Used at federation startup to claim the user-home scopes this
    /// node minted (the node that allocated a user's id homes them).
    fn list_all(&self) -> Result<Vec<User>, StoreError>;
}
