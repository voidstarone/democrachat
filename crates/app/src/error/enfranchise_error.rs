//! Errors from an enfranchisement attempt.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EnfranchiseError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    #[error("'{0}' is not a member of this server")]
    NotAMember(String),
    #[error("'{0}' is already a citizen of this server")]
    AlreadyCitizen(String),
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
