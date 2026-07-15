//! Read-model for the identity popover: the roles a member holds, and each custom
//! role's voted-on colour.

use domain::Tier;

/// One custom role a member holds, with its current (plurality) colour and the
/// viewer's own colour vote — enough for the popover to show a swatch and offer a
/// re-vote control.
pub struct RoleColorView {
    pub id: u64,
    pub name: String,
    /// The winning colour among franchised citizens (`#rrggbb`), if any voted.
    pub color: Option<String>,
    /// The viewing citizen's own colour vote, if any.
    pub my_color: Option<String>,
}

/// The roles a member holds on a server: the built-in standing roles their tier
/// admits, plus every custom role they've been assigned.
pub struct UserRoles {
    pub handle: String,
    pub tier: Tier,
    /// Standing-role names the member holds (`everyone`, and `members` or `citizens`).
    pub standing: Vec<String>,
    pub roles: Vec<RoleColorView>,
}
