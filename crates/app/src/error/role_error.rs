//! Errors from voting on a custom role's colour.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RoleError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    /// Only an enfranchised citizen may vote on a role's colour.
    #[error("only a citizen of this server may do that")]
    NotACitizen,
    #[error("no such role: {0}")]
    NoSuchRole(u64),
    #[error("a colour must be a hex value like #3b82f6")]
    BadColor,
}
