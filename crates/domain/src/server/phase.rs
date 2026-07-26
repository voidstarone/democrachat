//! The bootstrap phase of a server.

use serde::{Deserialize, Serialize};

use crate::PhaseThresholds;

/// The bootstrap phase of a server, derived purely from its citizen count and the
/// deployment's [`PhaseThresholds`].
///
/// Small servers are where capture is easiest and percentage-math is weakest, so
/// new servers run on "training wheels" until self-governance is meaningful. In
/// **Seed** the founder may provisionally set the server up (channels, emojis,
/// rules) to bootstrap it, and whoever joins is enfranchised on arrival; from
/// **Chartering** on, those changes become ballots and newcomers earn the franchise
/// the ordinary way.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Phase {
    /// Below the chartering threshold (1–4 citizens by default). No constitutional
    /// amendments; founder may provision; the founding cohort votes on arrival.
    Seed,
    /// Chartering up to the sovereign threshold (5–24 by default). Amendments
    /// allowed but under stricter thresholds.
    Chartering,
    /// At or above the sovereign threshold (25+ by default). Full self-governance;
    /// percentage math now works naturally.
    Sovereign,
}

impl Phase {
    /// Which phase `citizens` puts a server in, under this deployment's thresholds.
    /// Takes them explicitly rather than reading a global so a call site cannot
    /// quietly judge a server by the platform default while the operator has
    /// configured something else.
    pub fn from_citizen_count(citizens: u64, thresholds: PhaseThresholds) -> Phase {
        if citizens >= thresholds.sovereign_at {
            Phase::Sovereign
        } else if citizens >= thresholds.chartering_at {
            Phase::Chartering
        } else {
            Phase::Seed
        }
    }

    /// Whether the founder still holds provisional setup power (Seed only).
    pub fn founder_may_provision(self) -> bool {
        matches!(self, Phase::Seed)
    }

    /// The phase's canonical wire name for the client. Explicit, not the `Debug`
    /// rendering, so the JSON the SPA reads is a deliberate contract.
    pub const fn name(self) -> &'static str {
        match self {
            Phase::Seed => "Seed",
            Phase::Chartering => "Chartering",
            Phase::Sovereign => "Sovereign",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_boundaries() {
        let d = PhaseThresholds::platform_default();
        assert_eq!(Phase::from_citizen_count(0, d), Phase::Seed);
        assert_eq!(Phase::from_citizen_count(4, d), Phase::Seed);
        assert_eq!(Phase::from_citizen_count(5, d), Phase::Chartering);
        assert_eq!(Phase::from_citizen_count(24, d), Phase::Chartering);
        assert_eq!(Phase::from_citizen_count(25, d), Phase::Sovereign);
        assert_eq!(Phase::from_citizen_count(10_000, d), Phase::Sovereign);
    }

    /// A host's thresholds move the boundaries wholesale.
    #[test]
    fn configured_thresholds_move_the_boundaries() {
        let tight = PhaseThresholds::new(2, 10).unwrap();
        assert_eq!(Phase::from_citizen_count(1, tight), Phase::Seed, "the founder alone");
        assert_eq!(Phase::from_citizen_count(2, tight), Phase::Chartering);
        assert_eq!(Phase::from_citizen_count(10, tight), Phase::Sovereign);

        // Equal thresholds skip Chartering: Seed straight to Sovereign.
        let skip = PhaseThresholds::new(8, 8).unwrap();
        assert_eq!(Phase::from_citizen_count(7, skip), Phase::Seed);
        assert_eq!(Phase::from_citizen_count(8, skip), Phase::Sovereign);
    }

    #[test]
    fn only_seed_may_be_provisioned_by_the_founder() {
        assert!(Phase::Seed.founder_may_provision());
        assert!(!Phase::Chartering.founder_may_provision());
        assert!(!Phase::Sovereign.founder_may_provision());
    }
}
