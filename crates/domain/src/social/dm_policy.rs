//! Who a user is willing to receive direct messages from.

use serde::{Deserialize, Serialize};

/// A user's direct-message policy: the set of people allowed to open a DM with
/// them. Defaults to [`Everyone`](DmPolicy::Everyone) — DMs are on by default —
/// and a user may tighten it to [`FriendsOnly`](DmPolicy::FriendsOnly).
///
/// A [`Block`](crate::Block) overrides this in either direction: a blocked user
/// can never DM, whatever the policy says.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum DmPolicy {
    /// Anyone (who has not been blocked) may open a DM.
    #[default]
    Everyone,
    /// Only accepted friends may open a DM.
    FriendsOnly,
}
