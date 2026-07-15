//! Decide a verdict from the running tally.

use crate::Verdict;

/// Decide a verdict from the running tally, in units of *vote weight*.
///
/// Convicts only when the guilty weight reaches a 2/3 supermajority of the
/// *full* jury's weight. Acquits as soon as conviction is arithmetically
/// impossible — which also covers the "everyone voted, bar unmet" case.
///
/// Under one-juror-one-vote every weight is `1`, so `*_weight` equal head counts
/// and `jury_weight` equals the jury size: the supermajority rule is unchanged.
pub fn reach_verdict(guilty_weight: u64, not_guilty_weight: u64, jury_weight: u64) -> Verdict {
    if jury_weight == 0 {
        return Verdict::NotGuilty;
    }
    let need_guilty = (jury_weight * 2).div_ceil(3); // ceil(2/3 * total weight)

    if guilty_weight >= need_guilty {
        Verdict::Guilty
    } else if jury_weight - not_guilty_weight < need_guilty {
        // Even if every undecided juror votes guilty, the bar can't be met.
        Verdict::NotGuilty
    } else {
        Verdict::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_thirds_required_to_convict() {
        // Jury of 7 -> need ceil(14/3) = 5 guilty.
        assert_eq!(reach_verdict(5, 0, 7), Verdict::Guilty);
        assert_eq!(reach_verdict(4, 0, 7), Verdict::Pending);
        assert_eq!(reach_verdict(0, 3, 7), Verdict::NotGuilty);
    }

    #[test]
    fn full_split_without_supermajority_acquits() {
        // 4 guilty / 3 not-guilty, everyone voted, bar (5) unmet -> acquit.
        assert_eq!(reach_verdict(4, 3, 7), Verdict::NotGuilty);
    }
}
