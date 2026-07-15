//! Errors from minting or redeeming a server invite code.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InviteError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    /// The actor is not a member of the server, so may not mint invites for it.
    #[error("not a member of '{0}'")]
    NotMember(String),
    /// The server's invite policy is `Closed` — no member may mint or redeem.
    #[error("invites are closed for '{0}'")]
    Closed(String),
    /// The presented code matches no live invite (unknown or revoked).
    #[error("invalid or revoked invite code")]
    InvalidCode,
    /// The redeemer already belongs to the server.
    #[error("already a member of '{0}'")]
    AlreadyMember(String),
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
