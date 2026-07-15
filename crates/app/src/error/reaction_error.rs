//! Errors from adding or removing a reaction.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReactionError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such message: {0}")]
    NoSuchMessage(u64),
    #[error("'{0}' is not a member of this server and cannot react")]
    NotAMember(String),
    #[error("emoji must not be empty")]
    EmptyEmoji,
}
