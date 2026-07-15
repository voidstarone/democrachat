//! Errors from casting a vote on a proposal.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VoteError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such proposal: {0}")]
    NoSuchProposal(u64),
    /// Only an enfranchised citizen of this server may vote.
    #[error("only a citizen of this server may vote")]
    NotACitizen,
    #[error("this proposal is closed")]
    Closed,
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
