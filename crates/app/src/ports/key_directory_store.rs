//! Persistence for the server-blind key directory.

use domain::{UserId, UserKeys};

/// Stores each user's published device keys ([`UserKeys`]) — one entry per user,
/// replacing any previous one when re-published. The store treats the entry as
/// opaque: the wrapped secret is never inspected, only held and handed back.
pub trait KeyDirectoryStore: Send + Sync {
    /// Publish (or replace) a user's directory entry.
    fn put_keys(&self, keys: UserKeys);
    /// The user's current directory entry, if they have published one.
    fn get_keys(&self, user: UserId) -> Option<UserKeys>;
}
