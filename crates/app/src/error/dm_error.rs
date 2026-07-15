//! Errors from sending a direct message.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DmError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    /// A user may not DM themselves.
    #[error("cannot send a direct message to yourself")]
    Self_,
    /// The recipient is unreachable: either party has blocked the other, or the
    /// recipient only accepts DMs from friends and the sender isn't one.
    #[error("you can't message this user")]
    NotAllowed,
    /// A sealed body (one ciphertext per party) was missing — the server never
    /// sees plaintext, so "empty" here means an absent ciphertext.
    #[error("message body cannot be empty")]
    EmptyBody,
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
