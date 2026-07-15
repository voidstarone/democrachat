//! What a server uses to value a citizen's vote.

use serde::{Deserialize, Serialize};

use crate::{Membership, Timestamp, MAX_VOTE_WEIGHT};

/// What a server uses to value a citizen's vote. Filed under the constitution —
/// changed via [`crate::ProposalKind::SetVoteWeighting`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum VoteWeighting {
    /// One citizen, one vote. The platform default.
    #[default]
    Equal,
    /// Weight grows with recorded contribution, with diminishing returns
    /// (`1 + ⌊√contribution⌋`) so a large score cannot run away.
    ByContribution,
    /// Weight grows with time served in the franchise — one step per full year
    /// since enfranchisement. Resists fresh-account gaming.
    ByTenure,
    /// Weight is the per-member value the server has granted
    /// ([`crate::ProposalKind::GrantVoteWeight`]); ungranted citizens weigh `1`.
    ByRole,
}

impl VoteWeighting {
    /// The scheme's canonical wire name — the explicit string form crossing the
    /// web↔domain boundary, not the `Debug` rendering (which is no stability
    /// contract). Matches the derived serde representation (pinned by a test).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Equal => "Equal",
            Self::ByContribution => "ByContribution",
            Self::ByTenure => "ByTenure",
            Self::ByRole => "ByRole",
        }
    }

    /// Parse a canonical [`name`](Self::name) back into a scheme. Used to translate
    /// a client's governance-settings choice into the domain type.
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Equal, Self::ByContribution, Self::ByTenure, Self::ByRole]
            .into_iter()
            .find(|w| w.name() == name)
    }

    /// This member's voting weight under the scheme — always within
    /// `1..=MAX_VOTE_WEIGHT`.
    pub fn weight_of(&self, member: &Membership, now: Timestamp) -> u64 {
        let raw = match self {
            VoteWeighting::Equal => 1,
            VoteWeighting::ByContribution => 1 + (member.contribution.max(0) as u64).isqrt(),
            VoteWeighting::ByTenure => match member.enfranchised_at {
                Some(at) => 1 + (now.days_since(at).max(0) as u64) / 365,
                None => 1,
            },
            VoteWeighting::ByRole => member.granted_weight as u64,
        };
        raw.clamp(1, MAX_VOTE_WEIGHT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ServerId, Tier, UserId};

    const DAY: i64 = Timestamp::SECONDS_PER_DAY;

    fn citizen(contribution: i64, enfranchised_days_ago: Option<i64>, granted: u32) -> Membership {
        let now = Timestamp(10_000 * DAY);
        let mut m = Membership::joined(UserId(1), ServerId(1), Timestamp(0));
        m.tier = Tier::Citizen;
        m.contribution = contribution;
        m.enfranchised_at = enfranchised_days_ago.map(|d| Timestamp(now.0 - d * DAY));
        m.granted_weight = granted;
        m
    }

    #[test]
    fn equal_is_always_one() {
        let now = Timestamp(10_000 * DAY);
        assert_eq!(VoteWeighting::Equal.weight_of(&citizen(9_999, Some(3650), 9), now), 1);
    }

    #[test]
    fn contribution_weight_has_diminishing_returns_and_a_floor() {
        let now = Timestamp(10_000 * DAY);
        // √100 = 10 -> weight 11.
        assert_eq!(VoteWeighting::ByContribution.weight_of(&citizen(100, None, 1), now), 11);
        // Negative/zero contribution never drops below 1.
        assert_eq!(VoteWeighting::ByContribution.weight_of(&citizen(-5, None, 1), now), 1);
    }

    #[test]
    fn tenure_weight_steps_once_per_year() {
        let now = Timestamp(10_000 * DAY);
        // ~2.5 years served -> 1 + 2 = 3.
        assert_eq!(VoteWeighting::ByTenure.weight_of(&citizen(0, Some(900), 1), now), 3);
        assert_eq!(VoteWeighting::ByTenure.weight_of(&citizen(0, None, 1), now), 1);
    }

    #[test]
    fn weights_are_capped() {
        let now = Timestamp(10_000 * DAY);
        assert_eq!(VoteWeighting::ByRole.weight_of(&citizen(0, None, 1_000), now), MAX_VOTE_WEIGHT);
        assert_eq!(VoteWeighting::ByRole.weight_of(&citizen(0, None, 0), now), 1);
    }

    /// `name`/`from_name` round-trip, and the name matches the serde form so the
    /// stored and client representations can't diverge.
    #[test]
    fn name_round_trips_and_matches_serde() {
        for w in [
            VoteWeighting::Equal,
            VoteWeighting::ByContribution,
            VoteWeighting::ByTenure,
            VoteWeighting::ByRole,
        ] {
            assert_eq!(VoteWeighting::from_name(w.name()), Some(w));
            assert_eq!(serde_json::to_string(&w).unwrap(), format!("\"{}\"", w.name()));
        }
        assert_eq!(VoteWeighting::from_name("Nope"), None);
    }
}
