//! Persistence for citizens' role-colour votes.

use domain::{RoleColor, RoleColorVote, RoleId, ServerId, UserId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for votes on a custom role's display colour. Votes are stored raw;
/// the app layer tallies only *currently-franchised* citizens' votes when picking
/// the winning colour, so a change of franchise re-weights the result without
/// rewriting history — mirrors [`EmojiVoteStore`](crate::EmojiVoteStore).
#[async_trait]
pub trait RoleColorVoteStore: Send + Sync {
    /// Record `vote`, replacing any prior vote by the same voter on the same role.
    async fn upsert_role_color_vote(&self, vote: RoleColorVote) -> Result<(), StoreError>;
    /// Every role-colour vote cast in `server`.
    async fn role_color_votes_for_server(&self, server: ServerId) -> Result<Vec<RoleColorVote>, StoreError>;
    /// This voter's current colour for `role`, if any.
    async fn my_role_color_vote(&self, role: RoleId, voter: UserId) -> Result<Option<RoleColor>, StoreError>;
}
