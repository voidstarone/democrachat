//! Verify a password against a stored Argon2 PHC hash.

use argon2::Argon2;
use password_hash::{PasswordHash, PasswordVerifier};

/// Whether `password` matches the stored `phc` hash. Any parse failure (empty or
/// malformed stored hash) returns `false` rather than panicking, so a corrupt or
/// unset hash never leaks via a crash and never authenticates.
pub fn verify_password(password: &str, phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}
