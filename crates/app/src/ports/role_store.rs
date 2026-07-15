//! Persistence for custom roles.

use domain::{Role, RoleId, ServerId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for custom roles. Role *membership* is not stored here — it is
/// derived on read by evaluating each member against the role's
/// [`RoleCriteria`](domain::RoleCriteria) (see `RoleService`), so a role has no
/// assignment rows to keep. This port owns only the roles themselves.
#[async_trait]
pub trait RoleStore: Send + Sync {
    async fn next_role_id(&self) -> Result<RoleId, StoreError>;
    async fn insert_role(&self, role: Role) -> Result<(), StoreError>;
    async fn get_role(&self, id: RoleId) -> Result<Option<Role>, StoreError>;
    /// A role by (normalized) name within a server.
    async fn find_role(&self, server: ServerId, name: &str) -> Result<Option<Role>, StoreError>;
    /// Delete a role; returns whether it existed.
    async fn remove_role(&self, id: RoleId) -> Result<bool, StoreError>;
    /// Every custom role in a server, in id order.
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Role>, StoreError>;
}
