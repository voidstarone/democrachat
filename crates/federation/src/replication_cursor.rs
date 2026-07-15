//! A node's per-peer replication cursor — how far it has applied each peer's feed.

use async_trait::async_trait;
use domain::NodeId;

/// Persistence for the replay cursor the [`Replicator`](crate) keeps per peer: the
/// highest feed `seq` this node has applied from that peer, where its next pull
/// resumes. The store implements it (in-memory map, or a Postgres row); the
/// replication logic stays store-agnostic. Both operations are infallible from the
/// caller's view — a read failure degrades to `0` (re-pull from the start), an
/// advance failure is dropped (the next pull simply re-applies idempotently).
#[async_trait]
pub trait ReplicationCursor: Send + Sync {
    /// The highest applied `seq` for `peer`, or `0` if none recorded.
    async fn replication_cursor(&self, peer: NodeId) -> u64;
    /// Advance `peer`'s cursor to `seq`. Monotonic: a lower `seq` never moves it back.
    async fn advance_cursor(&self, peer: NodeId, seq: u64);
}
