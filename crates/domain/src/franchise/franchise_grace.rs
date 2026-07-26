//! How long a founding member may vote before confirming their email address.

use crate::Timestamp;

/// How long a founding member's vote stands on trust before it must be backed by a
/// confirmed email address.
///
/// A server's first members are seated the moment they arrive — a brand-new
/// community that cannot vote for a month is not a community — and on a deployment
/// that asks for confirmed addresses they are trusted to confirm afterwards. This
/// is the length of that trust.
///
/// It is 28 days because that is the platform's default membership dwell
/// ([`FranchiseCriteria::platform_default`](crate::FranchiseCriteria::platform_default)):
/// the founding cohort is being let off exactly that wait, so their deadline falls
/// where their qualification would have. A member who confirms in time keeps the
/// vote continuously; one who lets it lapse is in the position they would have been
/// in without the head start, and can confirm to be re-admitted on the ordinary
/// path.
///
/// Fixed rather than per-server on purpose: it is the operator's identity policy,
/// not a bar the electorate votes on, and a server that could vote its own grace
/// to a decade would hollow out the requirement it is a grace *from*.
pub const UNCONFIRMED_FRANCHISE_GRACE_DAYS: i64 = 28;

/// The deadline for a member seated on trust at `seated_at`.
pub fn confirmation_deadline(seated_at: Timestamp) -> Timestamp {
    Timestamp(seated_at.0 + UNCONFIRMED_FRANCHISE_GRACE_DAYS * Timestamp::SECONDS_PER_DAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deadline_is_the_grace_after_seating() {
        let seated = Timestamp(1_000);
        let deadline = confirmation_deadline(seated);
        assert_eq!(deadline.0 - seated.0, 28 * Timestamp::SECONDS_PER_DAY);
    }

    /// The grace matches the dwell it excuses, so a lapse lands exactly where the
    /// member would have qualified anyway.
    #[test]
    fn the_grace_matches_the_default_dwell() {
        assert_eq!(
            UNCONFIRMED_FRANCHISE_GRACE_DAYS,
            crate::FranchiseCriteria::platform_default().min_membership_days
        );
    }
}
