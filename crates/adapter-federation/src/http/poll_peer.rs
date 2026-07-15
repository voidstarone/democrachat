//! Pull one page from a peer and apply the authorized events.

use crate::http::peer::Peer;
use crate::replicator::Replicator;

/// Pull one page from `peer`, resuming at this node's cursor for it, and apply the
/// authorized events. Returns how many were applied. A transport error surfaces as
/// `Err`; an authorization rejection is logged, not fatal (the feed is ordered, so
/// the replicator decides retry-vs-skip).
pub async fn poll_peer(replicator: &Replicator, peer: &Peer, limit: u64) -> Result<u64, String> {
    let since = replicator.cursor(peer.node).await;
    let events = peer.client.changes_since(since, limit).await?;
    if events.is_empty() {
        return Ok(0);
    }
    let out = replicator.ingest(peer.node, &events).await;
    for why in &out.rejected {
        tracing::warn!(peer = peer.node.0, "rejected federated event: {why}");
    }
    for err in &out.apply_errors {
        tracing::warn!(peer = peer.node.0, "skipped malformed federated row: {err}");
    }
    Ok(out.applied)
}
