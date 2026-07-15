//! Persistence for a server's custom emoji — a retained, id-keyed pool.

use domain::{Emoji, EmojiId, ServerId};
use crate::StoreError;

/// Persistence for custom emoji. Emoji are **retained forever** (no delete), so a
/// `:name:` in an old message always resolves; the vote ranking decides which are
/// active, not whether a row exists.
pub trait EmojiStore: Send + Sync {
    /// Mint the next emoji id (node-stamped, like every other id).
    fn next_emoji_id(&self) -> Result<EmojiId, StoreError>;
    fn insert_emoji(&self, emoji: Emoji) -> Result<(), StoreError>;
    fn get_emoji(&self, id: EmojiId) -> Result<Option<Emoji>, StoreError>;
    /// The current emoji with this normalized name on the server (names are unique
    /// per server for all time).
    fn find_emoji(&self, server: ServerId, name: &str) -> Result<Option<Emoji>, StoreError>;
    fn list_for_server(&self, server: ServerId) -> Result<Vec<Emoji>, StoreError>;
}
