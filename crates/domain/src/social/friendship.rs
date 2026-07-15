//! A friendship (or pending friend request) between two users.

use serde::{Deserialize, Serialize};

use crate::{FriendStatus, Timestamp, UserId};

/// A directed friend request that becomes a mutual friendship once accepted.
/// `requester` sent it; `addressee` received it. Direction only matters while
/// [`Pending`](FriendStatus::Pending) — an [`Accepted`](FriendStatus::Accepted)
/// friendship is symmetric.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Friendship {
    pub requester: UserId,
    pub addressee: UserId,
    pub status: FriendStatus,
    pub created_at: Timestamp,
}

impl Friendship {
    /// Open a pending friend request from `requester` to `addressee`.
    pub fn request(requester: UserId, addressee: UserId, created_at: Timestamp) -> Self {
        Self {
            requester,
            addressee,
            status: FriendStatus::Pending,
            created_at,
        }
    }

    /// Accept this request, making the two mutual friends. Idempotent.
    pub fn accept(&mut self) {
        self.status = FriendStatus::Accepted;
    }

    /// Whether the two are accepted, mutual friends.
    pub fn are_friends(&self) -> bool {
        self.status == FriendStatus::Accepted
    }

    /// Whether this record concerns both `a` and `b`, in either role.
    pub fn is_between(&self, a: UserId, b: UserId) -> bool {
        (self.requester == a && self.addressee == b)
            || (self.requester == b && self.addressee == a)
    }
}
