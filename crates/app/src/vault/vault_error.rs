//! Errors from the at-rest vault.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VaultError {
    /// The configured key is not a valid 32-byte hex string.
    #[error("invalid vault key: {0}")]
    BadKey(String),
    /// The sealed envelope is malformed (bad hex, missing field, wrong version).
    #[error("malformed sealed data: {0}")]
    Format(String),
    /// Authenticated decryption failed — the wrong key, or the data was tampered
    /// with. The two are deliberately indistinguishable (an AEAD tag failure).
    #[error("could not decrypt sealed data (wrong key or tampered)")]
    Decrypt,
}
