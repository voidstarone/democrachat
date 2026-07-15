//! Persistence for governance proposals.

use domain::{Proposal, ProposalId, ServerId};
use crate::StoreError;

/// Persistence for proposals.
pub trait ProposalStore: Send + Sync {
    fn next_proposal_id(&self) -> Result<ProposalId, StoreError>;
    fn insert_proposal(&self, proposal: Proposal) -> Result<(), StoreError>;
    fn get_proposal(&self, id: ProposalId) -> Result<Option<Proposal>, StoreError>;
    fn update_proposal(&self, proposal: Proposal) -> Result<(), StoreError>;
    /// Every proposal in a server, newest id first is not required — callers sort.
    fn list_for_server(&self, server: ServerId) -> Result<Vec<Proposal>, StoreError>;
}
