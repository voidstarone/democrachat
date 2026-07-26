//! Whether a confirmed email address is a condition of the franchise.

/// Whether a member must have confirmed their email address before they can hold
/// the franchise.
///
/// This is *deployment* policy — an operator's choice about how much identity a
/// vote costs — and deliberately **not** part of [`FranchiseCriteria`](crate::FranchiseCriteria),
/// which is the per-server bar that members themselves vote on. A server cannot
/// vote its way out of the operator's email requirement, and the operator cannot
/// use it to set a server's age or contribution bar. It arrives as an argument to
/// [`evaluate_eligibility`](crate::evaluate_eligibility) so the two stay separate
/// while still meeting at the one function allowed to conclude "eligible".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EmailFranchiseRule {
    /// Email confirmation has no bearing on the franchise (verification off).
    #[default]
    Ignored,
    /// An unconfirmed address keeps the member off the roll — except that a
    /// server's founding cohort may be trusted to confirm afterwards.
    MustBeConfirmed {
        /// How many days a founding member's vote stands before it must be backed
        /// by a confirmed address. The length lives *here*, alongside the rule that
        /// grants the trust, so the waiver and the deadline can never be configured
        /// out of step with each other.
        ///
        /// `0` means no trust at all: founding members are still excused the wait,
        /// but they hold the vote only once confirmed. That is the strict setting
        /// for an operator who wants the founding head start without the loophole
        /// of instantly-seated unconfirmed accounts.
        founding_grace_days: i64,
    },
}

impl EmailFranchiseRule {
    /// The trust window this rule extends to a founding member, in days. Zero when
    /// the rule extends none — either because addresses are ignored entirely or
    /// because the operator set the grace to nothing.
    pub fn founding_grace_days(self) -> i64 {
        match self {
            Self::Ignored => 0,
            Self::MustBeConfirmed { founding_grace_days } => founding_grace_days.max(0),
        }
    }

    /// Whether an unconfirmed founding member may be seated on trust at all.
    pub fn extends_founding_trust(self) -> bool {
        matches!(self, Self::MustBeConfirmed { .. }) && self.founding_grace_days() > 0
    }

    /// Whether a confirmed address is required to hold the franchise.
    pub fn requires_confirmation(self) -> bool {
        matches!(self, Self::MustBeConfirmed { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignoring_addresses_extends_no_trust_because_none_is_needed() {
        let r = EmailFranchiseRule::Ignored;
        assert!(!r.requires_confirmation());
        assert!(!r.extends_founding_trust());
        assert_eq!(r.founding_grace_days(), 0);
    }

    #[test]
    fn a_grace_of_zero_still_requires_confirmation_but_trusts_nobody() {
        let r = EmailFranchiseRule::MustBeConfirmed { founding_grace_days: 0 };
        assert!(r.requires_confirmation());
        assert!(!r.extends_founding_trust(), "the strict setting: confirm first, then vote");
    }

    #[test]
    fn a_configured_grace_is_reported_as_given() {
        let r = EmailFranchiseRule::MustBeConfirmed { founding_grace_days: 7 };
        assert!(r.extends_founding_trust());
        assert_eq!(r.founding_grace_days(), 7);
    }

    /// A negative value can only come from a misread config; treat it as no trust
    /// rather than as a deadline in the past.
    #[test]
    fn a_negative_grace_is_clamped_to_none() {
        let r = EmailFranchiseRule::MustBeConfirmed { founding_grace_days: -3 };
        assert_eq!(r.founding_grace_days(), 0);
        assert!(!r.extends_founding_trust());
    }
}
