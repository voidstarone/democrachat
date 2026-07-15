//! A process-local `OwnershipRegistry` for single-node / dev / tests.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use async_trait::async_trait;

use domain::NodeId;

use crate::node_public_key::NodePublicKey;
use crate::ownership::claim_outcome::ClaimOutcome;
use crate::ownership::node_load::NodeLoad;
use crate::ownership::node_status::NodeStatus;
use crate::ownership::owned_scope::OwnedScope;
use crate::ownership::ownership::Ownership;
use crate::ownership::ownership_registry::OwnershipRegistry;
use crate::ownership::registry_error::RegistryError;

#[derive(Default)]
struct RegistryState {
    /// scope → (owner, epoch). Epoch is monotonic per scope and survives handoff.
    owners: HashMap<OwnedScope, (NodeId, u64)>,
    /// The highest epoch ever assigned to a scope, so a re-claim always bumps.
    max_epoch: HashMap<OwnedScope, u64>,
    /// node → public key hex.
    keys: HashMap<u16, String>,
    /// node → last-reported load (presence implies "live" in this simple model).
    loads: HashMap<u16, NodeLoad>,
    /// scope → designated standby nodes.
    standbys: HashMap<OwnedScope, Vec<u16>>,
    /// Scopes whose community has **opted out** of automatic rehoming. Absent ⇒
    /// rehoming allowed (the default), so this only records the exceptions.
    rehoming_disabled: HashSet<OwnedScope>,
}

/// A process-local [`OwnershipRegistry`] with no real leases — ownership is
/// explicit ([`claim`](OwnershipRegistry::claim) / [`release`](OwnershipRegistry::release)).
/// It models the epoch-fencing semantics faithfully (a re-claim always bumps past
/// the highest epoch the scope ever had), which is what the authorization logic
/// depends on. Good for a single-node deployment and for deterministic tests;
/// production uses the etcd adapter (M4).
#[derive(Default)]
pub struct InMemoryRegistry {
    state: Mutex<RegistryState>,
}

impl InMemoryRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl OwnershipRegistry for InMemoryRegistry {
    async fn owner_of(&self, scope: OwnedScope) -> Result<Option<Ownership>, RegistryError> {
        let s = self.state.lock().unwrap();
        Ok(s.owners.get(&scope).map(|&(owner, epoch)| Ownership {
            scope,
            owner,
            epoch,
        }))
    }

    async fn claim(&self, scope: OwnedScope, node: NodeId) -> Result<ClaimOutcome, RegistryError> {
        let mut s = self.state.lock().unwrap();
        if let Some(&(by, epoch)) = s.owners.get(&scope) {
            return Ok(ClaimOutcome::Held { by, epoch });
        }
        // Fence: always bump past the highest epoch this scope ever held.
        let epoch = s.max_epoch.get(&scope).copied().unwrap_or(0) + 1;
        s.owners.insert(scope, (node, epoch));
        s.max_epoch.insert(scope, epoch);
        Ok(ClaimOutcome::Claimed { epoch })
    }

    async fn release(&self, scope: OwnedScope, node: NodeId) -> Result<(), RegistryError> {
        let mut s = self.state.lock().unwrap();
        if s.owners.get(&scope).map(|&(o, _)| o) == Some(node) {
            s.owners.remove(&scope);
        }
        Ok(())
    }

    async fn set_standby(&self, scope: OwnedScope, node: NodeId) -> Result<(), RegistryError> {
        let mut s = self.state.lock().unwrap();
        let list = s.standbys.entry(scope).or_default();
        if !list.contains(&node.0) {
            list.push(node.0);
        }
        Ok(())
    }

    async fn standbys(&self, scope: OwnedScope) -> Result<Vec<NodeId>, RegistryError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .standbys
            .get(&scope)
            .map(|v| v.iter().map(|&n| NodeId(n)).collect())
            .unwrap_or_default())
    }

    async fn can_rehome(&self, scope: OwnedScope) -> Result<bool, RegistryError> {
        Ok(!self.state.lock().unwrap().rehoming_disabled.contains(&scope))
    }

    async fn set_rehoming(&self, scope: OwnedScope, enabled: bool) -> Result<(), RegistryError> {
        let mut s = self.state.lock().unwrap();
        if enabled {
            s.rehoming_disabled.remove(&scope);
        } else {
            s.rehoming_disabled.insert(scope);
        }
        Ok(())
    }

    async fn renew(&self, _node: NodeId) -> Result<(), RegistryError> {
        Ok(()) // no lease expiry in the in-memory model
    }

    async fn publish_key(&self, node: NodeId, public_hex: &str) -> Result<(), RegistryError> {
        // First-write-wins: a node's signing key is the anchor every authorization
        // decision trusts, so once published it must not be silently overwritten
        // with a different key. Re-publishing the *same* key (on restart) is a no-op.
        let mut s = self.state.lock().unwrap();
        if let Some(existing) = s.keys.get(&node.0) {
            return if existing == public_hex {
                Ok(())
            } else {
                Err(RegistryError(
                    "node key already published; refusing to overwrite it (first-write-wins)".into(),
                ))
            };
        }
        s.keys.insert(node.0, public_hex.to_string());
        Ok(())
    }

    async fn public_key(&self, node: NodeId) -> Result<Option<NodePublicKey>, RegistryError> {
        let hex = self.state.lock().unwrap().keys.get(&node.0).cloned();
        match hex {
            None => Ok(None),
            Some(h) => NodePublicKey::from_hex(node, &h)
                .map(Some)
                .map_err(|e| RegistryError(e.to_string())),
        }
    }

    async fn report_load(&self, node: NodeId, load: NodeLoad) -> Result<(), RegistryError> {
        self.state.lock().unwrap().loads.insert(node.0, load);
        Ok(())
    }

    async fn live_nodes(&self) -> Result<Vec<NodeStatus>, RegistryError> {
        let s = self.state.lock().unwrap();
        Ok(s.loads
            .iter()
            .map(|(&n, &load)| NodeStatus {
                node: NodeId(n),
                load,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_keypair::NodeKeypair;

    async fn owned(kp: &NodeKeypair, scope: OwnedScope) -> (InMemoryRegistry, u64) {
        let reg = InMemoryRegistry::new();
        reg.publish_key(kp.node(), &kp.public().to_hex()).await.unwrap();
        let ClaimOutcome::Claimed { epoch } = reg.claim(scope, kp.node()).await.unwrap() else {
            panic!("first claim must succeed");
        };
        (reg, epoch)
    }

    #[tokio::test]
    async fn a_second_claim_while_owned_does_not_take_ownership() {
        let a = NodeKeypair::generate(NodeId(1));
        let (reg, epoch) = owned(&a, OwnedScope::Server(7)).await;
        let outcome = reg.claim(OwnedScope::Server(7), NodeId(2)).await.unwrap();
        assert_eq!(outcome, ClaimOutcome::Held { by: a.node(), epoch });
    }

    #[tokio::test]
    async fn reclaiming_after_release_bumps_the_epoch_and_fences_the_old_owner() {
        let a = NodeKeypair::generate(NodeId(1));
        let (reg, epoch1) = owned(&a, OwnedScope::Server(7)).await;
        reg.release(OwnedScope::Server(7), NodeId(1)).await.unwrap();
        let ClaimOutcome::Claimed { epoch: epoch2 } =
            reg.claim(OwnedScope::Server(7), NodeId(2)).await.unwrap()
        else {
            panic!("re-claim of an unowned scope must succeed");
        };
        assert!(epoch2 > epoch1, "a re-claim always bumps the epoch (fencing)");
        // The old owner returning cannot reclaim — a live node holds it.
        assert_eq!(
            reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap(),
            ClaimOutcome::Held { by: NodeId(2), epoch: epoch2 }
        );
    }

    #[tokio::test]
    async fn a_server_and_a_user_home_with_the_same_id_own_independently() {
        // The two-axis guarantee: numerically-equal scopes of different kinds do
        // not interfere. Server(7) owned by node 1, UserHome(7) free for node 2.
        let a = NodeKeypair::generate(NodeId(1));
        let (reg, _) = owned(&a, OwnedScope::Server(7)).await;
        assert!(reg.owner_of(OwnedScope::UserHome(7)).await.unwrap().is_none());
        assert!(matches!(
            reg.claim(OwnedScope::UserHome(7), NodeId(2)).await.unwrap(),
            ClaimOutcome::Claimed { .. }
        ));
        assert_eq!(
            reg.owner_of(OwnedScope::Server(7)).await.unwrap().unwrap().owner,
            NodeId(1)
        );
    }

    #[tokio::test]
    async fn rehoming_is_allowed_by_default_and_toggles() {
        let reg = InMemoryRegistry::new();
        let s = OwnedScope::Server(7);
        assert!(reg.can_rehome(s).await.unwrap(), "rehoming is on by default");
        reg.set_rehoming(s, false).await.unwrap(); // citizens opt out
        assert!(!reg.can_rehome(s).await.unwrap());
        // The opt-out is per typed scope — a same-numbered user-home is unaffected.
        assert!(reg.can_rehome(OwnedScope::UserHome(7)).await.unwrap());
        reg.set_rehoming(s, true).await.unwrap(); // and can be re-enabled
        assert!(reg.can_rehome(s).await.unwrap());
    }

    #[tokio::test]
    async fn publishing_a_different_key_for_a_node_is_refused() {
        let reg = InMemoryRegistry::new();
        let a = NodeKeypair::generate(NodeId(1));
        reg.publish_key(NodeId(1), &a.public().to_hex()).await.unwrap();
        // Same key again: idempotent.
        reg.publish_key(NodeId(1), &a.public().to_hex()).await.unwrap();
        // A different key for the same node id: refused (first-write-wins).
        let impostor = NodeKeypair::generate(NodeId(1));
        assert!(reg.publish_key(NodeId(1), &impostor.public().to_hex()).await.is_err());
    }
}
