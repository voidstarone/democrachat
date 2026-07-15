//! Persistence for custom roles and their memberships.

use domain::{Role, RoleAssignment, RoleId, ServerId, UserId};
use crate::StoreError;

/// Persistence for custom roles and their assignments. Roles and their membership
/// are one concern — deleting a role purges its assignments — so a single port
/// owns both.
pub trait RoleStore: Send + Sync {
    fn next_role_id(&self) -> Result<RoleId, StoreError>;
    fn insert_role(&self, role: Role) -> Result<(), StoreError>;
    fn get_role(&self, id: RoleId) -> Result<Option<Role>, StoreError>;
    /// A role by (normalized) name within a server.
    fn find_role(&self, server: ServerId, name: &str) -> Result<Option<Role>, StoreError>;
    /// Delete a role and every assignment to it; returns whether it existed.
    fn remove_role(&self, id: RoleId) -> Result<bool, StoreError>;
    /// Every custom role in a server, in id order.
    fn list_for_server(&self, server: ServerId) -> Result<Vec<Role>, StoreError>;

    /// Record a role assignment; returns `false` if it already existed.
    fn assign(&self, assignment: RoleAssignment) -> Result<bool, StoreError>;
    /// Remove a role assignment; returns `false` if it wasn't present.
    fn unassign(&self, role: RoleId, user: UserId) -> Result<bool, StoreError>;
    /// The users holding a role.
    fn holders(&self, role: RoleId) -> Result<Vec<UserId>, StoreError>;
}
