//! One citizen's vote for a custom role's display colour.

use serde::{Deserialize, Serialize};

use crate::{RoleColor, RoleId, ServerId, UserId};

/// A citizen's chosen colour for a role. One per (role, voter) — re-voting
/// replaces the prior one. Carries `server_id` so a replicated vote is scoped
/// without a lookup (federation).
///
/// **Only currently-franchised citizens' votes count** toward a role's winning
/// colour; the app layer applies that filter when tallying, so a revoked franchise
/// silently stops counting without touching stored votes — exactly like
/// [`EmojiVote`](crate::EmojiVote).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RoleColorVote {
    pub server_id: ServerId,
    pub role_id: RoleId,
    pub voter: UserId,
    pub color: RoleColor,
}

impl RoleColorVote {
    pub fn new(server_id: ServerId, role_id: RoleId, voter: UserId, color: RoleColor) -> Self {
        Self { server_id, role_id, voter, color }
    }
}
