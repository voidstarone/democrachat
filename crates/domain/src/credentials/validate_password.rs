//! Pure password-policy check (length only; never trims).

use crate::{PasswordError, MAX_PASSWORD_LEN, MIN_PASSWORD_LEN};

/// Validate a password against the length policy. The password is **never
/// trimmed** — leading/trailing spaces are legitimate secret material. Length is
/// counted in bytes, which is also what the Argon2-DoS cap
/// ([`MAX_PASSWORD_LEN`]) is about.
pub fn validate_password(password: &str) -> Result<(), PasswordError> {
    let len = password.len();
    if len < MIN_PASSWORD_LEN {
        return Err(PasswordError::TooShort);
    }
    if len > MAX_PASSWORD_LEN {
        return Err(PasswordError::TooLong);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short() {
        assert_eq!(validate_password("short"), Err(PasswordError::TooShort));
    }

    #[test]
    fn accepts_a_sixteen_char_password() {
        assert!(validate_password("correct horse!!!").is_ok());
    }

    #[test]
    fn rejects_over_max() {
        let long = "a".repeat(MAX_PASSWORD_LEN + 1);
        assert_eq!(validate_password(&long), Err(PasswordError::TooLong));
    }

    #[test]
    fn does_not_trim() {
        // 12 visible chars + 4 trailing spaces = 16 bytes → valid, spaces kept.
        assert!(validate_password("abcdefghijkl    ").is_ok());
    }
}
