//! Layer 1 — evaluate a member against a server's franchise criteria.

use crate::{
    Eligibility, EmailFranchiseRule, FranchiseCriteria, Membership, Phase, Timestamp, Unmet, User,
};

/// Layer 1 — evaluate a member against a server's franchise criteria.
///
/// Pure: depends only on the user, their membership, the criteria, the server's
/// phase, the deployment's email rule, and `now`. This is the **only** function in
/// the system that can conclude a member is eligible for the franchise — there is
/// no bypass, which is why both the founding-cohort waiver and the operator's email
/// policy are judged here rather than at whichever call site happens to admit
/// someone.
///
/// ## The founding cohort
///
/// While a server is still in [`Phase::Seed`] its members are its founders, and a
/// founder does not wait: the time bars (account age, membership dwell) are waived,
/// so anyone who joins a brand-new server is enfranchised at once. The window shuts
/// by itself at five citizens, when the server enters
/// [`Chartering`](Phase::Chartering) and the ordinary criteria resume — so the
/// waiver can seat at most the handful of people who were actually there at the
/// start. What is *never* waived: a sanction, a franchise bar, or a contribution
/// bar the server has voted for itself.
pub fn evaluate_eligibility(
    user: &User,
    membership: &Membership,
    criteria: &FranchiseCriteria,
    phase: Phase,
    email_rule: EmailFranchiseRule,
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
    // The founding cohort is excused the wait, and only the wait.
    let founding = phase == Phase::Seed;

    let account_age = user.account_age_days(now);
    if !founding && account_age < criteria.min_account_age_days {
        unmet.push(Unmet::AccountTooYoung {
            need_days: criteria.min_account_age_days,
            have_days: account_age,
        });
    }

    let member_age = membership.membership_age_days(now);
    if !founding && member_age < criteria.min_membership_days {
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

    // Deployment policy, not a server criterion: where the operator asks for a
    // confirmed address, an unconfirmed one bars the franchise no matter how long
    // or how well the member has participated. Last in the list because it is the
    // only entry the member can clear on the spot.
    //
    // The founding cohort is trusted to confirm *afterwards* — seated now, with a
    // deadline (see `UNCONFIRMED_FRANCHISE_GRACE_DAYS`). That trust is extended
    // once: a member carrying a deadline already has had it, so once theirs lapses
    // they must confirm like anyone else. Otherwise a founder could ride out one
    // grace, lose the vote, and be handed another by the same waiver.
    if email_rule == EmailFranchiseRule::MustBeConfirmed && !user.is_email_verified() {
        let trust_available = founding && membership.unconfirmed_franchise_until.is_none();
        if !trust_available {
            unmet.push(Unmet::EmailUnverified);
        }
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
            Phase::Chartering,
            EmailFranchiseRule::Ignored,
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
            Phase::Chartering,
            EmailFranchiseRule::Ignored,
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
            Phase::Chartering,
            EmailFranchiseRule::Ignored,
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
        let e = evaluate_eligibility(&user_aged(1, now), &member_aged(1, 0, now), &strict, Phase::Chartering, EmailFranchiseRule::Ignored, now);
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
            Phase::Chartering,
            EmailFranchiseRule::Ignored,
            now,
        );
        assert_eq!(e.unmet, vec![Unmet::Sanctioned]);
    }

    fn confirmed(mut u: User) -> User {
        u.email_verified = true;
        u
    }

    /// Soft verification: the member has served the time, so the address is the
    /// one thing between them and the vote. This is the case the UI banners.
    #[test]
    fn an_unconfirmed_address_is_the_sole_bar_for_an_otherwise_qualified_member() {
        let now = Timestamp(100 * DAY);
        let e = evaluate_eligibility(
            &user_aged(40, now), // User::new leaves the address unconfirmed
            &member_aged(30, 0, now),
            &FranchiseCriteria::platform_default(),
            Phase::Chartering,
            EmailFranchiseRule::MustBeConfirmed,
            now,
        );
        assert!(!e.is_eligible());
        assert_eq!(e.unmet, vec![Unmet::EmailUnverified]);
    }

    #[test]
    fn confirming_the_address_clears_the_bar() {
        let now = Timestamp(100 * DAY);
        let e = evaluate_eligibility(
            &confirmed(user_aged(40, now)),
            &member_aged(30, 0, now),
            &FranchiseCriteria::platform_default(),
            Phase::Chartering,
            EmailFranchiseRule::MustBeConfirmed,
            now,
        );
        assert!(e.is_eligible(), "expected eligible, got {:?}", e.unmet);
    }

    /// With verification off, an unconfirmed address is simply not a franchise
    /// question — the seed/CLI accounts that never had one still qualify.
    #[test]
    fn the_address_is_ignored_when_the_deployment_does_not_ask_for_it() {
        let now = Timestamp(100 * DAY);
        let e = evaluate_eligibility(
            &user_aged(40, now),
            &member_aged(30, 0, now),
            &FranchiseCriteria::platform_default(),
            Phase::Chartering,
            EmailFranchiseRule::Ignored,
            now,
        );
        assert!(e.is_eligible(), "expected eligible, got {:?}", e.unmet);
    }

    /// The email rule adds to the other bars, it doesn't replace them — a member
    /// who is both too new and unconfirmed is told both.
    #[test]
    fn the_email_bar_stacks_with_the_earned_criteria() {
        let now = Timestamp(100 * DAY);
        let e = evaluate_eligibility(
            &user_aged(40, now),
            &member_aged(27, 0, now), // a day short of the dwell
            &FranchiseCriteria::platform_default(),
            Phase::Chartering,
            EmailFranchiseRule::MustBeConfirmed,
            now,
        );
        assert_eq!(e.unmet.len(), 2);
        assert!(e.unmet.contains(&Unmet::EmailUnverified));
    }

    /// Barring short-circuits everything, so a barred account reports only the
    /// bar — adding "confirm your email" would imply confirming would help.
    #[test]
    fn a_barred_account_is_not_also_told_to_confirm_its_email() {
        let now = Timestamp(100 * DAY);
        let e = evaluate_eligibility(
            &user_aged(40, now).barred(),
            &member_aged(30, 0, now),
            &FranchiseCriteria::platform_default(),
            Phase::Chartering,
            EmailFranchiseRule::MustBeConfirmed,
            now,
        );
        assert_eq!(e.unmet, vec![Unmet::Barred]);
    }

    /// A founding member joins a day-old server and votes today: no dwell, and the
    /// unconfirmed address is trusted for now (the deadline is applied by whoever
    /// seats them).
    #[test]
    fn a_founding_member_qualifies_on_the_day_they_join() {
        let now = Timestamp(100 * DAY);
        let e = evaluate_eligibility(
            &user_aged(0, now),
            &member_aged(0, 0, now),
            &FranchiseCriteria::platform_default(),
            Phase::Seed,
            EmailFranchiseRule::MustBeConfirmed,
            now,
        );
        assert!(e.is_eligible(), "expected eligible, got {:?}", e.unmet);
    }

    /// The waiver covers the wait, not the rest of the constitution.
    #[test]
    fn the_founding_waiver_does_not_excuse_a_sanction_or_a_contribution_bar() {
        let now = Timestamp(100 * DAY);
        let paid = FranchiseCriteria { min_account_age_days: 0, min_membership_days: 28, min_contribution: 5 };
        let e = evaluate_eligibility(
            &confirmed(user_aged(0, now)),
            &member_aged(0, 0, now), // no contribution
            &paid,
            Phase::Seed,
            EmailFranchiseRule::Ignored,
            now,
        );
        assert_eq!(e.unmet, vec![Unmet::InsufficientContribution { need: 5, have: 0 }]);

        let mut sanctioned = member_aged(0, 9, now);
        sanctioned.is_sanctioned = true;
        let e = evaluate_eligibility(
            &confirmed(user_aged(0, now)),
            &sanctioned,
            &FranchiseCriteria::platform_default(),
            Phase::Seed,
            EmailFranchiseRule::Ignored,
            now,
        );
        assert_eq!(e.unmet, vec![Unmet::Sanctioned]);
    }

    /// Trust is extended once. A member whose deadline is on record has already had
    /// their grace, so the founding waiver no longer covers their address — even
    /// while the server is still in Seed.
    #[test]
    fn a_lapsed_founding_member_cannot_be_handed_a_second_grace() {
        let now = Timestamp(100 * DAY);
        let mut spent = member_aged(30, 0, now);
        spent.unconfirmed_franchise_until = Some(Timestamp(now.0 - DAY)); // lapsed yesterday
        let e = evaluate_eligibility(
            &user_aged(40, now),
            &spent,
            &FranchiseCriteria::platform_default(),
            Phase::Seed,
            EmailFranchiseRule::MustBeConfirmed,
            now,
        );
        assert_eq!(e.unmet, vec![Unmet::EmailUnverified]);

        // Confirming is all it takes to come back.
        let e = evaluate_eligibility(
            &confirmed(user_aged(40, now)),
            &spent,
            &FranchiseCriteria::platform_default(),
            Phase::Seed,
            EmailFranchiseRule::MustBeConfirmed,
            now,
        );
        assert!(e.is_eligible(), "expected eligible, got {:?}", e.unmet);
    }

    /// Past Seed the head start is over: a newcomer to an established server waits
    /// like everyone else.
    #[test]
    fn the_waiver_is_gone_once_the_server_charters() {
        let now = Timestamp(100 * DAY);
        let e = evaluate_eligibility(
            &confirmed(user_aged(0, now)),
            &member_aged(0, 0, now),
            &FranchiseCriteria::platform_default(),
            Phase::Chartering,
            EmailFranchiseRule::MustBeConfirmed,
            now,
        );
        assert_eq!(e.unmet, vec![Unmet::MembershipTooShort { need_days: 28, have_days: 0 }]);
    }
}
