//! Built-in roles derived from a member's tier, rather than stored.

use serde::{Deserialize, Serialize};

use crate::Tier;

/// A built-in, mentionable group whose membership follows directly from a
/// member's [`Tier`] — no one is ever *assigned* to one, so it can never be a
/// backdoor to power. Every server has all three, always mentionable.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum StandingRole {
    /// Everyone who has joined the server (any tier at or above Member).
    Everyone,
    /// Members who have not (yet) been enfranchised.
    Members,
    /// Enfranchised citizens.
    Citizens,
}

impl StandingRole {
    /// All built-in roles, in mention-priority order.
    pub fn all() -> [StandingRole; 3] {
        [StandingRole::Everyone, StandingRole::Members, StandingRole::Citizens]
    }

    /// The mention token for this role, without the leading `@`.
    pub fn name(&self) -> &'static str {
        match self {
            StandingRole::Everyone => "everyone",
            StandingRole::Members => "members",
            StandingRole::Citizens => "citizens",
        }
    }

    /// Resolve a bare token (no `@`) to a built-in role, if it names one.
    pub fn from_token(token: &str) -> Option<StandingRole> {
        Self::all().into_iter().find(|r| r.name() == token)
    }

    /// Whether a member of the given tier is addressed by this role.
    pub fn admits(&self, tier: Tier) -> bool {
        match self {
            StandingRole::Everyone => matches!(tier, Tier::Member | Tier::Citizen),
            StandingRole::Members => matches!(tier, Tier::Member),
            StandingRole::Citizens => matches!(tier, Tier::Citizen),
        }
    }
}
