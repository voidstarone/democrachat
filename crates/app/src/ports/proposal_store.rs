//! Persistence for governance proposals.

use domain::{Proposal, ProposalId, ServerId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for proposals.
#[async_trait]
pub trait ProposalStore: Send + Sync {
    async fn next_proposal_id(&self) -> Result<ProposalId, StoreError>;
    async fn insert_proposal(&self, proposal: Proposal) -> Result<(), StoreError>;
    async fn get_proposal(&self, id: ProposalId) -> Result<Option<Proposal>, StoreError>;
    async fn update_proposal(&self, proposal: Proposal) -> Result<(), StoreError>;
    /// Every proposal in a server, newest id first is not required — callers sort.
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Proposal>, StoreError>;
}
