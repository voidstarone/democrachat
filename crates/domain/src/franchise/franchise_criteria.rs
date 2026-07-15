//! A server's franchise constitution.

use serde::{Deserialize, Serialize};

/// A server's franchise constitution: the bar a member must clear to become a
/// citizen. Amendable only by constitutional vote (Layer 3) — and never in the
/// Seed phase (training wheels). This is the *sole* gate to the franchise; no
/// person can waive or grant around it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct FranchiseCriteria {
    /// Minimum platform-wide account age, in days.
    pub min_account_age_days: i64,
    /// Minimum dwell time as a member of *this* server, in days.
    pub min_membership_days: i64,
    /// Minimum endorsement-weighted contribution within this server.
    pub min_contribution: i64,
}

impl FranchiseCriteria {
    /// The platform default every new server starts from: 28 days of membership in
    /// this server, and nothing else. A server's electorate can add stricter gates
    /// (account age, endorsed contribution) later by constitutional vote.
    pub fn platform_default() -> Self {
        Self {
            min_account_age_days: 0,
            min_membership_days: 28,
            min_contribution: 0,
        }
    }
}
