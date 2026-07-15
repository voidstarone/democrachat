//! Persistence for governance proposals.

use domain::{Proposal, ProposalId, ServerId};

/// Persistence for proposals.
pub trait ProposalStore: Send + Sync {
    fn next_proposal_id(&self) -> ProposalId;
    fn insert_proposal(&self, proposal: Proposal);
    fn get_proposal(&self, id: ProposalId) -> Option<Proposal>;
    fn update_proposal(&self, proposal: Proposal);
    /// Every proposal in a server, newest id first is not required — callers sort.
    fn list_for_server(&self, server: ServerId) -> Vec<Proposal>;
}
