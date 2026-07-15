//! Errors from voting on a custom role's colour.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RoleError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    /// The caller isn't a member of the server they're trying to act on.
    #[error("not a member of this server: '{0}'")]
    NotAMember(String),
    /// Only an enfranchised citizen may vote on a role's colour.
    #[error("only a citizen of this server may do that")]
    NotACitizen,
    #[error("no such role: {0}")]
    NoSuchRole(u64),
    #[error("a colour must be a hex value like #3b82f6")]
    BadColor,
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
