//! Identifies a custom emoji (stable across its whole life, so an old message's
//! `:name:` always resolves even after the emoji is demoted).

use serde::{Deserialize, Serialize};

/// Identifies a custom emoji within the federation. Assigned once and never reused;
/// an emoji is retained for good so any message that used it still renders.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct EmojiId(pub u64);

impl std::fmt::Display for EmojiId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
