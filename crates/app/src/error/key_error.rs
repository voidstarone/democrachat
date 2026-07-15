//! Errors from publishing or fetching a user's directory keys.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    /// The submitted public key is not a valid X25519 key.
    #[error("invalid public key: {0}")]
    BadPublicKey(String),
    /// The user has not published a directory entry yet.
    #[error("no keys published for '{0}'")]
    NotPublished(String),
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
