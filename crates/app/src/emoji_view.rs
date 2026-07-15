//! A ranked emoji as shown in the server-settings vote list.

use domain::EmojiStanding;

/// One emoji in a server's vote list: its current net score (from citizens only),
/// tier, and how the viewer voted. The web layer maps this to its DTO.
pub struct RankedEmoji {
    pub id: u64,
    pub name: String,
    pub url: String,
    /// Net score: citizen upvotes − downvotes.
    pub score: i64,
    pub standing: EmojiStanding,
    /// The viewing citizen's own vote, if any (`Some(true)` = up).
    pub my_vote: Option<bool>,
}
