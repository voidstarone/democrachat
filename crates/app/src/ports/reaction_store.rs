//! Persistence for reactions.

use domain::{MessageId, Reaction, UserId};

/// Persistence for reactions.
pub trait ReactionStore: Send + Sync {
    /// Add a reaction; returns `false` if that exact (message, user, emoji)
    /// reaction already exists (idempotent).
    fn add(&self, reaction: Reaction) -> bool;
    /// Remove a reaction; returns `false` if it wasn't present.
    fn remove(&self, message: MessageId, user: UserId, emoji: &str) -> bool;
    fn list_for_message(&self, message: MessageId) -> Vec<Reaction>;
    /// Whether `user` has *any* reaction on `message` (used to count an
    /// endorsement once, regardless of how many emojis they add).
    fn user_has_any(&self, message: MessageId, user: UserId) -> bool;
}
