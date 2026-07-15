//! A single message in a proposal's deliberation thread.

use serde::{Deserialize, Serialize};

use crate::{Timestamp, UserId};

/// One citizen's contribution to the debate on a proposal. The thread lives on
/// the [`Proposal`](crate::Proposal) itself — deliberation is governance data, not
/// ordinary chat, and only franchised citizens may post (the app enforces that;
/// the entity is inert).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct DiscussionPost {
    pub author: UserId,
    pub body: String,
    pub at: Timestamp,
}

impl DiscussionPost {
    pub fn new(author: UserId, body: impl Into<String>, at: Timestamp) -> Self {
        Self { author, body: body.into(), at }
    }
}
