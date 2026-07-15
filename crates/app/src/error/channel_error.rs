//! Errors from creating or addressing a channel.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChannelError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    #[error("channel name must not be empty")]
    EmptyName,
    #[error("channel '#{0}' already exists in this server")]
    NameTaken(String),
    /// A server past the Seed phase governs its channel layout by ballot (M3), not
    /// by founder fiat.
    #[error("only the founder may create channels while the server is in Seed; a larger server must vote (not yet implemented)")]
    NotProvisionable,
}
