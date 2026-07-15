//! The gate before applying any replicated event: authenticate it and authorize it.

use domain::NodeId;

use crate::auth_error::AuthError;
use crate::change_event::ChangeEvent;
use crate::classify::classify;
use crate::derived_scope::DerivedScope;
use crate::ownership::owned_scope::OwnedScope;
use crate::ownership::ownership_registry::OwnershipRegistry;
use crate::scope_resolver::ScopeResolver;
use crate::signed_part::SignedPart;

/// **The** gate before applying any replicated event. Returns the verified
/// [`SignedPart`] only when all hold:
///
/// 1. the event is signed by the key the control plane has for its claimed node;
/// 2. the signing node is the **current owner** of the scope the row *actually*
///    belongs to — derived from the **payload** via [`classify`], resolving a
///    vote/reaction through its parent — never the envelope's self-declared scope;
/// 3. the event's epoch is **not older** than that scope's current epoch.
///
/// Binding the check to the payload-derived scope is what stops a node that
/// legitimately owns *one* scope from stamping its own scope onto an event whose
/// row belongs to *another* — which would let it forge memberships, votes, DMs, or
/// blocks fleet-wide and defeat the anti-takeover design.
pub async fn authorize(
    registry: &dyn OwnershipRegistry,
    event: &ChangeEvent,
    resolver: &dyn ScopeResolver,
) -> Result<SignedPart, AuthError> {
    // Untrusted peek, only to learn which node's key to fetch. Its contents are
    // not trusted until the signature verifies below.
    let claimed = event.peek().map_err(AuthError::Fed)?;
    let key = registry
        .public_key(NodeId(claimed.node))
        .await
        .map_err(|e| AuthError::Registry(e.0))?
        .ok_or(AuthError::UnknownNode)?;

    // Authenticity: signature over the received bytes, by that node's key.
    let part = event.verify(&key).map_err(AuthError::Fed)?;

    // The authoritative scope comes from the payload, not the envelope.
    let owned = match classify(&part) {
        DerivedScope::Owned(s) => s,
        DerivedScope::ViaProposal(pid) => match resolver.proposal_server(pid).await {
            Some(sid) => OwnedScope::Server(sid),
            // Parent not replicated yet — retry once it arrives (ordered puller).
            None => return Err(AuthError::Unowned),
        },
        DerivedScope::ViaMessage(mid) => match resolver.message_server(mid).await {
            Some(sid) => OwnedScope::Server(sid),
            None => return Err(AuthError::Unowned),
        },
        DerivedScope::Indeterminate => return Err(AuthError::ScopeMismatch),
    };

    // Authorization: rightful owner of the *derived* scope, non-stale epoch.
    let owner = registry
        .owner_of(owned)
        .await
        .map_err(|e| AuthError::Registry(e.0))?
        .ok_or(AuthError::Unowned)?;
    if owner.owner != NodeId(part.node) {
        return Err(AuthError::NotOwner);
    }
    if part.epoch < owner.epoch {
        return Err(AuthError::StaleEpoch);
    }
    Ok(part)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    use crate::change_op::ChangeOp;
    use crate::event_scope::EventScope;
    use crate::fed_error::FedError;
    use crate::node_keypair::NodeKeypair;
    use crate::ownership::claim_outcome::ClaimOutcome;
    use crate::ownership::in_memory_registry::InMemoryRegistry;

    /// A resolver that maps any proposal/message to one fixed server (or nothing).
    struct FixedServer(Option<u64>);
    #[async_trait]
    impl ScopeResolver for FixedServer {
        async fn proposal_server(&self, _: u64) -> Option<u64> {
            self.0
        }
        async fn message_server(&self, _: u64) -> Option<u64> {
            self.0
        }
    }

    /// A signed `messages` event whose payload names `server_id`.
    fn message_event(kp: &NodeKeypair, server_id: u64, epoch: u64) -> ChangeEvent {
        ChangeEvent::sign(
            kp,
            SignedPart {
                node: 0,
                epoch,
                seq: 1,
                scope: EventScope::Server(server_id),
                entity: "messages".into(),
                op: ChangeOp::Upsert,
                payload: serde_json::json!({ "id": 1, "server_id": server_id, "body": "hi" }),
            },
        )
    }

    async fn registry_owning(kp: &NodeKeypair, scope: OwnedScope) -> (InMemoryRegistry, u64) {
        let reg = InMemoryRegistry::new();
        reg.publish_key(kp.node(), &kp.public().to_hex()).await.unwrap();
        let ClaimOutcome::Claimed { epoch } = reg.claim(scope, kp.node()).await.unwrap() else {
            panic!("first claim must succeed");
        };
        (reg, epoch)
    }

    #[tokio::test]
    async fn the_owners_event_at_the_current_epoch_is_authorized() {
        let owner = NodeKeypair::generate(NodeId(1));
        let (reg, epoch) = registry_owning(&owner, OwnedScope::Server(7)).await;
        let ev = message_event(&owner, 7, epoch);
        assert!(authorize(&reg, &ev, &FixedServer(None)).await.is_ok());
    }

    #[tokio::test]
    async fn a_non_owner_is_rejected() {
        let owner = NodeKeypair::generate(NodeId(1));
        let (reg, epoch) = registry_owning(&owner, OwnedScope::Server(7)).await;
        // An impostor node, key published, correctly signs — but does not own s/7.
        let impostor = NodeKeypair::generate(NodeId(2));
        reg.publish_key(impostor.node(), &impostor.public().to_hex()).await.unwrap();
        let ev = message_event(&impostor, 7, epoch);
        assert_eq!(authorize(&reg, &ev, &FixedServer(None)).await, Err(AuthError::NotOwner));
    }

    #[tokio::test]
    async fn payload_forgery_across_scopes_is_rejected() {
        // THE anti-forgery property: node 1 owns Server(7). It signs a well-formed
        // `messages` event, but the payload's server_id is 8 — a server it does NOT
        // own. classify() derives the scope from the payload (Server(8)), so the
        // ownership check is against 8, which node 1 doesn't hold → rejected. A
        // self-declared envelope scope of 7 cannot launder the row into server 8.
        let node1 = NodeKeypair::generate(NodeId(1));
        let (reg, _epoch) = registry_owning(&node1, OwnedScope::Server(7)).await;
        let forged = ChangeEvent::sign(
            &node1,
            SignedPart {
                node: 0,
                epoch: 1,
                seq: 1,
                scope: EventScope::Server(7), // claims 7…
                entity: "messages".into(),
                op: ChangeOp::Upsert,
                payload: serde_json::json!({ "id": 1, "server_id": 8, "body": "gotcha" }), // …but writes into 8
            },
        );
        // Server(8) is unowned here → Unowned (and would be NotOwner if 8 had a
        // different owner) — either way, never applied.
        assert_eq!(authorize(&reg, &forged, &FixedServer(None)).await, Err(AuthError::Unowned));
    }

    #[tokio::test]
    async fn a_fenced_old_owner_is_rejected_after_rehoming() {
        // Node 1 owns Server(7) at epoch 1 and signs an event, then loses ownership.
        let node1 = NodeKeypair::generate(NodeId(1));
        let (reg, epoch1) = registry_owning(&node1, OwnedScope::Server(7)).await;
        let stale = message_event(&node1, 7, epoch1);
        // Rehome: node 2 takes over, bumping the epoch.
        let node2 = NodeKeypair::generate(NodeId(2));
        reg.publish_key(node2.node(), &node2.public().to_hex()).await.unwrap();
        reg.release(OwnedScope::Server(7), NodeId(1)).await.unwrap();
        reg.claim(OwnedScope::Server(7), NodeId(2)).await.unwrap();
        // Node 1's event, signed under the old epoch, is now from a non-owner.
        assert_eq!(authorize(&reg, &stale, &FixedServer(None)).await, Err(AuthError::NotOwner));
    }

    #[tokio::test]
    async fn a_vote_authorizes_against_its_proposals_server() {
        let owner = NodeKeypair::generate(NodeId(1));
        let (reg, epoch) = registry_owning(&owner, OwnedScope::Server(7)).await;
        let vote = ChangeEvent::sign(
            &owner,
            SignedPart {
                node: 0,
                epoch,
                seq: 2,
                scope: EventScope::Server(7),
                entity: "votes".into(),
                op: ChangeOp::Upsert,
                payload: serde_json::json!({ "proposal_id": 500, "voter": 1, "is_aye": true }),
            },
        );
        // Resolver places proposal 500 in server 7 (which the owner holds) → ok.
        assert!(authorize(&reg, &vote, &FixedServer(Some(7))).await.is_ok());
        // If the parent proposal hasn't replicated yet, retry later.
        assert_eq!(authorize(&reg, &vote, &FixedServer(None)).await, Err(AuthError::Unowned));
    }

    #[tokio::test]
    async fn an_unknown_node_and_a_tampered_event_are_rejected() {
        let owner = NodeKeypair::generate(NodeId(1));
        let reg = InMemoryRegistry::new();
        reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap();
        // Key never published → we can't fetch it.
        let ev = message_event(&owner, 7, 1);
        assert_eq!(authorize(&reg, &ev, &FixedServer(None)).await, Err(AuthError::UnknownNode));
        // Publish, then tamper the body → signature fails.
        reg.publish_key(NodeId(1), &owner.public().to_hex()).await.unwrap();
        let tampered = ChangeEvent::from_wire(
            ev.body().replace("hi", "HIJACKED"),
            ev.signature().to_string(),
        );
        assert_eq!(
            authorize(&reg, &tampered, &FixedServer(None)).await,
            Err(AuthError::Fed(FedError::BadSignature))
        );
    }
}
