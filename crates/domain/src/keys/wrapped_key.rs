//! A device secret key wrapped under the user's password — opaque to the server.

use serde::{Deserialize, Serialize};

/// A user's device secret key encrypted under a key derived from their password
/// (Argon2id → KEK, AEAD wrap). The server stores this blob **verbatim** and hands
/// it back at login; only the user, client-side with their password, can unwrap it,
/// so the server is blind to the secret. Recovering it on a new device is just
/// re-entering the password. All fields are hex; the server never inspects them.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct WrappedKey {
    /// Argon2id salt (per-user, random).
    pub salt: String,
    /// AEAD nonce for the wrap.
    pub nonce: String,
    /// The 32-byte device secret, encrypted under the password-derived key.
    pub ciphertext: String,
}
