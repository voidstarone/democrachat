//! Persistence for citizens' emoji votes.

use domain::{EmojiId, EmojiVote, ServerId, UserId};
use crate::StoreError;

/// Persistence for up/down votes on custom emoji. Votes are stored raw; the app
/// layer tallies only *currently-franchised* citizens' votes when ranking, so a
/// change of franchise re-weights scores without rewriting history.
pub trait EmojiVoteStore: Send + Sync {
    /// Record `vote`, replacing any prior vote by the same voter on the same emoji.
    fn upsert_emoji_vote(&self, vote: EmojiVote) -> Result<(), StoreError>;
    /// Every vote cast on any emoji of `server`.
    fn emoji_votes_for_server(&self, server: ServerId) -> Result<Vec<EmojiVote>, StoreError>;
    /// This voter's current vote on `emoji`, if any (`Some(true)` = up).
    fn my_emoji_vote(&self, emoji: EmojiId, voter: UserId) -> Result<Option<bool>, StoreError>;
}
