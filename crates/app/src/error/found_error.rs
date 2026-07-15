//! Errors from founding a server.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FoundError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("server slug '{0}' is already taken")]
    SlugTaken(String),
    #[error("name/slug must not be empty")]
    EmptyName,
    #[error("a franchise-barred account may not found a server")]
    FounderBarred,
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
