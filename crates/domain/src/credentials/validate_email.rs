//! Pure email-policy check — a conservative `local@domain` syntactic validation.
//!
//! There is no way to prove an address exists or is deliverable without sending
//! to it, so this only rejects clearly-invalid input; the verification email is
//! the real proof of ownership.

use crate::{EmailError, MAX_EMAIL_LEN};

/// Validate an email against a conservative syntactic policy. The input is
/// trimmed first (unlike passwords, surrounding whitespace in an email is never
/// meaningful). Returns the error describing the first failed rule.
pub fn validate_email(email: &str) -> Result<(), EmailError> {
    let email = email.trim();
    if email.is_empty() {
        return Err(EmailError::Empty);
    }
    if email.len() > MAX_EMAIL_LEN {
        return Err(EmailError::TooLong);
    }
    if email.chars().any(|c| c.is_whitespace()) {
        return Err(EmailError::Malformed);
    }
    // Exactly one '@', splitting into a non-empty local and domain part.
    let (local, domain) = match email.split_once('@') {
        Some(parts) => parts,
        None => return Err(EmailError::Malformed),
    };
    if local.is_empty() || domain.contains('@') {
        return Err(EmailError::Malformed);
    }
    // Domain must have a dot with non-empty labels on both sides (rejects
    // "a@b", "a@.com", "a@b.").
    match domain.rsplit_once('.') {
        Some((host, tld)) if !host.is_empty() && !tld.is_empty() => Ok(()),
        _ => Err(EmailError::Malformed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_normal_address() {
        assert!(validate_email("alice@example.com").is_ok());
        assert!(validate_email("a.b+tag@sub.example.co.uk").is_ok());
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert!(validate_email("  alice@example.com  ").is_ok());
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(validate_email("   "), Err(EmailError::Empty));
    }

    #[test]
    fn rejects_malformed() {
        for bad in ["plainaddress", "a@b", "a@.com", "a@b.", "@example.com", "a b@example.com", "a@b@c.com"] {
            assert_eq!(validate_email(bad), Err(EmailError::Malformed), "should reject {bad:?}");
        }
    }

    #[test]
    fn rejects_too_long() {
        let long = format!("{}@example.com", "a".repeat(MAX_EMAIL_LEN));
        assert_eq!(validate_email(&long), Err(EmailError::TooLong));
    }
}
