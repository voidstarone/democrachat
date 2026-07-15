//! Persistence for custom roles and their memberships.

use domain::{Role, RoleAssignment, RoleId, ServerId, UserId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for custom roles and their assignments. Roles and their membership
/// are one concern — deleting a role purges its assignments — so a single port
/// owns both.
#[async_trait]
pub trait RoleStore: Send + Sync {
    async fn next_role_id(&self) -> Result<RoleId, StoreError>;
    async fn insert_role(&self, role: Role) -> Result<(), StoreError>;
    async fn get_role(&self, id: RoleId) -> Result<Option<Role>, StoreError>;
    /// A role by (normalized) name within a server.
    async fn find_role(&self, server: ServerId, name: &str) -> Result<Option<Role>, StoreError>;
    /// Delete a role and every assignment to it; returns whether it existed.
    async fn remove_role(&self, id: RoleId) -> Result<bool, StoreError>;
    /// Every custom role in a server, in id order.
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Role>, StoreError>;

    /// Record a role assignment; returns `false` if it already existed.
    async fn assign(&self, assignment: RoleAssignment) -> Result<bool, StoreError>;
    /// Remove a role assignment; returns `false` if it wasn't present.
    async fn unassign(&self, role: RoleId, user: UserId) -> Result<bool, StoreError>;
    /// The users holding a role.
    async fn holders(&self, role: RoleId) -> Result<Vec<UserId>, StoreError>;
}
