//! Errors from registering a platform account.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegisterError {
    #[error("handle '{0}' is already taken")]
    HandleTaken(String),
    #[error("handle must not be empty")]
    EmptyHandle,
    /// The chosen password failed the length policy.
    #[error("{0}")]
    WeakPassword(String),
    /// Argon2 hashing failed (should be effectively impossible).
    #[error("could not secure the password")]
    HashFailed,
}
