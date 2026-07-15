//! Layer 3 — the threshold for a decision class in a given phase.

use crate::{DecisionClass, Phase, Threshold};

/// Layer 3 — the threshold for a decision class in a given phase.
///
/// Returns `None` when the decision is not permitted in that phase — notably,
/// constitutional amendments are disabled during the Seed phase (training
/// wheels), and Chartering uses a stricter bar than Sovereign.
pub fn threshold_for(class: DecisionClass, phase: Phase) -> Option<Threshold> {
    match class {
        DecisionClass::Moderation => Some(Threshold {
            approval_bp: 5_000, // simple majority (strictly > 50%)
            quorum_bp: 1_000,   // 10% turnout
        }),
        DecisionClass::RuleChange => Some(Threshold {
            approval_bp: 6_000, // 60% — a real consensus, but not constitutional
            quorum_bp: 3_000,   // 30% turnout; permitted in all phases
        }),
        DecisionClass::BanOrRecall => Some(Threshold {
            approval_bp: 6_000, // 60%
            quorum_bp: 3_000,   // 30% turnout
        }),
        DecisionClass::Constitutional => match phase {
            // Training wheels: no amendments while a server is tiny.
            Phase::Seed => None,
            // Chartering is deliberately stiffer than steady state.
            Phase::Chartering => Some(Threshold {
                approval_bp: 7_500, // 75%
                quorum_bp: 6_000,   // 60% turnout
            }),
            Phase::Sovereign => Some(Threshold {
                approval_bp: 6_666, // ~2/3
                quorum_bp: 5_000,   // 50% turnout
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constitutional_amendments_are_forbidden_in_seed() {
        assert!(threshold_for(DecisionClass::Constitutional, Phase::Seed).is_none());
    }

    #[test]
    fn rule_changes_are_allowed_even_in_seed() {
        assert!(threshold_for(DecisionClass::RuleChange, Phase::Seed).is_some());
    }
}
