//! Roles: mentionable groups of members.
//!
//! Two flavours, both **derived** rather than assigned. [`StandingRole`]s
//! (`@everyone`, `@members`, `@citizens`) follow a member's
//! [`Tier`](crate::Tier); [`Role`]s are custom, server-defined groups a member
//! holds automatically the moment they meet the role's [`RoleCriteria`]. No one is
//! ever *assigned* to either, so neither can be a backdoor to power: they are
//! addressing groups, nothing more, keeping the franchise criteria-only.
pub mod normalize_role_name;
pub mod role;
pub mod role_criteria;
pub mod role_color;
pub mod role_color_vote;
pub mod standing_role;
pub mod tally_role_color;
