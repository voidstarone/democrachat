//! Field-level encryption for the email address stored on a `User`.
//!
//! Reuses the vault AEAD ([`seal`](crate::vault::seal)/[`open`](crate::vault::open),
//! ChaCha20-Poly1305 envelope) under a **dedicated** email key so the address is
//! ciphertext in the record even when the snapshot file itself is not sealed. The
//! result is the opaque string kept in [`User::email_enc`](domain::User::email_enc).

use crate::vault::open::open;
use crate::vault::seal::seal;
use crate::vault::sealed::Sealed;
use crate::vault::vault_error::VaultError;
use crate::vault::vault_key::VaultKey;

/// Seal a plaintext email into the opaque `email_enc` blob. Fresh nonces are drawn
/// per call, so the same address encrypts to different ciphertext each time.
pub fn seal_email(key: &VaultKey, email: &str) -> String {
    seal(key, email.as_bytes()).to_json()
}

/// Recover a plaintext email from its sealed `email_enc` blob. `Err` if the blob
/// is malformed, the key is wrong, or the plaintext is not valid UTF-8.
pub fn open_email(key: &VaultKey, blob: &str) -> Result<String, VaultError> {
    let sealed = Sealed::from_json(blob)?;
    let bytes = open(key, &sealed)?;
    String::from_utf8(bytes).map_err(|e| VaultError::Format(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> VaultKey {
        VaultKey::from_hex(&"ab".repeat(32)).unwrap()
    }

    #[test]
    fn round_trips() {
        let k = key();
        let blob = seal_email(&k, "alice@example.com");
        assert_ne!(blob, "alice@example.com", "must not be plaintext");
        assert_eq!(open_email(&k, &blob).unwrap(), "alice@example.com");
    }

    #[test]
    fn same_input_differs_each_seal() {
        let k = key();
        assert_ne!(seal_email(&k, "a@b.com"), seal_email(&k, "a@b.com"));
    }

    #[test]
    fn wrong_key_fails() {
        let blob = seal_email(&key(), "alice@example.com");
        let other = VaultKey::from_hex(&"cd".repeat(32)).unwrap();
        assert!(open_email(&other, &blob).is_err());
    }
}
