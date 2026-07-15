//! Layer 1 — evaluate a member against a server's franchise criteria.

use crate::{Eligibility, FranchiseCriteria, Membership, Timestamp, Unmet, User};

/// Layer 1 — evaluate a member against a server's franchise criteria.
///
/// Pure: depends only on the user, their membership, the criteria, and `now`.
/// This is the **only** function in the system that can conclude a member is
/// eligible for the franchise — there is no bypass.
pub fn evaluate_eligibility(
    user: &User,
    membership: &Membership,
    criteria: &FranchiseCriteria,
    now: Timestamp,
) -> Eligibility {
    // A franchise-barred account (a dev/content puppet) is never eligible, full
    // stop — no criterion, contribution, or age can lift the bar. Returned as the
    // sole unmet reason so callers/UI can say why.
    if user.is_franchise_barred {
        return Eligibility {
            unmet: vec![Unmet::Barred],
        };
    }

    let mut unmet = Vec::new();

    let account_age = user.account_age_days(now);
    if account_age < criteria.min_account_age_days {
        unmet.push(Unmet::AccountTooYoung {
            need_days: criteria.min_account_age_days,
            have_days: account_age,
        });
    }

    let member_age = membership.membership_age_days(now);
    if member_age < criteria.min_membership_days {
        unmet.push(Unmet::MembershipTooShort {
            need_days: criteria.min_membership_days,
            have_days: member_age,
        });
    }

    if membership.contribution < criteria.min_contribution {
        unmet.push(Unmet::InsufficientContribution {
            need: criteria.min_contribution,
            have: membership.contribution,
        });
    }

    if membership.is_sanctioned {
        unmet.push(Unmet::Sanctioned);
    }

    Eligibility { unmet }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ServerId, UserId};

    const DAY: i64 = Timestamp::SECONDS_PER_DAY;

    fn user_aged(days: i64, now: Timestamp) -> User {
        User::new(UserId(1), "alice", Timestamp(now.0 - days * DAY))
    }

    fn member_aged(days: i64, contribution: i64, now: Timestamp) -> Membership {
        let mut m = Membership::joined(UserId(1), ServerId(1), Timestamp(now.0 - days * DAY));
        m.contribution = contribution;
        m
    }

    #[test]
    fn fully_qualified_member_is_eligible() {
        let now = Timestamp(100 * DAY);
        // The default gate is 28 days of membership and nothing else.
        let e = evaluate_eligibility(
            &user_aged(40, now),
            &member_aged(30, 0, now),
            &FranchiseCriteria::platform_default(),
            now,
        );
        assert!(e.is_eligible(), "expected eligible, got {:?}", e.unmet);
    }

    #[test]
    fn membership_shorter_than_the_default_dwell_is_ineligible() {
        let now = Timestamp(100 * DAY);
        let e = evaluate_eligibility(
            &user_aged(40, now),
            &member_aged(27, 0, now), // one day short of the 28-day default
            &FranchiseCriteria::platform_default(),
            now,
        );
        assert!(!e.is_eligible());
        assert_eq!(e.unmet.len(), 1, "only the membership axis is unmet by default");
    }

    #[test]
    fn franchise_barred_account_is_never_eligible() {
        let now = Timestamp(100 * DAY);
        // Otherwise fully qualified — the bar must override every satisfied criterion.
        let user = user_aged(40, now).barred();
        let e = evaluate_eligibility(
            &user,
            &member_aged(20, 9, now),
            &FranchiseCriteria::platform_default(),
            now,
        );
        assert!(!e.is_eligible());
        assert_eq!(e.unmet, vec![Unmet::Barred]);
    }

    #[test]
    fn fresh_flood_account_is_blocked_on_every_axis() {
        let now = Timestamp(100 * DAY);
        // A server that has voted in the stricter three-axis constitution.
        let strict = FranchiseCriteria { min_account_age_days: 30, min_membership_days: 14, min_contribution: 5 };
        let e = evaluate_eligibility(&user_aged(1, now), &member_aged(1, 0, now), &strict, now);
        assert!(!e.is_eligible());
        assert_eq!(e.unmet.len(), 3); // young account, short membership, no contribution
    }

    #[test]
    fn sanction_alone_disqualifies() {
        let now = Timestamp(100 * DAY);
        let mut m = member_aged(30, 0, now); // otherwise clears the default 28-day dwell
        m.is_sanctioned = true;
        let e = evaluate_eligibility(
            &user_aged(40, now),
            &m,
            &FranchiseCriteria::platform_default(),
            now,
        );
        assert_eq!(e.unmet, vec![Unmet::Sanctioned]);
    }
}
