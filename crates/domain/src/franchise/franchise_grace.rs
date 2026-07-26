//! How long a founding member may vote before confirming their email address.

use crate::Timestamp;

/// The default length of a founding member's vote-on-trust, in days.
///
/// A server's first members are seated the moment they arrive — a brand-new
/// community that cannot vote for a month is not a community — and on a deployment
/// that asks for confirmed addresses they are trusted to confirm afterwards. This is
/// the default length of that trust.
///
/// It defaults to 28 days because that is the platform's default membership dwell
/// ([`FranchiseCriteria::platform_default`](crate::FranchiseCriteria::platform_default)):
/// the founding cohort is being let off exactly that wait, so their deadline falls
/// where their qualification would have. A member who confirms in time keeps the
/// vote continuously; one who lets it lapse is in the position they would have been
/// in without the head start, and can confirm to be re-admitted on the ordinary
/// path.
///
/// An operator may set their own length (or `0` for no trust at all — confirm
/// first, then vote). It is deployment-wide rather than per-server on purpose: it is
/// the operator's identity policy, not a bar the electorate votes on, and a server
/// that could vote its own grace to a decade would hollow out the requirement it is
/// a grace *from*.
pub const DEFAULT_UNCONFIRMED_FRANCHISE_GRACE_DAYS: i64 = 28;

/// The deadline for a member seated on trust at `seated_at`, given a grace of
/// `grace_days`.
pub fn confirmation_deadline(seated_at: Timestamp, grace_days: i64) -> Timestamp {
    Timestamp(seated_at.0 + grace_days.max(0) * Timestamp::SECONDS_PER_DAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deadline_is_the_grace_after_seating() {
        let seated = Timestamp(1_000);
        assert_eq!(
            confirmation_deadline(seated, DEFAULT_UNCONFIRMED_FRANCHISE_GRACE_DAYS).0 - seated.0,
            28 * Timestamp::SECONDS_PER_DAY
        );
        assert_eq!(confirmation_deadline(seated, 7).0 - seated.0, 7 * Timestamp::SECONDS_PER_DAY);
    }

    /// The default grace matches the dwell it excuses, so a lapse lands exactly
    /// where the member would have qualified anyway.
    #[test]
    fn the_default_grace_matches_the_default_dwell() {
        assert_eq!(
            DEFAULT_UNCONFIRMED_FRANCHISE_GRACE_DAYS,
            crate::FranchiseCriteria::platform_default().min_membership_days
        );
    }

    /// A deadline is never in the past: a nonsense grace collapses to "right now",
    /// which the caller's own `extends_founding_trust` check keeps unreachable.
    #[test]
    fn a_negative_grace_yields_no_window() {
        let seated = Timestamp(1_000);
        assert_eq!(confirmation_deadline(seated, -5), seated);
        assert_eq!(confirmation_deadline(seated, 0), seated);
    }
}
