//! Fault-injection resiliency tests: what happens when the persistence layer
//! itself fails mid-operation — the "the database is down" case that only became
//! representable once the store ports were made fallible (`Result<_, StoreError>`).
//!
//! A `FailingMembershipStore` decorates the real in-memory store and, once *armed*,
//! returns `StoreError::Unavailable` from every membership call — standing in for a
//! Postgres node that drops out after the service has already begun a request. The
//! tests assert the service surfaces that as a typed error (never a panic), and that
//! disarming restores normal operation.
//!
//! This is the harness the production Postgres adapter's outage/timeout behaviour
//! will be exercised through; today it runs against the memory store so the contract
//! is pinned before the backend exists.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{
    CapAdmission, EnfranchiseError, EnfranchiseOutcome, MembershipStore, Services, StoreError,
    VoteError,
};
use domain::{FranchiseCriteria, Membership, ProposalKind, ServerId, Timestamp, UserId};

const DAY: i64 = 86_400;

/// Wraps the real membership store; while `armed`, every call fails as if the
/// backend were unreachable. Reads and writes both fail, mirroring a node that
/// drops out — the service must cope with a failure at any step.
struct FailingMembershipStore {
    inner: Arc<MemoryStore>,
    armed: AtomicBool,
}

impl FailingMembershipStore {
    fn new(inner: Arc<MemoryStore>) -> Self {
        Self { inner, armed: AtomicBool::new(false) }
    }
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
    fn disarm(&self) {
        self.armed.store(false, Ordering::SeqCst);
    }
    fn guard(&self) -> Result<(), StoreError> {
        if self.armed.load(Ordering::SeqCst) {
            Err(StoreError::Unavailable("membership node down".into()))
        } else {
            Ok(())
        }
    }
}

#[async_trait::async_trait]
impl MembershipStore for FailingMembershipStore {
    async fn upsert(&self, membership: Membership) -> Result<(), StoreError> {
        self.guard()?;
        self.inner.upsert(membership).await
    }
    async fn get(&self, user: UserId, server: ServerId) -> Result<Option<Membership>, StoreError> {
        self.guard()?;
        self.inner.get(user, server).await
    }
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Membership>, StoreError> {
        self.guard()?;
        self.inner.list_for_server(server).await
    }
    async fn citizen_count(&self, server: ServerId) -> Result<u64, StoreError> {
        self.guard()?;
        self.inner.citizen_count(server).await
    }
    async fn admitted_since(&self, server: ServerId, since: Timestamp) -> Result<u64, StoreError> {
        self.guard()?;
        self.inner.admitted_since(server, since).await
    }
    async fn admit_within_cap(
        &self,
        admitted: Membership,
        window_start: Timestamp,
        slots_open: &(dyn Fn(u64, u64) -> u64 + Send + Sync),
    ) -> Result<CapAdmission, StoreError> {
        self.guard()?;
        self.inner.admit_within_cap(admitted, window_start, slots_open).await
    }
}

struct Harness {
    services: Services,
    failing: Arc<FailingMembershipStore>,
    clock: Arc<FixedClock>,
    store: Arc<MemoryStore>,
}

/// Stand up services whose membership store is the failing decorator (disarmed),
/// over an otherwise-real in-memory backend.
fn harness(now_secs: i64) -> Harness {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(now_secs)));
    let failing = Arc::new(FailingMembershipStore::new(store.clone()));
    let mut stores = store.as_stores();
    stores.memberships = failing.clone();
    Harness { services: Services::new(clock.clone(), stores), failing, clock, store }
}

/// Open the franchise criteria so a fresh member is Layer-1 eligible — isolating the
/// store failure from any domain-rule rejection.
async fn open_criteria(h: &Harness, slug: &str) {
    use app::ServerStore;
    let mut s = h.store.find_by_slug(slug).await.unwrap().unwrap();
    s.criteria = FranchiseCriteria { min_account_age_days: 0, min_membership_days: 0, min_contribution: 0 };
    h.store.update_server(s).await.unwrap();
}

/// A membership write that fails mid-enfranchise surfaces as a typed
/// `EnfranchiseError::Store` — the operation is refused cleanly, not with a panic.
#[tokio::test]
async fn an_enfranchise_surfaces_a_store_outage() {
    let h = harness(1_000 * DAY);
    h.services.register_account("boss").await.unwrap();
    h.services.found_server("boss", "Town").await.unwrap();
    open_criteria(&h, "town").await;
    h.services.register_account("newbie").await.unwrap();
    h.services.join_server("newbie", "town").await.unwrap();
    h.clock.set(Timestamp(1_040 * DAY));

    // The backend drops out; the very next enfranchise attempt must fail loudly-but-cleanly.
    h.failing.arm();
    match h.services.try_enfranchise("newbie", "town").await {
        Err(EnfranchiseError::Store(StoreError::Unavailable(_))) => {}
        other => panic!("expected a store-outage error, got {other:?}"),
    }
}

/// The outage is transient: once the backend recovers (disarm), the same operation
/// succeeds — the failure left no poisoned state behind.
#[tokio::test]
async fn recovery_after_an_outage_lets_the_operation_succeed() {
    let h = harness(1_000 * DAY);
    h.services.register_account("boss").await.unwrap();
    h.services.found_server("boss", "Town").await.unwrap();
    open_criteria(&h, "town").await;
    h.services.register_account("newbie").await.unwrap();
    h.services.join_server("newbie", "town").await.unwrap();
    h.clock.set(Timestamp(1_040 * DAY));

    h.failing.arm();
    assert!(h.services.try_enfranchise("newbie", "town").await.is_err(), "outage refuses the write");
    h.failing.disarm();
    // Backend restored: the member is admitted (open criteria + a lone-server floor).
    assert!(
        matches!(h.services.try_enfranchise("newbie", "town").await, Ok(EnfranchiseOutcome::Admitted)),
        "the operation succeeds once the store recovers",
    );
}

/// A read failure on a command path surfaces too: casting a vote must read the
/// caller's membership, and an outage there is reported as `VoteError::Store`
/// rather than being mistaken for "not a citizen".
#[tokio::test]
async fn a_read_outage_is_not_mistaken_for_a_domain_rejection() {
    let h = harness(1_000 * DAY);
    h.services.register_account("boss").await.unwrap();
    h.services.found_server("boss", "Town").await.unwrap();
    let p = h
        .services
        .governance()
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .await
        .unwrap();

    h.failing.arm();
    match h.services.governance().cast_vote("boss", p.id.0, true).await {
        Err(VoteError::Store(StoreError::Unavailable(_))) => {}
        other => panic!("a store read outage must surface as Store, not a domain no: {other:?}"),
    }
}
