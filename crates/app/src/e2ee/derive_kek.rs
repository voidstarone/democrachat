//! Derive a key-encryption key from a password (Argon2id), for wrapping the device
//! secret.

use argon2::Argon2;

use crate::e2ee::e2ee_error::E2eeError;

/// Stretch `password` with `salt` into a 32-byte KEK via Argon2id — the same
/// password-hardening the login path uses, but producing raw key bytes rather than
/// a verifier. Deriving the wrap key from the password is what lets a user recover
/// their E2EE identity on a new device by re-entering it (§5c).
pub(crate) fn derive_kek(password: &str, salt: &[u8]) -> Result<[u8; 32], E2eeError> {
    let mut kek = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut kek)
        .map_err(|e| E2eeError::BadKey(e.to_string()))?;
    Ok(kek)
}
