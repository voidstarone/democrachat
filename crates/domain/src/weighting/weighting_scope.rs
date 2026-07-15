//! Which collective decisions a server applies its vote weighting to.

use serde::{Deserialize, Serialize};

/// Which collective decisions a server applies its [`crate::VoteWeighting`] to.
/// Changed via [`crate::ProposalKind::SetWeightingScope`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum WeightingScope {
    /// Both jury verdicts and governance ballots are weighted.
    #[default]
    Both,
    /// Only jury verdicts are weighted; ballots stay one-citizen-one-vote.
    JuriesOnly,
    /// Only governance ballots are weighted; juries stay one-juror-one-vote.
    BallotsOnly,
    /// Weighting is ignored — one-citizen-one-vote everywhere.
    None,
}

impl WeightingScope {
    /// Parse the variant name (as rendered by `{:?}`) back into a scope. Used to
    /// translate a client's governance-settings choice into the domain type.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Both" => Some(Self::Both),
            "JuriesOnly" => Some(Self::JuriesOnly),
            "BallotsOnly" => Some(Self::BallotsOnly),
            "None" => Some(Self::None),
            _ => None,
        }
    }

    pub fn applies_to_juries(&self) -> bool {
        matches!(self, WeightingScope::Both | WeightingScope::JuriesOnly)
    }

    pub fn applies_to_ballots(&self) -> bool {
        matches!(self, WeightingScope::Both | WeightingScope::BallotsOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_predicates() {
        assert!(WeightingScope::Both.applies_to_juries());
        assert!(WeightingScope::Both.applies_to_ballots());
        assert!(WeightingScope::JuriesOnly.applies_to_juries());
        assert!(!WeightingScope::JuriesOnly.applies_to_ballots());
        assert!(!WeightingScope::None.applies_to_juries());
        assert!(!WeightingScope::None.applies_to_ballots());
    }
}
