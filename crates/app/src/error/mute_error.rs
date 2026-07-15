//! Errors from a police officer imposing or lifting a mute.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MuteError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    /// The actor is not a police officer of this server.
    #[error("only a police officer may do that here")]
    NotPolice,
    /// The target is not a member of this server.
    #[error("'{0}' is not a member of this server")]
    NotAMember(String),
    /// A police officer may not mute themselves or another officer.
    #[error("police officers cannot be muted")]
    CannotMutePolice,
    /// A vote lifted this officer's mute of this member; they are barred from
    /// re-muting them until the 24-hour cooldown elapses.
    #[error("a vote overturned your mute of this member; you cannot re-mute them yet")]
    RemuteBlocked,
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
