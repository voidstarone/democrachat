//! Errors from the end-to-end encryption primitives.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum E2eeError {
    /// A hex-encoded key or blob was malformed or the wrong length.
    #[error("malformed key material: {0}")]
    BadKey(String),
    /// A sealed blob was too short to contain its header (ephemeral key + nonce).
    #[error("malformed sealed message")]
    BadSealed,
    /// Authenticated decryption failed — the wrong recipient key, a wrong password,
    /// or tampered data. Deliberately indistinguishable (an AEAD tag failure).
    #[error("could not decrypt (wrong key/password or tampered)")]
    Decrypt,
}
