//! An emoji reaction on a message, and a tally of them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{MessageId, UserId};

/// One user's reaction to a message with a given emoji. Unique per
/// (message, user, emoji).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Reaction {
    pub message_id: MessageId,
    pub user: UserId,
    /// The emoji: a unicode glyph (`"👍"`) or a custom `:name:`.
    pub emoji: String,
}

impl Reaction {
    pub fn new(message_id: MessageId, user: UserId, emoji: impl Into<String>) -> Self {
        Self {
            message_id,
            user,
            emoji: emoji.into(),
        }
    }
}

/// Count reactions per emoji for a single message, returned in a stable
/// (emoji-sorted) order for deterministic display.
pub fn summarize_reactions(reactions: &[Reaction]) -> Vec<(String, u64)> {
    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    for r in reactions {
        *counts.entry(r.emoji.as_str()).or_insert(0) += 1;
    }
    counts.into_iter().map(|(e, n)| (e.to_string(), n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_counts_per_emoji_in_stable_order() {
        let rs = vec![
            Reaction::new(MessageId(1), UserId(1), "👍"),
            Reaction::new(MessageId(1), UserId(2), "👍"),
            Reaction::new(MessageId(1), UserId(3), "🎉"),
        ];
        assert_eq!(
            summarize_reactions(&rs),
            vec![("🎉".to_string(), 1), ("👍".to_string(), 2)]
        );
    }
}
