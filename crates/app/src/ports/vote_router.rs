//! Driven port: route a citizen's vote to the node that owns the proposal's server.

use async_trait::async_trait;

/// Where the web vote handler sends a ballot when the deployment is federated. The
/// single-box deployment leaves this **unset** and votes apply locally through
/// [`Services::cast_vote`](crate::Services::cast_vote); a federated deployment
/// supplies a router that either applies locally (this node owns the server) or
/// forwards a signed command to the owner.
///
/// The voter is identified by user id — the caller has already resolved it from the
/// authenticated session — and any failure collapses to a message the web layer
/// surfaces as a `400`. This is the one async port; unlike the synchronous store
/// ports, routing a write can cross the network.
#[async_trait]
pub trait VoteRouter: Send + Sync {
    async fn cast_vote(&self, voter_id: u64, proposal_id: u64, is_aye: bool) -> Result<(), String>;
}
