//! Read-model for the identity popover: the roles a member holds on a server.

use domain::Tier;

use crate::RoleColorView;

/// The roles a member holds on a server: the built-in standing roles their tier
/// admits, plus every custom role they've been assigned.
pub struct UserRoles {
    pub handle: String,
    pub tier: Tier,
    /// Standing-role names the member holds (`everyone`, and `members` or `citizens`).
    pub standing: Vec<String>,
    pub roles: Vec<RoleColorView>,
}
