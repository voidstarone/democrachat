//! A node identity in the federated network.

use serde::{Deserialize, Serialize};

/// Identifies one node — one deployment (its own store + app), the source of
/// truth for the servers it hosts and the users it homes.
///
/// Node `0` is the reserved single-box / bootstrap identity: an un-federated
/// deployment keeps minting bare sequences `1, 2, 3, …` exactly as before, so
/// existing data and the JSON store keep working untouched. [`NodeId::default`]
/// is therefore `NodeId(0)`.
/// `Default` is `NodeId(0)` (via `u16::default()`), the reserved single-box
/// identity.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct NodeId(pub u16);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
