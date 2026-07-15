//! Resolve a parent-scoped row's scope from the local replica.

use std::sync::Arc;

use adapter_store_memory::MemoryStore;
use app::{MessageStore, ProposalStore};
use async_trait::async_trait;
use domain::{MessageId, ProposalId};
use federation::ScopeResolver;

/// The data-access half of `federation::authorize`'s parent-scoped
/// classification: a vote's server is its proposal's, a reaction's is its
/// message's. `classify` decides a row is parent-scoped; this looks the parent up
/// in the local store. A parent not yet replicated → `None`, which `authorize`
/// turns into a transient `Unowned` (retried on the next pull), so a child that
/// races ahead of its parent is not lost.
pub struct StoreResolver(pub Arc<MemoryStore>);

#[async_trait]
impl ScopeResolver for StoreResolver {
    async fn proposal_server(&self, proposal: u64) -> Option<u64> {
        self.0.get_proposal(ProposalId(proposal)).map(|p| p.server_id.0)
    }
    async fn message_server(&self, message: u64) -> Option<u64> {
        self.0.get_message(MessageId(message)).map(|m| m.server_id.0)
    }
}
