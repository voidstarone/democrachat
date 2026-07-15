//! Roles: mentionable groups of members.
//!
//! Two flavours. [`StandingRole`]s (`@everyone`, `@members`, `@citizens`) are
//! **derived** from a member's [`Tier`](crate::Tier) and exist on every server —
//! no one is assigned to them, so they can't gate power. [`Role`]s are custom,
//! server-defined groups created and populated **only by ballot**
//! ([`RoleAssignment`] records the membership). Neither kind carries any vote or
//! permission: they are addressing groups, nothing more, keeping the franchise
//! criteria-only.
pub mod normalize_role_name;
pub mod role;
pub mod role_assignment;
pub mod role_color;
pub mod role_color_vote;
pub mod standing_role;
pub mod tally_role_color;
