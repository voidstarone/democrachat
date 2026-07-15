//! The pure predicate deciding whether one user may DM another.

use crate::{Block, DmPolicy, Friendship};

/// Whether `sender` may open/continue a DM to a `recipient` whose policy is
/// `recipient_policy`, given every [`Block`] and [`Friendship`] the two share.
///
/// The rule, in order:
/// 1. A [`Block`] in *either* direction silences the DM — permanently.
/// 2. Under [`DmPolicy::FriendsOnly`], the two must be accepted friends.
/// 3. Otherwise ([`DmPolicy::Everyone`]) the DM is allowed.
///
/// Pure: the caller supplies only the blocks and friendships that involve this
/// pair, so the domain makes no store calls.
pub fn can_dm(
    sender: crate::UserId,
    recipient: crate::UserId,
    recipient_policy: DmPolicy,
    blocks: &[Block],
    friendships: &[Friendship],
) -> bool {
    let is_blocked = blocks.iter().any(|b| b.is_between(sender, recipient));
    if is_blocked {
        return false;
    }

    match recipient_policy {
        DmPolicy::Everyone => true,
        DmPolicy::FriendsOnly => friendships
            .iter()
            .any(|f| f.are_friends() && f.is_between(sender, recipient)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Timestamp, UserId};

    const A: UserId = UserId(1);
    const B: UserId = UserId(2);
    const T: Timestamp = Timestamp(0);

    #[test]
    fn everyone_policy_allows_a_stranger() {
        assert!(can_dm(A, B, DmPolicy::Everyone, &[], &[]));
    }

    #[test]
    fn friends_only_blocks_a_stranger() {
        assert!(!can_dm(A, B, DmPolicy::FriendsOnly, &[], &[]));
    }

    #[test]
    fn friends_only_allows_an_accepted_friend() {
        let mut f = Friendship::request(A, B, T);
        f.accept();
        assert!(can_dm(A, B, DmPolicy::FriendsOnly, &[], &[f]));
    }

    #[test]
    fn friends_only_still_blocks_a_pending_request() {
        let f = Friendship::request(A, B, T);
        assert!(!can_dm(A, B, DmPolicy::FriendsOnly, &[], &[f]));
    }

    #[test]
    fn a_block_silences_even_under_everyone() {
        let block = Block::new(B, A, T);
        assert!(!can_dm(A, B, DmPolicy::Everyone, &[block], &[]));
    }

    #[test]
    fn a_block_silences_in_both_directions() {
        // A blocked B, yet B may not DM A either.
        let block = Block::new(A, B, T);
        assert!(!can_dm(B, A, DmPolicy::Everyone, &[block], &[]));
    }

    #[test]
    fn a_block_beats_even_friendship() {
        let mut f = Friendship::request(A, B, T);
        f.accept();
        let block = Block::new(A, B, T);
        assert!(!can_dm(A, B, DmPolicy::FriendsOnly, &[block], &[f]));
    }
}
