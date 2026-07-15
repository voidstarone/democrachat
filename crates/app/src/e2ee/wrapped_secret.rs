//! A user's device secret key, wrapped under their password — the only form of it
//! the server ever holds.

use serde::{Deserialize, Serialize};

/// The user's device secret key encrypted under a key derived from their password
/// (Argon2id → KEK). The server stores this blob verbatim and hands it back to the
/// user at login; the user unwraps it **client-side** with their password, so the
/// server is blind to the secret. Recovering it on a new device is just re-entering
/// the password. All fields are hex.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct WrappedSecret {
    /// Argon2id salt (per-user, random).
    pub salt: String,
    /// AEAD nonce for the wrap.
    pub nonce: String,
    /// The 32-byte device secret, encrypted under the password-derived key.
    pub ciphertext: String,
}

// The crypto form ([`WrappedSecret`]) and the storage form ([`domain::WrappedKey`])
// are byte-for-byte the same blob; these conversions let the client wrap here and
// hand the result straight to the key-directory service (and back).
impl From<WrappedSecret> for domain::WrappedKey {
    fn from(w: WrappedSecret) -> Self {
        domain::WrappedKey { salt: w.salt, nonce: w.nonce, ciphertext: w.ciphertext }
    }
}

impl From<domain::WrappedKey> for WrappedSecret {
    fn from(w: domain::WrappedKey) -> Self {
        WrappedSecret { salt: w.salt, nonce: w.nonce, ciphertext: w.ciphertext }
    }
}
