//! A custom, server-defined mention/notification group.

use serde::{Deserialize, Serialize};

use crate::{RoleId, ServerId, Timestamp};

/// A custom role: a named group a server can `@mention`, created and populated
/// **only** by ballot (see [`crate::ProposalKind::CreateRole`]).
///
/// A role is franchise-decoupled by construction: it carries no permissions and
/// no vote weight, and holding one has no bearing on
/// [`evaluate_eligibility`](crate::evaluate_eligibility). It exists purely so a
/// message can address a group of members at once. Contrast the built-in
/// [`StandingRole`](crate::StandingRole)s, which are derived from tier rather than
/// stored.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Role {
    pub id: RoleId,
    pub server_id: ServerId,
    /// The mention token, without the leading `@` (normalized: lowercase, no spaces).
    pub name: String,
    pub created_at: Timestamp,
}

impl Role {
    pub fn new(id: RoleId, server_id: ServerId, name: impl Into<String>, created_at: Timestamp) -> Self {
        Self {
            id,
            server_id,
            name: name.into(),
            created_at,
        }
    }
}
