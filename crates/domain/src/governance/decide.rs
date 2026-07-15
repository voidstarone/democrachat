//! Apply a threshold to a tally.

use crate::{Decision, Tally, Threshold};

/// Apply a threshold to a tally given the size of the electorate.
///
/// All comparisons are exact integer arithmetic (widened to `u128` to avoid
/// overflow). Approval must be *strictly* exceeded, so a perfect tie fails.
pub fn decide(tally: Tally, established_citizens: u64, threshold: Threshold) -> Decision {
    let cast = tally.cast();

    // Turnout: cast / established >= quorum_bp / 10_000.
    let has_quorum = established_citizens > 0
        && (cast as u128) * 10_000 >= (threshold.quorum_bp as u128) * (established_citizens as u128);

    // Approval: aye / cast > approval_bp / 10_000 (strict).
    let has_approval =
        cast > 0 && (tally.aye as u128) * 10_000 > (threshold.approval_bp as u128) * (cast as u128);

    Decision {
        did_pass: has_quorum && has_approval,
        has_quorum,
        has_approval,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{threshold_for, DecisionClass, Phase};

    #[test]
    fn moderation_is_simple_majority() {
        let t = threshold_for(DecisionClass::Moderation, Phase::Seed).unwrap();
        // 6 aye / 5 nay of 100 citizens: >50% and >10% turnout -> pass.
        assert!(decide(Tally { aye: 6, nay: 5 }, 100, t).did_pass);
        // Tie fails (strict majority).
        assert!(!decide(Tally { aye: 5, nay: 5 }, 100, t).did_pass);
    }

    #[test]
    fn quorum_can_sink_a_lopsided_vote() {
        let t = threshold_for(DecisionClass::BanOrRecall, Phase::Sovereign).unwrap();
        // 100% aye but only 2% turnout of 100 citizens -> fails quorum (need 30%).
        let d = decide(Tally { aye: 2, nay: 0 }, 100, t);
        assert!(!d.did_pass);
        assert!(d.has_approval);
        assert!(!d.has_quorum);
    }

    #[test]
    fn two_thirds_required_for_sovereign_amendment() {
        let t = threshold_for(DecisionClass::Constitutional, Phase::Sovereign).unwrap();
        // 66 / 34 of 100 — just under 2/3 — fails.
        assert!(!decide(Tally { aye: 66, nay: 34 }, 100, t).did_pass);
        // 67 / 33 — over 2/3 with full turnout — passes.
        assert!(decide(Tally { aye: 67, nay: 33 }, 100, t).did_pass);
    }
}
