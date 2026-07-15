//! The control plane: who owns each scope, under which epoch, and which nodes are
//! live standbys for failover.
//!
//! Ownership is keyed by [`OwnedScope`](owned_scope::OwnedScope) — a server or a
//! user-home — so the two sharding axes never collide even when a `ServerId` and a
//! `UserId` share a number. Every ownership hand-off bumps a monotonic **epoch**,
//! which signed events carry so a returning stale owner is fenced (see
//! `docs/federation.md` §6). The trait
//! ([`OwnershipRegistry`](ownership_registry::OwnershipRegistry)) is what etcd
//! implements in M4; [`InMemoryRegistry`](in_memory_registry::InMemoryRegistry)
//! is the single-node / test implementation with the same fencing semantics.

pub mod claim_outcome;
pub mod in_memory_registry;
pub mod node_load;
pub mod node_status;
pub mod owned_scope;
#[allow(clippy::module_inception)]
pub mod ownership;
pub mod ownership_registry;
pub mod registry_error;

pub use claim_outcome::ClaimOutcome;
pub use in_memory_registry::InMemoryRegistry;
pub use node_load::NodeLoad;
pub use node_status::NodeStatus;
pub use owned_scope::OwnedScope;
pub use ownership::Ownership;
pub use ownership_registry::OwnershipRegistry;
pub use registry_error::RegistryError;
