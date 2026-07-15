//! Persistence for a server's custom emoji — a retained, id-keyed pool.

use domain::{Emoji, EmojiId, ServerId};

/// Persistence for custom emoji. Emoji are **retained forever** (no delete), so a
/// `:name:` in an old message always resolves; the vote ranking decides which are
/// active, not whether a row exists.
pub trait EmojiStore: Send + Sync {
    /// Mint the next emoji id (node-stamped, like every other id).
    fn next_emoji_id(&self) -> EmojiId;
    fn insert_emoji(&self, emoji: Emoji);
    fn get_emoji(&self, id: EmojiId) -> Option<Emoji>;
    /// The current emoji with this normalized name on the server (names are unique
    /// per server for all time).
    fn find_emoji(&self, server: ServerId, name: &str) -> Option<Emoji>;
    fn list_for_server(&self, server: ServerId) -> Vec<Emoji>;
}
