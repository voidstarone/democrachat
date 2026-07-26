//! How strictly email verification is enforced for a deployment.

use domain::EmailFranchiseRule;

/// The email-verification policy, parsed from `DEMOCRACHAT_EMAIL_VERIFICATION`.
///
/// The three modes differ in what an unconfirmed address costs you:
///
/// | mode | sign up | log in | hold the franchise |
/// |------|---------|--------|--------------------|
/// | [`Off`](Self::Off)   | verified at once | yes | yes |
/// | [`Soft`](Self::Soft) | link emailed     | yes | **only once confirmed** |
/// | [`Hard`](Self::Hard) | link emailed     | **only once confirmed** | yes (implied) |
///
/// Soft is the middle setting: the account works normally — read, post, DM, join
/// servers — but the vote, the one thing worth farming accounts for, stays out of
/// reach until the address is confirmed. That keeps the sign-up funnel open while
/// still pricing the franchise in confirmed identity.
///
/// The [`Default`] is [`Off`](Self::Off) so the CLI and test fixtures behave
/// exactly as before (accounts are immediately usable). The composition root
/// resolves the real deployment mode — whose env *default* is
/// [`Hard`](Self::Hard) — and injects it via
/// [`Services::with_email_policy`](crate::Services::with_email_policy).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EmailVerificationMode {
    /// No verification: accounts are created already-verified and usable at once.
    #[default]
    Off,
    /// Accounts work normally from the start, but cannot hold the franchise until
    /// the emailed verification link is clicked.
    Soft,
    /// Accounts cannot authenticate until the emailed verification link is clicked.
    Hard,
}

impl EmailVerificationMode {
    /// Parse the env value. `None` for an unrecognized token so the caller can
    /// fail-closed on a typo rather than silently pick a mode.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" => Some(Self::Off),
            "soft" | "lenient" | "vote-only" | "vote_only" => Some(Self::Soft),
            "hard" | "on" | "required" => Some(Self::Hard),
            _ => None,
        }
    }

    /// Whether an account must have a confirmed email before it can log in. Hard
    /// only — soft mode's whole point is that it lets you in.
    pub fn requires_verification(&self) -> bool {
        matches!(self, Self::Hard)
    }

    /// Whether sign-up creates the account unconfirmed and issues a link to email.
    /// True for both enforcing modes: soft still has to send the mail, or there
    /// would be nothing to click.
    pub fn issues_verification(&self) -> bool {
        matches!(self, Self::Soft | Self::Hard)
    }

    /// How [`evaluate_eligibility`](domain::evaluate_eligibility) should treat an
    /// unconfirmed address, given the operator's founding grace. Soft enforces it at
    /// the ballot box; hard enforces it at the door, so by the time anyone is
    /// evaluated they are confirmed anyway — asking in both keeps the rule true
    /// rather than merely unreachable.
    pub fn franchise_rule(&self, founding_grace_days: i64) -> EmailFranchiseRule {
        match self {
            Self::Off => EmailFranchiseRule::Ignored,
            Self::Soft | Self::Hard => EmailFranchiseRule::MustBeConfirmed { founding_grace_days },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_accepted_spellings() {
        assert_eq!(EmailVerificationMode::parse("off"), Some(EmailVerificationMode::Off));
        assert_eq!(EmailVerificationMode::parse(" HARD "), Some(EmailVerificationMode::Hard));
        assert_eq!(EmailVerificationMode::parse("soft"), Some(EmailVerificationMode::Soft));
        assert_eq!(EmailVerificationMode::parse("Vote-Only"), Some(EmailVerificationMode::Soft));
        assert_eq!(EmailVerificationMode::parse("nonsense"), None);
    }

    #[test]
    fn default_is_off() {
        assert_eq!(EmailVerificationMode::default(), EmailVerificationMode::Off);
        assert!(!EmailVerificationMode::Off.requires_verification());
        assert!(EmailVerificationMode::Hard.requires_verification());
    }

    /// Soft lets you in the door but not into the polling booth.
    #[test]
    fn soft_gates_the_franchise_without_gating_login() {
        let soft = EmailVerificationMode::Soft;
        assert!(!soft.requires_verification(), "soft must not block sign-in");
        assert!(soft.issues_verification(), "soft still emails a link");
        assert_eq!(
            soft.franchise_rule(28),
            EmailFranchiseRule::MustBeConfirmed { founding_grace_days: 28 }
        );
    }

    #[test]
    fn off_asks_for_nothing_and_hard_asks_at_the_door() {
        assert!(!EmailVerificationMode::Off.issues_verification());
        assert_eq!(EmailVerificationMode::Off.franchise_rule(28), EmailFranchiseRule::Ignored);
        assert!(EmailVerificationMode::Hard.issues_verification());
        assert_eq!(
            EmailVerificationMode::Hard.franchise_rule(28),
            EmailFranchiseRule::MustBeConfirmed { founding_grace_days: 28 }
        );
    }
}
