//! Persistence for friendships and pending friend requests.

use domain::{Friendship, UserId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for friendships and pending friend requests.
#[async_trait]
pub trait FriendStore: Send + Sync {
    /// Record a new pending request; returns `false` if any record (pending or
    /// accepted) already exists between the two.
    async fn add(&self, friendship: Friendship) -> Result<bool, StoreError>;
    /// Replace the record between two users (e.g. on accept).
    async fn update(&self, friendship: Friendship) -> Result<(), StoreError>;
    /// The record between `a` and `b`, if any, in either direction.
    async fn between(&self, a: UserId, b: UserId) -> Result<Option<Friendship>, StoreError>;
    /// Every friendship record involving `who`, in either role.
    async fn involving(&self, who: UserId) -> Result<Vec<Friendship>, StoreError>;
}
