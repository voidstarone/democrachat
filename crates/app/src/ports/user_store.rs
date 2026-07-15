//! Persistence for platform-wide user accounts.

use domain::{User, UserId};

/// Persistence for platform-wide user accounts.
pub trait UserStore: Send + Sync {
    /// Allocate a fresh, unused user id.
    fn next_user_id(&self) -> UserId;
    fn insert_user(&self, user: User);
    /// Replace an existing account (e.g. a DM-policy change).
    fn update_user(&self, user: User);
    fn get_user(&self, id: UserId) -> Option<User>;
    fn find_by_handle(&self, handle: &str) -> Option<User>;
    /// Every account. Used at federation startup to claim the user-home scopes this
    /// node minted (the node that allocated a user's id homes them).
    fn list_all(&self) -> Vec<User>;
}
