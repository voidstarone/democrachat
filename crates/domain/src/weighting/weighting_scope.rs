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
    /// The scope's canonical wire name — the explicit string form crossing the
    /// web↔domain boundary, not the `Debug` rendering (no stability contract).
    /// Matches the derived serde representation (pinned by a test).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Both => "Both",
            Self::JuriesOnly => "JuriesOnly",
            Self::BallotsOnly => "BallotsOnly",
            Self::None => "None",
        }
    }

    /// Parse a canonical [`name`](Self::name) back into a scope. Used to translate a
    /// client's governance-settings choice into the domain type.
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Both, Self::JuriesOnly, Self::BallotsOnly, Self::None]
            .into_iter()
            .find(|s| s.name() == name)
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

    #[test]
    fn name_round_trips_and_matches_serde() {
        for s in [
            WeightingScope::Both,
            WeightingScope::JuriesOnly,
            WeightingScope::BallotsOnly,
            WeightingScope::None,
        ] {
            assert_eq!(WeightingScope::from_name(s.name()), Some(s));
            assert_eq!(serde_json::to_string(&s).unwrap(), format!("\"{}\"", s.name()));
        }
        assert_eq!(WeightingScope::from_name("Nope"), None);
    }
}
