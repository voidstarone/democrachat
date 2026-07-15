//! Apply a peer's change feed to the local replica, gated by full authorization
//! and advancing an ordered per-peer cursor.

use std::sync::Arc;

use domain::NodeId;
use federation::{
    authorize, ChangeEvent, ChangeSink, Ingested, OwnershipRegistry, ReplicationCursor,
    ScopeResolver,
};

use crate::is_transient::is_transient;

/// The consumer side of replication. Holds the local sink (where authorized events
/// land) and cursor (the per-peer replay position), the control-plane registry
/// (whose key/owner/epoch each event is checked against), and the parent-scope
/// resolver. The one choke point where an untrusted peer's bytes cross into the
/// replica — every event passes `federation::authorize` before it can reach the
/// sink.
///
/// Store-agnostic: `sink` and `cursor` are trait objects, so the same replicator
/// drives the in-memory store or Postgres.
pub struct Replicator {
    sink: Arc<dyn ChangeSink>,
    cursor: Arc<dyn ReplicationCursor>,
    registry: Arc<dyn OwnershipRegistry>,
    resolver: Arc<dyn ScopeResolver>,
}

impl Replicator {
    pub fn new(
        sink: Arc<dyn ChangeSink>,
        cursor: Arc<dyn ReplicationCursor>,
        registry: Arc<dyn OwnershipRegistry>,
        resolver: Arc<dyn ScopeResolver>,
    ) -> Self {
        Self { sink, cursor, registry, resolver }
    }

    /// This node's replication cursor for `peer` — where its next pull resumes.
    pub async fn cursor(&self, peer: NodeId) -> u64 {
        self.cursor.replication_cursor(peer).await
    }

    /// Authorize events **in order** and apply the authorized ones, then advance
    /// the peer's cursor over the handled prefix.
    ///
    /// A peer's feed is a strictly ordered log, so a rejection is handled by its
    /// kind (see [`is_transient`]): a **transient** failure *stops* the batch with
    /// the cursor left before the event, so a later pull retries it; a
    /// **permanent** one is *skipped* and the cursor advances past it, so one dead
    /// event can never stall all later replication from that peer. A locally
    /// malformed row (authentic + authorized, but its payload won't deserialize)
    /// is likewise skipped past rather than allowed to poison the replica.
    pub async fn ingest(&self, peer: NodeId, events: &[ChangeEvent]) -> Ingested {
        let mut out = Ingested::default();
        // Never regress: start from the cursor already recorded for this peer.
        let mut handled_high = self.cursor.replication_cursor(peer).await;
        for event in events {
            let seq = event.peek().map(|p| p.seq).unwrap_or(0);
            match authorize(self.registry.as_ref(), event, self.resolver.as_ref()).await {
                Ok(part) => {
                    let applied_seq = part.seq;
                    match self.sink.apply(&part).await {
                        Ok(()) => out.applied += 1,
                        Err(e) => out.apply_errors.push(e), // malformed — skip past
                    }
                    handled_high = handled_high.max(applied_seq);
                }
                Err(e) if is_transient(&e) => {
                    // Stop: leave the cursor before this event so it is retried.
                    out.rejected.push(e);
                    break;
                }
                Err(e) => {
                    // Permanent: skip it and let the cursor step past.
                    out.rejected.push(e);
                    handled_high = handled_high.max(seq);
                }
            }
        }
        self.cursor.advance_cursor(peer, handled_high).await;
        out
    }
}
