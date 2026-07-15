//! Background loop that polls every peer forever.

use std::sync::Arc;
use std::time::Duration;

use crate::http::peer::Peer;
use crate::http::poll_peer::poll_peer;
use crate::replicator::Replicator;

/// Spawn a background task that polls every peer on `interval`, forever. A failed
/// poll is logged and retried next tick — a down or misbehaving peer never takes
/// this node down. The interval is floored at one second so a misconfiguration
/// can't turn into a hot loop.
pub fn spawn_puller(
    replicator: Arc<Replicator>,
    peers: Vec<Peer>,
    interval: Duration,
    limit: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval.max(Duration::from_secs(1)));
        loop {
            tick.tick().await;
            for peer in &peers {
                match poll_peer(&replicator, peer, limit).await {
                    Ok(n) if n > 0 => {
                        tracing::info!(peer = peer.node.0, applied = n, "replicated")
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(peer = peer.node.0, "poll failed: {e}"),
                }
            }
        }
    })
}
