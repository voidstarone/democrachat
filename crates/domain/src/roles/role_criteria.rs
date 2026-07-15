//! The conditions a member must clear to auto-hold a custom role.

use serde::{Deserialize, Serialize};

use crate::{Membership, Timestamp, User};

/// The conditions a member must satisfy to hold a custom [`Role`](crate::Role).
///
/// A role is not *assigned* — it is **earned**: a member holds it automatically
/// the instant they meet these criteria, and loses it automatically if they fall
/// below (there is no self-serve or admin grant). This keeps role membership a
/// pure function of standing, exactly like the criteria-only franchise. Set when
/// the role is created by ballot ([`crate::ProposalKind::CreateRole`]); the empty
/// default (all zero, `requires_citizen: false`) admits every member.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct RoleCriteria {
    /// Minimum platform-wide account age, in days.
    #[serde(default)]
    pub min_account_age_days: i64,
    /// Minimum dwell time as a member of *this* server, in days.
    #[serde(default)]
    pub min_membership_days: i64,
    /// Minimum endorsement-weighted contribution within this server.
    #[serde(default)]
    pub min_contribution: i64,
    /// Whether the member must be an enfranchised citizen to hold the role.
    #[serde(default)]
    pub requires_citizen: bool,
}

impl RoleCriteria {
    /// Whether `member` (the `user`'s membership record) satisfies every condition
    /// at `now`. A franchise-barred account or a sanctioned member never qualifies:
    /// a role is a standing signal, and neither has the standing to carry one.
    pub fn admits(&self, user: &User, member: &Membership, now: Timestamp) -> bool {
        if user.is_franchise_barred || member.is_sanctioned {
            return false;
        }
        if self.requires_citizen && !member.is_franchised() {
            return false;
        }
        user.account_age_days(now) >= self.min_account_age_days
            && member.membership_age_days(now) >= self.min_membership_days
            && member.contribution >= self.min_contribution
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ServerId, Tier, UserId};

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
    fn the_empty_default_admits_any_member() {
        let now = Timestamp(100 * DAY);
        assert!(RoleCriteria::default().admits(&user_aged(0, now), &member_aged(0, 0, now), now));
    }

    #[test]
    fn a_sanctioned_or_barred_member_never_qualifies() {
        let now = Timestamp(100 * DAY);
        let mut m = member_aged(90, 50, now);
        m.is_sanctioned = true;
        assert!(!RoleCriteria::default().admits(&user_aged(90, now), &m, now));
        let barred = user_aged(90, now).barred();
        assert!(!RoleCriteria::default().admits(&barred, &member_aged(90, 50, now), now));
    }

    #[test]
    fn requires_citizen_gates_on_the_franchise() {
        let now = Timestamp(100 * DAY);
        let c = RoleCriteria { requires_citizen: true, ..Default::default() };
        let mut member = member_aged(90, 0, now);
        assert!(!c.admits(&user_aged(90, now), &member, now), "a Member is not admitted");
        member.tier = Tier::Citizen;
        assert!(c.admits(&user_aged(90, now), &member, now), "a Citizen is");
    }

    #[test]
    fn every_axis_must_be_met() {
        let now = Timestamp(100 * DAY);
        let c = RoleCriteria { min_account_age_days: 30, min_membership_days: 14, min_contribution: 5, requires_citizen: false };
        assert!(!c.admits(&user_aged(29, now), &member_aged(20, 9, now), now), "account too young");
        assert!(!c.admits(&user_aged(40, now), &member_aged(13, 9, now), now), "membership too short");
        assert!(!c.admits(&user_aged(40, now), &member_aged(20, 4, now), now), "contribution too low");
        assert!(c.admits(&user_aged(40, now), &member_aged(20, 9, now), now), "all axes met");
    }
}
