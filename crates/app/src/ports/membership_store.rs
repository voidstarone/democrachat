//! Persistence for per-server membership records.

use domain::{ServerId, Membership, Timestamp, UserId};
use crate::StoreError;
use async_trait::async_trait;

/// The outcome of an atomic, rate-cap-checked admission
/// ([`MembershipStore::admit_within_cap`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapAdmission {
    /// A slot was open under the lock; the member is now a citizen.
    Admitted,
    /// No slot was free; nothing was written. Carries the trailing-window admission
    /// count so the caller can report how full the cap is.
    RateCapped { admitted_this_window: u64 },
}

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

    /// Atomically apply Layer 2 of enfranchisement. Under a per-server lock, count
    /// the electorate and the admissions since `window_start`, ask `slots_open`
    /// whether a rate-cap slot is free, and — only if so — commit `admitted` (a
    /// membership the caller has already promoted to citizen after passing Layer 1).
    ///
    /// This closes the check-then-write TOCTOU in `try_enfranchise`: two concurrent
    /// admissions on the same server can no longer both observe the final slot and
    /// overrun the cap. `slots_open(citizen_count, admitted_this_window)` carries the
    /// domain rule (`enfranchisement_slots`) so it never leaks into the adapter.
    async fn admit_within_cap(
        &self,
        admitted: Membership,
        window_start: Timestamp,
        slots_open: &(dyn Fn(u64, u64) -> u64 + Send + Sync),
    ) -> Result<CapAdmission, StoreError>;
}
