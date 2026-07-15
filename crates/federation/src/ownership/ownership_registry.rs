//! The control-plane trait.

use async_trait::async_trait;

use domain::NodeId;

use crate::node_public_key::NodePublicKey;
use crate::ownership::claim_outcome::ClaimOutcome;
use crate::ownership::node_load::NodeLoad;
use crate::ownership::node_status::NodeStatus;
use crate::ownership::owned_scope::OwnedScope;
use crate::ownership::ownership::Ownership;
use crate::ownership::registry_error::RegistryError;

/// The control plane. etcd implements this in production (leases, epoch fencing
/// via compare-and-swap, key distribution, load reporting);
/// [`InMemoryRegistry`](crate::ownership::in_memory_registry::InMemoryRegistry)
/// implements it for a single node and for tests. Ownership is keyed by
/// [`OwnedScope`] so a server and a user-home that share an id never collide.
#[async_trait]
pub trait OwnershipRegistry: Send + Sync {
    /// Current owner + epoch of a scope, or `None` if unowned (never claimed, or
    /// the owner's lease has lapsed).
    async fn owner_of(&self, scope: OwnedScope) -> Result<Option<Ownership>, RegistryError>;

    /// Attempt to take ownership of `scope` for `node`. Succeeds only if the scope
    /// is currently unowned; on success the epoch is **bumped** past the highest it
    /// ever held (fencing any prior owner). If a live node already holds it, returns
    /// [`ClaimOutcome::Held`].
    async fn claim(&self, scope: OwnedScope, node: NodeId) -> Result<ClaimOutcome, RegistryError>;

    /// Gracefully give up ownership of `scope` (planned handoff, or a lease lapse
    /// modelled explicitly in memory). No-op if `node` is not the current owner.
    async fn release(&self, scope: OwnedScope, node: NodeId) -> Result<(), RegistryError>;

    /// Designate `node` as a **standby** (pre-warmed, caught-up replica) for
    /// `scope` — the failover target rehoming promotes.
    async fn set_standby(&self, scope: OwnedScope, node: NodeId) -> Result<(), RegistryError>;

    /// The standbys currently designated for `scope`.
    async fn standbys(&self, scope: OwnedScope) -> Result<Vec<NodeId>, RegistryError>;

    /// Whether `scope` may be **automatically rehomed** onto a standby if its
    /// owner fails. A server whose citizens have voted to disable rehoming returns
    /// `false`, so the [rehoming controller](crate::rehome) leaves it unowned (and
    /// thus down for writes) until its home node returns, rather than migrating it
    /// onto a node the community did not choose. Trades availability for
    /// sovereignty — the community's call. Defaults to `true` (rehoming on).
    ///
    /// This gates only *automatic* failover; an explicit governance-approved
    /// [`claim`](Self::claim) (a planned move) is unaffected.
    async fn can_rehome(&self, scope: OwnedScope) -> Result<bool, RegistryError>;

    /// Set whether `scope` may be automatically rehomed. Called by the governance
    /// layer when a server's citizens change the policy — never by the fleet
    /// itself. `enabled = false` is the opt-out.
    async fn set_rehoming(&self, scope: OwnedScope, enabled: bool) -> Result<(), RegistryError>;

    /// Heartbeat: renew this node's lease so the scopes it owns stay owned.
    async fn renew(&self, node: NodeId) -> Result<(), RegistryError>;

    /// Publish this node's public key (hex) so peers can verify its events.
    /// First-write-wins: the key is the anchor every authorization trusts, so it
    /// is not silently overwritten (re-publishing the same key is idempotent).
    async fn publish_key(&self, node: NodeId, public_hex: &str) -> Result<(), RegistryError>;

    /// Fetch a node's published public key, if any.
    async fn public_key(&self, node: NodeId) -> Result<Option<NodePublicKey>, RegistryError>;

    /// Report this node's current load, for placement decisions.
    async fn report_load(&self, node: NodeId, load: NodeLoad) -> Result<(), RegistryError>;

    /// All currently-live nodes with their last-reported load.
    async fn live_nodes(&self) -> Result<Vec<NodeStatus>, RegistryError>;
}
