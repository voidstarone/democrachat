//! Apply a peer's change feed to the local replica, gated by full authorization
//! and advancing an ordered per-peer cursor.

use std::sync::Arc;

use adapter_store_memory::MemoryStore;
use domain::NodeId;
use federation::{authorize, ChangeEvent, Ingested, OwnershipRegistry, ScopeResolver};

use crate::is_transient::is_transient;

/// The consumer side of replication. Holds the local store, the control-plane
/// registry (whose key/owner/epoch each event is checked against), and the
/// parent-scope resolver. The one choke point where an untrusted peer's bytes
/// cross into the replica — every event passes `federation::authorize` before it
/// can reach `MemoryStore::apply_incoming`.
pub struct Replicator {
    store: Arc<MemoryStore>,
    registry: Arc<dyn OwnershipRegistry>,
    resolver: Arc<dyn ScopeResolver>,
}

impl Replicator {
    pub fn new(
        store: Arc<MemoryStore>,
        registry: Arc<dyn OwnershipRegistry>,
        resolver: Arc<dyn ScopeResolver>,
    ) -> Self {
        Self { store, registry, resolver }
    }

    /// This node's replication cursor for `peer` — where its next pull resumes.
    pub fn cursor(&self, peer: NodeId) -> u64 {
        self.store.replication_cursor(peer)
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
        let mut handled_high = self.store.replication_cursor(peer);
        for event in events {
            let seq = event.peek().map(|p| p.seq).unwrap_or(0);
            match authorize(self.registry.as_ref(), event, self.resolver.as_ref()).await {
                Ok(part) => {
                    let applied_seq = part.seq;
                    match self.store.apply_incoming(&part) {
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
        self.store.advance_cursor(peer, handled_high);
        out
    }
}
