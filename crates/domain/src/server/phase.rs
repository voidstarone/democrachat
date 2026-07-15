//! The bootstrap phase of a server.

use serde::{Deserialize, Serialize};

/// The bootstrap phase of a server, derived purely from its citizen count.
///
/// Small servers are where capture is easiest and percentage-math is weakest, so
/// new servers run on "training wheels" until self-governance is meaningful. In
/// **Seed** the founder may provisionally set the server up (channels, emojis,
/// rules) to bootstrap it; from **Chartering** on, those changes become ballots.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Phase {
    /// 1–9 citizens. No constitutional amendments; founder may provision.
    Seed,
    /// 10–24 citizens. Amendments allowed but under stricter thresholds.
    Chartering,
    /// 25+ citizens. Full self-governance; percentage math now works naturally.
    Sovereign,
}

impl Phase {
    pub const CHARTERING_AT: u64 = 10;
    pub const SOVEREIGN_AT: u64 = 25;

    pub fn from_citizen_count(citizens: u64) -> Phase {
        if citizens >= Self::SOVEREIGN_AT {
            Phase::Sovereign
        } else if citizens >= Self::CHARTERING_AT {
            Phase::Chartering
        } else {
            Phase::Seed
        }
    }

    /// Whether the founder still holds provisional setup power (Seed only).
    pub fn founder_may_provision(self) -> bool {
        matches!(self, Phase::Seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_boundaries() {
        assert_eq!(Phase::from_citizen_count(0), Phase::Seed);
        assert_eq!(Phase::from_citizen_count(9), Phase::Seed);
        assert_eq!(Phase::from_citizen_count(10), Phase::Chartering);
        assert_eq!(Phase::from_citizen_count(24), Phase::Chartering);
        assert_eq!(Phase::from_citizen_count(25), Phase::Sovereign);
        assert_eq!(Phase::from_citizen_count(10_000), Phase::Sovereign);
    }

    #[test]
    fn only_seed_may_be_provisioned_by_the_founder() {
        assert!(Phase::Seed.founder_may_provision());
        assert!(!Phase::Chartering.founder_may_provision());
        assert!(!Phase::Sovereign.founder_may_provision());
    }
}
