//! A community rule — a line in a server's rulebook, voted in and out.

use serde::{Deserialize, Serialize};

use crate::{ServerId, RuleId, Timestamp};

/// A single rule in a server's rulebook. Rules are added and repealed by a
/// `RuleChange` ballot (60% + quorum), permitted in *every* phase — including
/// Seed — so a founding community can establish conduct from day one. Changing
/// *the rulebook* is normal governance; changing *who votes* (the franchise) is
/// dangerous and stays locked behind constitutional amendment.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    pub server_id: ServerId,
    pub text: String,
    pub added_at: Timestamp,
}

impl Rule {
    pub fn new(id: RuleId, server_id: ServerId, text: impl Into<String>, added_at: Timestamp) -> Self {
        Self {
            id,
            server_id,
            text: text.into(),
            added_at,
        }
    }
}
