//! Resolve a parent-scoped row (a vote, a reaction) to its server.

use async_trait::async_trait;

/// Resolves the server of a row that doesn't carry `server_id` itself — a vote
/// (via its proposal) or a reaction (via its message). The consumer answers from
/// its **own replica**, so a vote/reaction is authorized against the same server
/// its parent already replicated under; a parent not yet present yields `None`,
/// and the puller retries once it arrives (ordered application).
#[async_trait]
pub trait ScopeResolver: Send + Sync {
    /// The server a proposal belongs to, if this node has it.
    async fn proposal_server(&self, proposal_id: u64) -> Option<u64>;
    /// The server a message belongs to, if this node has it.
    async fn message_server(&self, message_id: u64) -> Option<u64>;
}
