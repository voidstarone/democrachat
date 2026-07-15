//! Persistence for custom roles and their memberships.

use domain::{Role, RoleAssignment, RoleId, ServerId, UserId};

/// Persistence for custom roles and their assignments. Roles and their membership
/// are one concern — deleting a role purges its assignments — so a single port
/// owns both.
pub trait RoleStore: Send + Sync {
    fn next_role_id(&self) -> RoleId;
    fn insert_role(&self, role: Role);
    fn get_role(&self, id: RoleId) -> Option<Role>;
    /// A role by (normalized) name within a server.
    fn find_role(&self, server: ServerId, name: &str) -> Option<Role>;
    /// Delete a role and every assignment to it; returns whether it existed.
    fn remove_role(&self, id: RoleId) -> bool;
    /// Every custom role in a server, in id order.
    fn list_for_server(&self, server: ServerId) -> Vec<Role>;

    /// Record a role assignment; returns `false` if it already existed.
    fn assign(&self, assignment: RoleAssignment) -> bool;
    /// Remove a role assignment; returns `false` if it wasn't present.
    fn unassign(&self, role: RoleId, user: UserId) -> bool;
    /// The users holding a role.
    fn holders(&self, role: RoleId) -> Vec<UserId>;
}
