//! A minimal, serialization-friendly time value object.
//!
//! Time is the universal filter in this design ("make takeover slow, not
//! impossible"), so it is a first-class domain concept. We model it as unix
//! seconds — trivially storable in a text file or a DB, and free of timezone or
//! wall-clock surprises in the rules.

use serde::{Deserialize, Serialize};

/// A point in time, as whole seconds since the unix epoch (UTC).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub const SECONDS_PER_DAY: i64 = 86_400;

    /// Whole days from `earlier` to `self` (floored, never negative-rounded up).
    pub fn days_since(self, earlier: Timestamp) -> i64 {
        (self.0 - earlier.0).div_euclid(Self::SECONDS_PER_DAY)
    }

    /// A timestamp `days` days after this one.
    pub fn plus_days(self, days: i64) -> Timestamp {
        Timestamp(self.0 + days * Self::SECONDS_PER_DAY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_since_floors() {
        let start = Timestamp(0);
        assert_eq!(Timestamp(Timestamp::SECONDS_PER_DAY * 3).days_since(start), 3);
        // 2 days and 23 hours is still 2 whole days.
        let almost = Timestamp::SECONDS_PER_DAY * 3 - 3600;
        assert_eq!(Timestamp(almost).days_since(start), 2);
    }

    #[test]
    fn plus_days_roundtrips() {
        let t = Timestamp(1_000_000);
        assert_eq!(t.plus_days(7).days_since(t), 7);
    }
}
