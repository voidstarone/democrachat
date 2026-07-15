//! A member's membership in a custom role.

use serde::{Deserialize, Serialize};

use crate::{RoleId, ServerId, UserId};

/// Records that `user` holds `role` in `server`. Created and removed **only** by
/// ballot ([`crate::ProposalKind::AssignRole`] /
/// [`crate::ProposalKind::UnassignRole`]) — there is no self-serve or admin path,
/// keeping role membership a collective decision like everything else here.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub server_id: ServerId,
    pub role_id: RoleId,
    pub user: UserId,
}

impl RoleAssignment {
    pub fn new(server_id: ServerId, role_id: RoleId, user: UserId) -> Self {
        Self {
            server_id,
            role_id,
            user,
        }
    }
}
