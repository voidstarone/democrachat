//! Read-model for a server roster: a member with the flags a moderation UI needs.

use domain::Tier;

/// A member of a server, with the flags a roster/moderation UI needs.
pub struct MemberView {
    pub handle: String,
    pub tier: Tier,
    pub is_sanctioned: bool,
    pub is_muted: bool,
    pub is_police: bool,
}
