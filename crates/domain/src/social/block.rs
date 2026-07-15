//! A permanent, one-directional block between two users.

use serde::{Deserialize, Serialize};

use crate::{Timestamp, UserId};

/// A permanent block: `blocker` has barred `blocked`. Blocks are never lifted in
/// the domain — there is no unblock — so once recorded a block stands for good.
///
/// A block silences DMs in *both* directions: neither party may DM the other
/// while a block by either of them exists. See [`crate::can_dm`].
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Block {
    pub blocker: UserId,
    pub blocked: UserId,
    pub created_at: Timestamp,
}

impl Block {
    pub fn new(blocker: UserId, blocked: UserId, created_at: Timestamp) -> Self {
        Self {
            blocker,
            blocked,
            created_at,
        }
    }

    /// Whether this block stands between `a` and `b` in either direction.
    pub fn is_between(&self, a: UserId, b: UserId) -> bool {
        (self.blocker == a && self.blocked == b) || (self.blocker == b && self.blocked == a)
    }
}
