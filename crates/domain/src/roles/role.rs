//! A custom, server-defined mention/notification group.

use serde::{Deserialize, Serialize};

use crate::{RoleCriteria, RoleId, ServerId, Timestamp};

/// The well-known name of the **moderator** role: the one role a member may
/// decline (`Membership::has_declined_moderator`) even when they meet its
/// criteria, since moderating is a duty, not just a label.
pub const MODERATOR_ROLE_NAME: &str = "moderator";

/// A custom role: a named group whose membership is **earned by condition**, not
/// assigned. A server creates the role and sets its [`RoleCriteria`] by ballot
/// ([`crate::ProposalKind::CreateRole`]); thereafter every member who meets the
/// criteria holds it automatically, with no vote, request, or admin action — and
/// loses it automatically if they fall below.
///
/// A role is franchise-decoupled by construction: it carries no permissions and
/// no vote weight, and holding one has no bearing on
/// [`evaluate_eligibility`](crate::evaluate_eligibility). It exists purely so a
/// message can address a group of members at once. Contrast the built-in
/// [`StandingRole`](crate::StandingRole)s, which follow tier alone.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Role {
    pub id: RoleId,
    pub server_id: ServerId,
    /// The mention token, without the leading `@` (normalized: lowercase, no spaces).
    pub name: String,
    /// The conditions a member must clear to hold this role. Defaults (via
    /// `#[serde(default)]`) to the empty criteria — which admits every member — so
    /// pre-criteria datasets load as "everyone" rather than failing to decode.
    #[serde(default)]
    pub criteria: RoleCriteria,
    pub created_at: Timestamp,
}

impl Role {
    pub fn new(
        id: RoleId,
        server_id: ServerId,
        name: impl Into<String>,
        criteria: RoleCriteria,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            server_id,
            name: name.into(),
            criteria,
            created_at,
        }
    }

    /// Whether this is the **moderator** role — the sole role a member may opt out
    /// of even while qualified (see [`MODERATOR_ROLE_NAME`]).
    pub fn is_moderator(&self) -> bool {
        self.name == MODERATOR_ROLE_NAME
    }
}
