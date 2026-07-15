//! Persistence for proposal votes.

use domain::{ProposalId, UserId, Vote};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for votes on proposals.
#[async_trait]
pub trait VoteStore: Send + Sync {
    /// Record a vote, replacing any prior vote by the same citizen on the same
    /// proposal (a citizen may change their mind while the ballot is open).
    async fn upsert_vote(&self, vote: Vote) -> Result<(), StoreError>;
    async fn get_vote(&self, proposal: ProposalId, voter: UserId) -> Result<Option<Vote>, StoreError>;
    async fn list_for_proposal(&self, proposal: ProposalId) -> Result<Vec<Vote>, StoreError>;
    /// Discard every vote on a proposal — used when an amendment changes the ballot,
    /// so prior votes (cast on the old bundle) no longer count.
    async fn clear_for_proposal(&self, proposal: ProposalId) -> Result<(), StoreError>;
}
