//! Errors from managing an encrypted channel's keys (enable, grant, fetch).

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChannelKeyError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    #[error("no such channel: '#{0}'")]
    NoSuchChannel(String),
    /// The actor is not a member of the channel's server.
    #[error("'{0}' is not a member of this server")]
    NotAMember(String),
    /// Only a citizen may turn on channel encryption.
    #[error("only a citizen of this server may do that")]
    NotACitizen,
    /// A grant was submitted for a channel that is not encrypted.
    #[error("this channel is not encrypted")]
    NotEncrypted,
    /// The sealed key blob was empty.
    #[error("the sealed key is empty")]
    EmptySeal,
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
