//! Turn a node's outbox into a signed, epoch-stamped change feed.

use std::collections::HashMap;

use crate::change_event::ChangeEvent;
use crate::change_source::ChangeSource;
use crate::classify::classify;
use crate::derived_scope::DerivedScope;
use crate::event_scope::EventScope;
use crate::node_keypair::NodeKeypair;
use crate::ownership::owned_scope::OwnedScope;
use crate::ownership::ownership_registry::OwnershipRegistry;
use crate::scope_resolver::ScopeResolver;
use crate::signed_part::SignedPart;

/// Produce this node's change feed after `after_seq`, each event **signed** and
/// stamped with the current ownership epoch of the scope its row belongs to (so a
/// consumer can fence a feed produced under a stale epoch). The scope is derived
/// from the payload by [`classify`] — the same classification the consumer's
/// [`authorize`](crate::authorize::authorize) uses — so the epoch is stamped for
/// exactly the scope the consumer will check ownership against.
///
/// A row that classifies to no replicable scope is skipped (not put on the wire).
pub async fn sign_feed(
    source: &dyn ChangeSource,
    keypair: &NodeKeypair,
    registry: &dyn OwnershipRegistry,
    resolver: &dyn ScopeResolver,
    after_seq: u64,
    limit: u64,
) -> Vec<ChangeEvent> {
    let records = source.changes_since(after_seq, limit).await;
    let mut out = Vec::with_capacity(records.len());
    // One control-plane lookup per distinct scope, not per event (a batch tends to
    // be dominated by one scope). `None` = we do not currently own this scope, so its
    // rows are left off the wire (see below).
    let mut epoch_cache: HashMap<OwnedScope, Option<u64>> = HashMap::new();

    for rec in records {
        // Build the part first, then classify it by reference (no clones).
        let mut part = SignedPart {
            node: 0,                     // stamped by sign()
            epoch: 0,                    // set below, once the scope is known
            seq: rec.seq,
            scope: EventScope::Global,   // replaced with the derived scope below
            entity: rec.entity,
            op: rec.op,
            payload: rec.payload,
        };
        let owned = match classify(&part) {
            DerivedScope::Owned(s) => Some(s),
            DerivedScope::ViaProposal(pid) => {
                resolver.proposal_server(pid).await.map(OwnedScope::Server)
            }
            DerivedScope::ViaMessage(mid) => {
                resolver.message_server(mid).await.map(OwnedScope::Server)
            }
            DerivedScope::Indeterminate => None,
        };
        let Some(owned) = owned else {
            continue; // not a replicable row — leave it off the wire
        };
        // Only sign rows for scopes THIS node currently owns, at the owner's live
        // epoch. A row whose scope we don't own — because we haven't claimed it yet
        // (it was just minted) or it rehomed away — is left off the wire. Signing it
        // with a placeholder epoch is the bug this guards against: a peer that
        // received such a row would permanently skip it as `StaleEpoch` the instant
        // the scope *is* claimed at epoch ≥ 1, losing the row forever. Left off, the
        // row is simply offered on a later pull once we own it, with the real epoch.
        let epoch = match epoch_cache.get(&owned) {
            Some(&cached) => cached,
            None => {
                let mine = match registry.owner_of(owned).await.ok().flatten() {
                    Some(o) if o.owner == keypair.node() => Some(o.epoch),
                    _ => None,
                };
                epoch_cache.insert(owned, mine);
                mine
            }
        };
        let Some(epoch) = epoch else {
            continue; // not ours right now — offer it on a later pull
        };
        part.epoch = epoch;
        part.scope = EventScope::from(owned);
        out.push(ChangeEvent::sign(keypair, part));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    use domain::NodeId;

    use crate::change_op::ChangeOp;
    use crate::change_record::ChangeRecord;
    use crate::change_sink::ChangeSink;
    use crate::ingest::ingest;
    use crate::ownership::in_memory_registry::InMemoryRegistry;

    /// A fixed outbox.
    struct FakeSource(Vec<ChangeRecord>);
    #[async_trait]
    impl ChangeSource for FakeSource {
        async fn changes_since(&self, after_seq: u64, limit: u64) -> Vec<ChangeRecord> {
            self.0
                .iter()
                .filter(|r| r.seq > after_seq)
                .take(limit as usize)
                .cloned()
                .collect()
        }
    }

    struct NoParents;
    #[async_trait]
    impl ScopeResolver for NoParents {
        async fn proposal_server(&self, _: u64) -> Option<u64> {
            None
        }
        async fn message_server(&self, _: u64) -> Option<u64> {
            None
        }
    }

    #[derive(Default)]
    struct RecordingSink(std::sync::Mutex<Vec<u64>>);
    #[async_trait]
    impl ChangeSink for RecordingSink {
        async fn apply(&self, part: &SignedPart) -> Result<(), String> {
            let id = part.payload.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            self.0.lock().unwrap().push(id);
            Ok(())
        }
    }

    fn rec(seq: u64, entity: &str, payload: serde_json::Value) -> ChangeRecord {
        ChangeRecord { seq, entity: entity.into(), op: ChangeOp::Upsert, payload }
    }

    #[tokio::test]
    async fn a_signed_feed_round_trips_through_ingest() {
        // One shared control plane models two nodes agreeing on ownership + keys.
        let a = NodeKeypair::generate(NodeId(1));
        let reg = InMemoryRegistry::new();
        reg.publish_key(NodeId(1), &a.public().to_hex()).await.unwrap();
        reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap();

        // Node A's outbox: two Server(7) messages, and one un-replicable row.
        let source = FakeSource(vec![
            rec(1, "messages", serde_json::json!({ "id": 100, "server_id": 7 })),
            rec(2, "gremlins", serde_json::json!({ "id": 999 })), // skipped by classify
            rec(3, "messages", serde_json::json!({ "id": 101, "server_id": 7 })),
        ]);

        let feed = sign_feed(&source, &a, &reg, &NoParents, 0, 100).await;
        assert_eq!(feed.len(), 2, "the un-replicable row is left off the wire");

        // Node B ingests A's feed against the same control plane.
        let sink = RecordingSink::default();
        let res = ingest(&reg, &NoParents, &sink, &feed).await;
        assert_eq!(res.applied, 2);
        assert_eq!(sink.0.lock().unwrap().clone(), vec![100, 101]);
    }

    #[tokio::test]
    async fn a_feed_produced_under_a_stale_epoch_is_fenced_on_ingest() {
        let a = NodeKeypair::generate(NodeId(1));
        let reg = InMemoryRegistry::new();
        reg.publish_key(NodeId(1), &a.public().to_hex()).await.unwrap();
        reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap();

        // A produces a feed while it still owns Server(7)…
        let source = FakeSource(vec![rec(1, "messages", serde_json::json!({ "id": 1, "server_id": 7 }))]);
        let feed = sign_feed(&source, &a, &reg, &NoParents, 0, 100).await;

        // …then Server(7) rehomes to node B (epoch bumps), fencing A.
        let b = NodeKeypair::generate(NodeId(2));
        reg.publish_key(NodeId(2), &b.public().to_hex()).await.unwrap();
        reg.release(OwnedScope::Server(7), NodeId(1)).await.unwrap();
        reg.claim(OwnedScope::Server(7), NodeId(2)).await.unwrap();

        let sink = RecordingSink::default();
        let res = ingest(&reg, &NoParents, &sink, &feed).await;
        assert_eq!(res.applied, 0, "A is no longer the owner");
        assert!(sink.0.lock().unwrap().is_empty());
    }
}
