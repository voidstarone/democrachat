//! Drives failover from the perspective of one node.

use std::sync::Arc;

use domain::NodeId;

use crate::ownership::claim_outcome::ClaimOutcome;
use crate::ownership::owned_scope::OwnedScope;
use crate::ownership::ownership_registry::OwnershipRegistry;
use crate::ownership::registry_error::RegistryError;

use super::choose_new_owner::choose_new_owner;
use super::choose_new_standby::choose_new_standby;
use super::rehome_outcome::RehomeOutcome;

/// Drives failover from the perspective of one node. Run its [`tick`](Self::tick)
/// on an interval over the scopes this node replicates.
pub struct RehomingController {
    node: NodeId,
    registry: Arc<dyn OwnershipRegistry>,
}

impl RehomingController {
    pub fn new(node: NodeId, registry: Arc<dyn OwnershipRegistry>) -> Self {
        Self { node, registry }
    }

    /// Evaluate every candidate scope once, taking over the ones this node is the
    /// best standby for. A registry error on a single scope drops that scope from
    /// the result (the caller sees a shorter list).
    pub async fn tick(&self, candidates: &[OwnedScope]) -> Vec<RehomeOutcome> {
        let mut out = Vec::new();
        for &scope in candidates {
            if let Ok(o) = self.consider(scope).await {
                out.push(o);
            }
        }
        out
    }

    async fn consider(&self, scope: OwnedScope) -> Result<RehomeOutcome, RegistryError> {
        if self.registry.owner_of(scope).await?.is_some() {
            return Ok(RehomeOutcome::StillOwned { scope });
        }
        // Honour a community's opt-out: a scope with rehoming disabled is left
        // down until its home node returns, never migrated onto another node.
        if !self.registry.can_rehome(scope).await? {
            return Ok(RehomeOutcome::RehomingDisabled { scope });
        }
        let standbys = self.registry.standbys(scope).await?;
        let loads = self.registry.live_nodes().await?;

        let Some(winner) = choose_new_owner(&standbys, &loads) else {
            return Ok(RehomeOutcome::Stranded { scope });
        };
        if winner != self.node {
            return Ok(RehomeOutcome::Yielded { scope, to: winner });
        }

        // We are the best candidate — claim (bumps the epoch, fencing the old owner).
        match self.registry.claim(scope, self.node).await? {
            ClaimOutcome::Claimed { epoch } => {
                // Re-protect the scope with a fresh, quiet standby.
                if let Some(sb) = choose_new_standby(&[self.node], &loads) {
                    let _ = self.registry.set_standby(scope, sb).await;
                }
                Ok(RehomeOutcome::Promoted { scope, epoch })
            }
            // Lost a race to another node between our read and our claim.
            ClaimOutcome::Held { by, .. } => Ok(RehomeOutcome::Yielded { scope, to: by }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ownership::in_memory_registry::InMemoryRegistry;
    use crate::ownership::node_load::NodeLoad;

    #[tokio::test]
    async fn failover_promotes_the_quiet_standby_and_fences_the_old_owner() {
        let reg = Arc::new(InMemoryRegistry::new());
        let s = OwnedScope::Server(7);
        // Node 1 owns Server(7); nodes 2 and 3 are standbys.
        reg.claim(s, NodeId(1)).await.unwrap();
        reg.set_standby(s, NodeId(2)).await.unwrap();
        reg.set_standby(s, NodeId(3)).await.unwrap();
        // Live loads for the standbys (node 1 is about to go down).
        reg.report_load(NodeId(2), NodeLoad { hosted_scopes: 5, requests_per_sec: 10.0 })
            .await
            .unwrap();
        reg.report_load(NodeId(3), NodeLoad { hosted_scopes: 1, requests_per_sec: 2.0 })
            .await
            .unwrap();

        // Node 1 goes down → its lease lapses → unowned.
        reg.release(s, NodeId(1)).await.unwrap();

        // The busy standby (2) yields to the quiet one (3).
        let c2 = RehomingController::new(NodeId(2), reg.clone());
        assert_eq!(c2.tick(&[s]).await, vec![RehomeOutcome::Yielded { scope: s, to: NodeId(3) }]);

        // The quiet standby (3) promotes itself; the epoch bumps past 1.
        let c3 = RehomingController::new(NodeId(3), reg.clone());
        let outcomes = c3.tick(&[s]).await;
        assert!(
            matches!(outcomes[0], RehomeOutcome::Promoted { scope, epoch } if scope == s && epoch > 1),
            "got {outcomes:?}"
        );
        assert_eq!(reg.owner_of(s).await.unwrap().unwrap().owner, NodeId(3));

        // A fresh standby was designated (the quiet remaining node, 2).
        assert!(reg.standbys(s).await.unwrap().contains(&NodeId(2)));

        // The old owner (1) returning is fenced: it cannot reclaim.
        assert!(matches!(
            reg.claim(s, NodeId(1)).await.unwrap(),
            ClaimOutcome::Held { by, .. } if by == NodeId(3)
        ));

        // A now-owned scope is left alone.
        assert_eq!(c3.tick(&[s]).await, vec![RehomeOutcome::StillOwned { scope: s }]);
    }

    #[tokio::test]
    async fn a_scope_with_rehoming_disabled_is_not_migrated_even_with_a_ready_standby() {
        // The citizens' sovereignty choice: a live, ready standby exists, but the
        // community opted out of rehoming, so the scope is deliberately left down.
        let reg = Arc::new(InMemoryRegistry::new());
        let s = OwnedScope::Server(7);
        reg.claim(s, NodeId(1)).await.unwrap();
        reg.set_standby(s, NodeId(2)).await.unwrap();
        reg.report_load(NodeId(2), NodeLoad { hosted_scopes: 0, requests_per_sec: 1.0 })
            .await
            .unwrap();
        reg.set_rehoming(s, false).await.unwrap(); // citizens disable rehoming
        reg.release(s, NodeId(1)).await.unwrap(); // owner goes down

        let c2 = RehomingController::new(NodeId(2), reg.clone());
        assert_eq!(c2.tick(&[s]).await, vec![RehomeOutcome::RehomingDisabled { scope: s }]);
        // Left unowned — not migrated onto node 2.
        assert!(reg.owner_of(s).await.unwrap().is_none());

        // Re-enabling rehoming lets the standby take over as normal.
        reg.set_rehoming(s, true).await.unwrap();
        let outcomes = c2.tick(&[s]).await;
        assert!(matches!(outcomes[0], RehomeOutcome::Promoted { scope, .. } if scope == s));
    }

    #[tokio::test]
    async fn an_unowned_scope_with_no_live_standby_is_stranded() {
        let reg = Arc::new(InMemoryRegistry::new());
        let s = OwnedScope::UserHome(3);
        // Designated standby exists but never reported load → not live.
        reg.set_standby(s, NodeId(9)).await.unwrap();
        let c = RehomingController::new(NodeId(2), reg.clone());
        assert_eq!(c.tick(&[s]).await, vec![RehomeOutcome::Stranded { scope: s }]);
    }
}
