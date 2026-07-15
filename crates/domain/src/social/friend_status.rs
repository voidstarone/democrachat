//! The state of a friendship request.

use serde::{Deserialize, Serialize};

/// Where a [`Friendship`](crate::Friendship) sits in its lifecycle: a request has
/// been sent and is awaiting the other party, or it has been accepted and the two
/// are mutual friends. There is no "declined" state — a declined request is simply
/// removed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FriendStatus {
    /// `requester` has asked; `addressee` has not yet answered.
    Pending,
    /// Both parties are friends.
    Accepted,
}
