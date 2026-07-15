//! Errors from opening a governance proposal.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProposeError {
    #[error("no such user: '{0}'")]
    NoSuchUser(String),
    #[error("no such server: '{0}'")]
    NoSuchServer(String),
    /// The proposal being amended does not exist.
    #[error("no such proposal: {0}")]
    NoSuchProposal(u64),
    /// A closed proposal can no longer take amendments.
    #[error("this proposal is closed")]
    Closed,
    /// Only an enfranchised citizen may open a proposal.
    #[error("only a citizen may open a proposal here")]
    NotACitizen,
    /// The server has not enabled this kind of decision on its governance surface.
    #[error("this server does not put that to a vote")]
    NotGoverned,
    /// The decision class is not permitted in the server's current phase (e.g. a
    /// constitutional amendment during Seed).
    #[error("that decision is not permitted in this server's current phase")]
    NotAllowedInPhase,
    #[error("proposal is malformed: {0}")]
    Invalid(String),
    /// The persistence layer itself failed (store unavailable, timeout, or
    /// write conflict) — distinct from any domain-rule rejection.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
}
