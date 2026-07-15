//! Persistence for per-server membership records.

use domain::{ServerId, Membership, Timestamp, UserId};

/// Persistence for per-server membership records, plus the aggregate queries the
/// enfranchisement rate cap (Layer 2) needs.
pub trait MembershipStore: Send + Sync {
    fn upsert(&self, membership: Membership);
    fn get(&self, user: UserId, server: ServerId) -> Option<Membership>;
    /// Every membership record in a server (used to resolve role mentions to the
    /// members a standing role addresses).
    fn list_for_server(&self, server: ServerId) -> Vec<Membership>;
    /// How many enfranchised citizens the server currently has.
    fn citizen_count(&self, server: ServerId) -> u64;
    /// How many members were admitted to the franchise at or after `since` —
    /// the trailing-window count the rate cap compares against.
    fn admitted_since(&self, server: ServerId, since: Timestamp) -> u64;
}
