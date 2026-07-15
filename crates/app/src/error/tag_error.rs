//! Errors from setting or searching tags on servers, channels, and users.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TagError {
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    #[error("no such channel: '{0}'")]
    NoSuchChannel(String),
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    /// The caller is not permitted to edit these tags (only a server's founder may
    /// tag the server or its channels).
    #[error("not permitted to edit those tags")]
    Forbidden,
    /// The persistence layer itself failed (store unavailable, timeout, or write
    /// conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
