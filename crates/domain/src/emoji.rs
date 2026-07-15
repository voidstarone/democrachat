//! A server's custom emoji — added by a citizen and ranked by continuous voting.

use serde::{Deserialize, Serialize};

use crate::{EmojiId, ServerId, Timestamp, UserId};

/// A custom emoji belonging to a server: a short `:name:` and the image (a `data:`
/// URI for an uploaded ≤256×256 PNG/GIF, or an external URL) it renders as.
///
/// Any citizen may add one; the pool is then curated by continuous up/down voting
/// (see [`crate::emoji_ranking`]). An emoji is **retained for good** — never hard
/// deleted — so a `:name:` in an old message always resolves even after the emoji
/// falls out of the active/considered tiers.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Emoji {
    pub id: EmojiId,
    pub server_id: ServerId,
    /// Normalized short name (no colons), unique per server for all time.
    pub name: String,
    pub url: String,
    /// The citizen who added it.
    pub added_by: UserId,
    pub added_at: Timestamp,
}

impl Emoji {
    pub fn new(
        id: EmojiId,
        server_id: ServerId,
        name: impl Into<String>,
        url: impl Into<String>,
        added_by: UserId,
        added_at: Timestamp,
    ) -> Self {
        Self {
            id,
            server_id,
            name: name.into(),
            url: url.into(),
            added_by,
            added_at,
        }
    }
}
