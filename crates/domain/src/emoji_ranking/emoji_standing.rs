//! Where a custom emoji sits in its server's ranked pool.

use serde::{Deserialize, Serialize};

/// An emoji's tier in the server's continuously-voted ranking. Emoji never leave
/// the pool (so a `:name:` in an old message always renders); their standing just
/// reflects their current rank by net vote score.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmojiStanding {
    /// Top [`ACTIVE_EMOJI_SLOTS`](super::emoji_slots::ACTIVE_EMOJI_SLOTS): usable in
    /// the picker and new messages.
    Active,
    /// The next [`CONSIDERED_EMOJI_SLOTS`](super::emoji_slots::CONSIDERED_EMOJI_SLOTS):
    /// candidates shown in the vote list, not yet in the picker.
    Considered,
    /// Below the cut: kept only so existing messages that used it still render.
    Archived,
}

impl EmojiStanding {
    /// Usable in the picker / a new message.
    pub fn is_active(self) -> bool {
        matches!(self, EmojiStanding::Active)
    }

    /// Shown in the vote list (active or considered), i.e. not archived.
    pub fn is_listed(self) -> bool {
        matches!(self, EmojiStanding::Active | EmojiStanding::Considered)
    }

    /// The standing's canonical wire tag (matches the serde representation) — the
    /// single home for the string form.
    pub const fn as_str(self) -> &'static str {
        match self {
            EmojiStanding::Active => "active",
            EmojiStanding::Considered => "considered",
            EmojiStanding::Archived => "archived",
        }
    }
}
