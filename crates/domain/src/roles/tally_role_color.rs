//! Pick a role's colour from its citizens' colour votes (plurality).

use std::collections::BTreeMap;

use crate::RoleColor;

/// The winning colour among a role's votes: the one cast most often, ties broken
/// by the lowest hex string so the result is deterministic and never flickers on
/// reload. `None` when no one has voted.
///
/// Pure: the caller passes only the colours cast by *currently-franchised*
/// citizens, mirroring how [`rank_emojis`](crate::rank_emojis) counts only live
/// voters.
pub fn winning_color<'a>(colors: impl IntoIterator<Item = &'a RoleColor>) -> Option<RoleColor> {
    let mut counts: BTreeMap<&RoleColor, u32> = BTreeMap::new();
    for c in colors {
        *counts.entry(c).or_default() += 1;
    }
    // Keys are unique, so the comparator fully orders the entries: greater = more
    // votes, and on a tie the lower hex string wins (reverse-compare the colour).
    counts
        .into_iter()
        .max_by(|(ac, an), (bc, bn)| an.cmp(bn).then_with(|| bc.cmp(ac)))
        .map(|(c, _)| c.clone())
}

#[cfg(test)]
mod tests {
    use super::winning_color;
    use crate::RoleColor;

    fn c(s: &str) -> RoleColor {
        RoleColor::parse(s).unwrap()
    }

    #[test]
    fn no_votes_no_colour() {
        assert_eq!(winning_color(Vec::<RoleColor>::new().iter()), None);
    }

    #[test]
    fn plurality_wins() {
        let votes = [c("#ff0000"), c("#ff0000"), c("#0000ff")];
        assert_eq!(winning_color(votes.iter()), Some(c("#ff0000")));
    }

    #[test]
    fn ties_break_to_the_lowest_hex() {
        // One each — #0000ff sorts below #ff0000, so it wins deterministically.
        let votes = [c("#ff0000"), c("#0000ff")];
        assert_eq!(winning_color(votes.iter()), Some(c("#0000ff")));
    }
}
