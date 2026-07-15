//! Persistence for friendships and pending friend requests.

use domain::{Friendship, UserId};

/// Persistence for friendships and pending friend requests.
pub trait FriendStore: Send + Sync {
    /// Record a new pending request; returns `false` if any record (pending or
    /// accepted) already exists between the two.
    fn add(&self, friendship: Friendship) -> bool;
    /// Replace the record between two users (e.g. on accept).
    fn update(&self, friendship: Friendship);
    /// The record between `a` and `b`, if any, in either direction.
    fn between(&self, a: UserId, b: UserId) -> Option<Friendship>;
    /// Every friendship record involving `who`, in either role.
    fn involving(&self, who: UserId) -> Vec<Friendship>;
}
