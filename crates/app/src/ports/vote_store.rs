//! Persistence for proposal votes.

use domain::{ProposalId, UserId, Vote};
use crate::StoreError;

/// Persistence for votes on proposals.
pub trait VoteStore: Send + Sync {
    /// Record a vote, replacing any prior vote by the same citizen on the same
    /// proposal (a citizen may change their mind while the ballot is open).
    fn upsert_vote(&self, vote: Vote) -> Result<(), StoreError>;
    fn get_vote(&self, proposal: ProposalId, voter: UserId) -> Result<Option<Vote>, StoreError>;
    fn list_for_proposal(&self, proposal: ProposalId) -> Result<Vec<Vote>, StoreError>;
}
