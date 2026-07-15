//! A server's custom-emoji pool is a continuously-voted ranking: members up/down
//! vote emoji, and their net score sorts them into tiers — the top
//! [`ACTIVE_EMOJI_SLOTS`](emoji_slots::ACTIVE_EMOJI_SLOTS) usable in messages, the
//! next [`CONSIDERED_EMOJI_SLOTS`](emoji_slots::CONSIDERED_EMOJI_SLOTS) as
//! candidates, the rest archived (kept only so old messages still render). Only
//! active members' (citizens') votes count toward the score; that rule lives in the
//! app layer, which supplies the already-tallied scores here.

pub mod emoji_slots;
pub mod emoji_standing;
pub mod normalize_emoji_name;
pub mod rank_emojis;

#[cfg(test)]
mod tests {
    use super::emoji_slots::{ACTIVE_EMOJI_SLOTS, CONSIDERED_EMOJI_SLOTS};
    use super::emoji_standing::EmojiStanding::{Active, Archived, Considered};
    use super::rank_emojis::rank_emojis;

    #[test]
    fn a_small_pool_is_all_active() {
        // Scores parallel to input; the two highest are active, order preserved.
        assert_eq!(rank_emojis(&[3, 1, 2]), vec![Active, Active, Active]);
    }

    #[test]
    fn ties_keep_input_order_so_older_emoji_win() {
        // Fill active exactly, then two tied emoji compete for the last considered-
        // vs-archived boundary is not hit here; instead test the active boundary.
        let mut scores = vec![10i64; ACTIVE_EMOJI_SLOTS]; // 128 active
        scores.push(5); // 129th, distinct → considered
        scores.push(5); // 130th, tie → considered
        let standings = rank_emojis(&scores);
        assert!(standings[..ACTIVE_EMOJI_SLOTS].iter().all(|s| *s == Active));
        assert_eq!(standings[ACTIVE_EMOJI_SLOTS], Considered);
        assert_eq!(standings[ACTIVE_EMOJI_SLOTS + 1], Considered);
    }

    #[test]
    fn the_boundaries_land_at_128_and_256() {
        // 300 emoji, each with a unique descending score so rank == index.
        let scores: Vec<i64> = (0..300).map(|i| (300 - i) as i64).collect();
        let standings = rank_emojis(&scores);
        assert_eq!(standings[ACTIVE_EMOJI_SLOTS - 1], Active); // rank 127
        assert_eq!(standings[ACTIVE_EMOJI_SLOTS], Considered); // rank 128
        let last_considered = ACTIVE_EMOJI_SLOTS + CONSIDERED_EMOJI_SLOTS - 1; // rank 255
        assert_eq!(standings[last_considered], Considered);
        assert_eq!(standings[last_considered + 1], Archived); // rank 256
        assert_eq!(standings[299], Archived);
    }

    #[test]
    fn a_negative_score_can_still_be_active_if_the_pool_is_small() {
        // Standing is by rank, not an absolute threshold — being downvoted only
        // demotes you relative to others.
        assert_eq!(rank_emojis(&[-5, -10]), vec![Active, Active]);
    }

    #[test]
    fn an_empty_pool_ranks_to_nothing() {
        assert!(rank_emojis(&[]).is_empty());
    }
}
