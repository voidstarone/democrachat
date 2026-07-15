//! Citizenship tier within a server.

use serde::{Deserialize, Serialize};

/// A user's tier within a server.
///
/// The **only** path to [`Tier::Citizen`] is meeting the server's franchise
/// criteria (see [`crate::evaluate_eligibility`]). There is deliberately no
/// domain constructor, proposal, role, or admin action that grants it — the
/// franchise cannot be handed to anyone, only earned.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Tier {
    /// Reading only; not joined.
    Guest,
    /// Joined; accruing dwell time and contribution toward the franchise.
    Member,
    /// Enfranchised citizen: may vote on this server's ballots.
    Citizen,
}
