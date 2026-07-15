//! Where authorized events are applied — a node's local replica.

use async_trait::async_trait;

use crate::signed_part::SignedPart;

/// The apply side of replication: write a **verified** event's row into this
/// node's replica. Only [`ingest`](crate::ingest::ingest) calls this, and only
/// after [`authorize`](crate::authorize::authorize) has passed — so an
/// implementation may trust the `part` it is handed (it is signed by the row's
/// rightful owner at a non-stale epoch). An `Err` is a local apply failure (e.g.
/// a store error), not an authorization decision.
#[async_trait]
pub trait ChangeSink: Send + Sync {
    async fn apply(&self, part: &SignedPart) -> Result<(), String>;
}
