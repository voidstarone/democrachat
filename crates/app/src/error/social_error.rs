//! Errors from blocking a user or managing friendships.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SocialError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    /// A user may not block or befriend themselves.
    #[error("cannot do that to yourself")]
    Self_,
    /// No pending friend request exists to accept.
    #[error("no pending friend request from that user")]
    NoPendingRequest,
}
