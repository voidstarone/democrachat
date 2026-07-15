//! Ingest a peer's change feed: authorize each event, apply the authorized ones.

use crate::authorize::authorize;
use crate::change_event::ChangeEvent;
use crate::change_sink::ChangeSink;
use crate::ingested::Ingested;
use crate::ownership::ownership_registry::OwnershipRegistry;
use crate::scope_resolver::ScopeResolver;

/// Apply a batch of events pulled from a peer. Each passes
/// [`authorize`](crate::authorize::authorize) (signature + rightful-owner +
/// non-stale-epoch, scope derived from the payload) **before** it can reach the
/// [`ChangeSink`]; an unsigned, forged, non-owner, or fenced-old-owner event is
/// dropped, never applied. This is the one place a peer's untrusted bytes cross
/// into the local replica, so the gate lives here and nowhere else.
pub async fn ingest(
    registry: &dyn OwnershipRegistry,
    resolver: &dyn ScopeResolver,
    sink: &dyn ChangeSink,
    events: &[ChangeEvent],
) -> Ingested {
    let mut out = Ingested::default();
    for event in events {
        match authorize(registry, event, resolver).await {
            Ok(part) => match sink.apply(&part).await {
                Ok(()) => out.applied += 1,
                Err(e) => out.apply_errors.push(e),
            },
            Err(e) => out.rejected.push(e),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    use domain::NodeId;

    use crate::change_op::ChangeOp;
    use crate::event_scope::EventScope;
    use crate::node_keypair::NodeKeypair;
    use crate::ownership::in_memory_registry::InMemoryRegistry;
    use crate::ownership::owned_scope::OwnedScope;
    use crate::signed_part::SignedPart;
    use crate::AuthError;

    /// A sink that records the ids it applied.
    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<u64>>);
    #[async_trait]
    impl ChangeSink for RecordingSink {
        async fn apply(&self, part: &SignedPart) -> Result<(), String> {
            let id = part.payload.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            self.0.lock().unwrap().push(id);
            Ok(())
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

    fn msg(kp: &NodeKeypair, id: u64, server_id: u64, epoch: u64) -> ChangeEvent {
        ChangeEvent::sign(
            kp,
            SignedPart {
                node: 0,
                epoch,
                seq: id,
                scope: EventScope::Server(server_id),
                entity: "messages".into(),
                op: ChangeOp::Upsert,
                payload: serde_json::json!({ "id": id, "server_id": server_id }),
            },
        )
    }

    #[tokio::test]
    async fn only_authorized_events_reach_the_sink() {
        let owner = NodeKeypair::generate(NodeId(1));
        let reg = InMemoryRegistry::new();
        reg.publish_key(NodeId(1), &owner.public().to_hex()).await.unwrap();
        reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap();

        // An interloper that owns nothing.
        let rogue = NodeKeypair::generate(NodeId(2));
        reg.publish_key(NodeId(2), &rogue.public().to_hex()).await.unwrap();

        let sink = RecordingSink::default();
        let batch = [
            msg(&owner, 100, 7, 1), // ok — owner of s/7
            msg(&rogue, 101, 7, 1), // rejected — not the owner
            msg(&owner, 102, 8, 1), // rejected — owner doesn't hold s/8 (forgery guard)
            msg(&owner, 103, 7, 1), // ok
        ];
        let res = ingest(&reg, &NoParents, &sink, &batch).await;

        assert_eq!(res.applied, 2);
        assert_eq!(sink.0.lock().unwrap().clone(), vec![100, 103], "only the owner's own-scope rows applied");
        assert_eq!(res.rejected, vec![AuthError::NotOwner, AuthError::Unowned]);
        assert!(res.apply_errors.is_empty());
    }
}
