//! Errors from joining a server.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum JoinError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    #[error("already a member of '{0}'")]
    AlreadyMember(String),
}
