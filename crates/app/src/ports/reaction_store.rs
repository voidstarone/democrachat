//! Persistence for reactions.

use domain::{MessageId, Reaction, UserId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for reactions.
#[async_trait]
pub trait ReactionStore: Send + Sync {
    /// Add a reaction; returns `false` if that exact (message, user, emoji)
    /// reaction already exists (idempotent).
    async fn add(&self, reaction: Reaction) -> Result<bool, StoreError>;
    /// Remove a reaction; returns `false` if it wasn't present.
    async fn remove(&self, message: MessageId, user: UserId, emoji: &str) -> Result<bool, StoreError>;
    async fn list_for_message(&self, message: MessageId) -> Result<Vec<Reaction>, StoreError>;
    /// Whether `user` has *any* reaction on `message` (used to count an
    /// endorsement once, regardless of how many emojis they add).
    async fn user_has_any(&self, message: MessageId, user: UserId) -> Result<bool, StoreError>;
}
