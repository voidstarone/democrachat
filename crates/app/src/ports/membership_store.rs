//! Persistence for per-server membership records.

use domain::{ServerId, Membership, Timestamp, UserId};
use crate::StoreError;
use async_trait::async_trait;

/// Persistence for per-server membership records, plus the aggregate queries the
/// enfranchisement rate cap (Layer 2) needs.
#[async_trait]
pub trait MembershipStore: Send + Sync {
    async fn upsert(&self, membership: Membership) -> Result<(), StoreError>;
    async fn get(&self, user: UserId, server: ServerId) -> Result<Option<Membership>, StoreError>;
    /// Every membership record in a server (used to resolve role mentions to the
    /// members a standing role addresses).
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Membership>, StoreError>;
    /// How many enfranchised citizens the server currently has.
    async fn citizen_count(&self, server: ServerId) -> Result<u64, StoreError>;
    /// How many members were admitted to the franchise at or after `since` —
    /// the trailing-window count the rate cap compares against.
    async fn admitted_since(&self, server: ServerId, since: Timestamp) -> Result<u64, StoreError>;
}
