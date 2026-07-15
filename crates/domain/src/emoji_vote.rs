//! One citizen's up/down vote on a custom emoji.

use serde::{Deserialize, Serialize};

use crate::{EmojiId, ServerId, UserId};

/// A citizen's vote on an emoji: `is_up` = a thumbs-up, `false` = thumbs-down. One
/// per (emoji, voter) — re-voting replaces the prior one. Carries `server_id` so a
/// replicated vote can be scoped to its server without a lookup (federation).
///
/// **Only currently-franchised citizens' votes count** toward an emoji's score; the
/// app layer applies that filter when tallying, so revoking someone's franchise
/// retroactively drops their weight without touching stored votes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct EmojiVote {
    pub server_id: ServerId,
    pub emoji_id: EmojiId,
    pub voter: UserId,
    pub is_up: bool,
}

impl EmojiVote {
    pub fn new(server_id: ServerId, emoji_id: EmojiId, voter: UserId, is_up: bool) -> Self {
        Self { server_id, emoji_id, voter, is_up }
    }
}
