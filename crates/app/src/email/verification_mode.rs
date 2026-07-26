//! How strictly email verification is enforced for a deployment.

/// The email-verification policy, parsed from `DEMOCRACHAT_EMAIL_VERIFICATION`.
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
    /// Accounts cannot authenticate until the emailed verification link is clicked.
    Hard,
}

impl EmailVerificationMode {
    /// Parse the env value. `None` for an unrecognized token so the caller can
    /// fail-closed on a typo rather than silently pick a mode.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" => Some(Self::Off),
            "hard" | "on" | "required" => Some(Self::Hard),
            _ => None,
        }
    }

    /// Whether an account must have a verified email before it can log in.
    pub fn requires_verification(&self) -> bool {
        matches!(self, Self::Hard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_accepted_spellings() {
        assert_eq!(EmailVerificationMode::parse("off"), Some(EmailVerificationMode::Off));
        assert_eq!(EmailVerificationMode::parse(" HARD "), Some(EmailVerificationMode::Hard));
        assert_eq!(EmailVerificationMode::parse("nonsense"), None);
    }

    #[test]
    fn default_is_off() {
        assert_eq!(EmailVerificationMode::default(), EmailVerificationMode::Off);
        assert!(!EmailVerificationMode::Off.requires_verification());
        assert!(EmailVerificationMode::Hard.requires_verification());
    }
}
