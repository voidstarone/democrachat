//! Where a server's bootstrap phases begin.

use serde::{Deserialize, Serialize};

/// The citizen counts at which a server graduates from [`Seed`](crate::Phase::Seed)
/// to [`Chartering`](crate::Phase::Chartering) and on to
/// [`Sovereign`](crate::Phase::Sovereign).
///
/// These are the platform's training wheels, and an operator's to set: how big a
/// community has to be before percentage-math means anything and constitutional
/// amendments are safe to unlock. They are deployment-wide, not per-server — a
/// server voting its own way out of its training wheels is precisely what they
/// exist to prevent.
///
/// Moving `chartering_at` moves two things at once, which is worth knowing before
/// you touch it: the size of a server's founding cohort (Seed members are
/// enfranchised on arrival), and when constitutional amendments become possible at
/// all. Raising it means a longer, larger founding window; lowering it means
/// amendments unlock sooner, in a smaller electorate.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PhaseThresholds {
    /// Citizens needed to leave Seed. Below this a server is still founding: its
    /// members are enfranchised on arrival and cannot amend the constitution.
    pub chartering_at: u64,
    /// Citizens needed to reach Sovereign — full self-governance.
    pub sovereign_at: u64,
}

impl PhaseThresholds {
    pub const DEFAULT_CHARTERING_AT: u64 = 5;
    pub const DEFAULT_SOVEREIGN_AT: u64 = 25;

    /// The platform defaults: Seed 1–4, Chartering 5–24, Sovereign 25+.
    pub fn platform_default() -> Self {
        Self {
            chartering_at: Self::DEFAULT_CHARTERING_AT,
            sovereign_at: Self::DEFAULT_SOVEREIGN_AT,
        }
    }

    /// Validate a pair of thresholds. `chartering_at` must be at least 2 — at 1 a
    /// server would be Chartering the moment it was founded, giving its lone founder
    /// an electorate of one that can amend the constitution — and `sovereign_at`
    /// must not undercut it, or Chartering would be a phase no server is ever in.
    pub fn new(chartering_at: u64, sovereign_at: u64) -> Result<Self, String> {
        if chartering_at < 2 {
            return Err(format!(
                "chartering threshold must be at least 2 citizens (got {chartering_at}): \
                 at 1 a lone founder could amend the constitution unopposed"
            ));
        }
        if sovereign_at < chartering_at {
            return Err(format!(
                "sovereign threshold ({sovereign_at}) must be at least the chartering \
                 threshold ({chartering_at})"
            ));
        }
        Ok(Self { chartering_at, sovereign_at })
    }
}

impl Default for PhaseThresholds {
    fn default() -> Self {
        Self::platform_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_platform_setting() {
        let t = PhaseThresholds::default();
        assert_eq!(t.chartering_at, 5);
        assert_eq!(t.sovereign_at, 25);
    }

    #[test]
    fn a_host_may_widen_or_narrow_the_windows() {
        let t = PhaseThresholds::new(3, 100).unwrap();
        assert_eq!(t.chartering_at, 3);
        assert_eq!(t.sovereign_at, 100);
        // Equal thresholds are allowed: a deployment that skips Chartering entirely.
        assert!(PhaseThresholds::new(10, 10).is_ok());
    }

    #[test]
    fn a_lone_founder_may_not_be_a_chartered_electorate() {
        assert!(PhaseThresholds::new(1, 25).is_err());
        assert!(PhaseThresholds::new(0, 25).is_err());
    }

    #[test]
    fn sovereign_may_not_undercut_chartering() {
        assert!(PhaseThresholds::new(10, 9).is_err());
    }
}
