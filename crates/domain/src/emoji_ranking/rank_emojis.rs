//! Assign each emoji a standing from its net vote score.

use crate::emoji_ranking::emoji_slots::{ACTIVE_EMOJI_SLOTS, CONSIDERED_EMOJI_SLOTS};
use crate::emoji_ranking::emoji_standing::EmojiStanding;

/// Classify emoji by rank: highest net score (upvotes − downvotes) first, the top
/// [`ACTIVE_EMOJI_SLOTS`] **active**, the next [`CONSIDERED_EMOJI_SLOTS`]
/// **considered**, the rest **archived**. Returns standings **parallel to `scores`**
/// (same order in, same order out).
///
/// Ties are broken by input order: pass emoji in a deterministic order (e.g. oldest
/// first, by id) and equal scores keep it — so ranking is stable and a newer emoji
/// never displaces an equally-scored older one.
pub fn rank_emojis(scores: &[i64]) -> Vec<EmojiStanding> {
    // Rank indices by score, descending; the sort is stable, so equal scores retain
    // their input order (the caller's deterministic tie-break).
    let mut ranked: Vec<usize> = (0..scores.len()).collect();
    ranked.sort_by(|&a, &b| scores[b].cmp(&scores[a]));

    let mut standings = vec![EmojiStanding::Archived; scores.len()];
    for (rank, &i) in ranked.iter().enumerate() {
        standings[i] = if rank < ACTIVE_EMOJI_SLOTS {
            EmojiStanding::Active
        } else if rank < ACTIVE_EMOJI_SLOTS + CONSIDERED_EMOJI_SLOTS {
            EmojiStanding::Considered
        } else {
            EmojiStanding::Archived
        };
    }
    standings
}
