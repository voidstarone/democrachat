//! Errors from adding or voting on a custom emoji.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EmojiError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    /// Only an enfranchised citizen may add or vote on emoji.
    #[error("only a citizen of this server may do that")]
    NotACitizen,
    #[error("an emoji name must be letters, numbers, - or _")]
    BadName,
    #[error("an emoji named ':{0}:' already exists here")]
    NameTaken(String),
    #[error("the image is invalid: {0}")]
    BadImage(String),
    #[error("no such emoji: {0}")]
    NoSuchEmoji(u64),
}
