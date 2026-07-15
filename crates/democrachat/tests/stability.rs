//! Stability / resiliency tests: the platform under stress and under partial
//! failure, as opposed to the deliberate attacks in `red_team.rs`.
//!
//! Two families live here:
//!
//! * **Concurrency** — many threads sharing one store (an `Arc<MemoryStore>` behind
//!   a single mutex, the same shape a connection pool over one Postgres will take).
//!   Service methods are read-modify-write across several store calls, so there is a
//!   real interleaving window. These tests pin the invariants that must survive *any*
//!   interleaving: an action applies at most once, totals stay internally
//!   consistent, and nothing panics or deadlocks. They deliberately do **not** assert
//!   an exact admission count — the rate cap's precise cut-off is racy by
//!   construction and is covered sequentially in `red_team.rs`.
//!
//! * **Degraded reads** — a referenced row is simply absent (a not-yet-replicated
//!   write, a deleted entity, a bad id from a stale client). The store ports are
//!   infallible by design, so "the database is unreachable" surfaces here as a read
//!   returning `None`/empty rather than an error. Every such path must degrade to a
//!   typed error or an empty result — never a panic.
//!
//! NOTE: a genuine Postgres *outage* (connection refused, statement timeout, a
//! deadlock abort) cannot be exercised until (a) the Postgres adapter exists and
//! (b) the store ports return `Result`. Today they return `Option`/`Vec`/`()`, so
//! an outage would panic or block rather than degrade. Making the ports fallible is
//! the prerequisite for testing true fault injection.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{EnfranchiseOutcome, MembershipStore, ServerStore, Services, VoteError};
use domain::{FranchiseCriteria, ProposalKind, Timestamp, Tier};

const DAY: i64 = 86_400;

struct Fixture {
    services: Services,
    store: Arc<MemoryStore>,
    clock: Arc<FixedClock>,
}

fn fixture(now_secs: i64) -> Fixture {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(now_secs)));
    let services = Services::new(clock.clone(), store.as_stores());
    Fixture { services, store, clock }
}

async fn found(f: &Fixture, founder: &str, name: &str) {
    f.services.register_account(founder).await.unwrap();
    f.services.found_server(founder, name).await.unwrap();
}

async fn register_and_join(f: &Fixture, handle: &str, slug: &str) {
    f.services.register_account(handle).await.unwrap();
    f.services.join_server(handle, slug).await.unwrap();
}

async fn citizen_count(f: &Fixture, slug: &str) -> u64 {
    let s = f.store.find_by_slug(slug).await.unwrap().unwrap();
    f.store.citizen_count(s.id).await.unwrap()
}

/// Open the constitution wide (every criterion zero) so every fresh member is
/// Layer-1 eligible — isolating Layer 2 (the rate cap) and the store under load.
async fn open_criteria(f: &Fixture, slug: &str) {
    let mut s = f.store.find_by_slug(slug).await.unwrap().unwrap();
    s.criteria = FranchiseCriteria { min_account_age_days: 0, min_membership_days: 0, min_contribution: 0 };
    f.store.update_server(s).await.unwrap();
}

// ─────────────────────────────────────────────────────────────────────────────
// A. Concurrency — an action applies at most once, whatever the interleaving
// ─────────────────────────────────────────────────────────────────────────────

/// A thundering herd all enfranchising the *same* eligible member seats them
/// exactly once: the membership is one row keyed by (user, server), so racing
/// writes converge on a single citizen — never a duplicate, never a torn state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_double_enfranchise_seats_one_citizen() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    open_criteria(&f, "town").await;
    register_and_join(&f, "target", "town").await;

    let services = Arc::new(f.services.clone());
    let mut tasks = Vec::new();
    for _ in 0..32 {
        let s = services.clone();
        tasks.push(tokio::spawn(async move {
            // Either Admitted or AlreadyCitizen — never a panic, never an Err path.
            let _ = s.try_enfranchise("target", "town").await;
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }

    assert!(f.services.member_tier("target", "town").await == Some(Tier::Citizen));
    assert_eq!(citizen_count(&f, "town").await, 2, "founder + target, never a phantom third");
}

/// Hammering `cast_vote` from many threads with the same citizen on the same
/// proposal collapses to a single aye — the vote is upserted on (proposal, voter),
/// so concurrency cannot stuff the box any more than a sequential loop could.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_vote_stuffing_collapses_to_one() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    let p = f
        .services.governance()
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .await
        .unwrap();

    let services = Arc::new(f.services.clone());
    let pid = p.id.0;
    let mut tasks = Vec::new();
    for i in 0..32 {
        let s = services.clone();
        tasks.push(tokio::spawn(async move {
            let _ = s.governance().cast_vote("boss", pid, i % 2 == 0).await;
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }

    let (ayes, nays) = f.services.governance().proposal_head_counts(pid).await;
    assert_eq!(ayes + nays, 1, "one citizen carries exactly one vote under any race");
}

/// Many threads enfranchising *distinct* eligible members keep the roll internally
/// consistent: however the rate cap's cut-off falls out under the race, the citizen
/// count equals the founder plus exactly the number of `Admitted` outcomes — no
/// lost update, no double count. (The exact admitted number is racy and unasserted.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_distinct_enfranchise_keeps_the_roll_consistent() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    open_criteria(&f, "town").await;
    for i in 0..100 {
        register_and_join(&f, &format!("m{i}"), "town").await;
    }
    f.clock.set(Timestamp(1_040 * DAY)); // age the founding out of the cap window

    let services = Arc::new(f.services.clone());
    let admitted = Arc::new(AtomicU64::new(0));
    let mut tasks = Vec::new();
    for t in 0..8 {
        let s = services.clone();
        let admitted = admitted.clone();
        tasks.push(tokio::spawn(async move {
            for i in (t..100).step_by(8) {
                if let Ok(EnfranchiseOutcome::Admitted) = s.try_enfranchise(&format!("m{i}"), "town").await {
                    admitted.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }

    let admitted = admitted.load(Ordering::Relaxed);
    assert_eq!(
        citizen_count(&f, "town").await,
        1 + admitted,
        "every Admitted is reflected in the roll and nothing else is",
    );
}

/// Concurrent registration and joins of distinct accounts neither deadlock nor lose
/// a write: after the herd, every member is present and the electorate is untouched
/// (a join is never a franchise).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_registration_and_joins_do_not_deadlock() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;

    let services = Arc::new(f.services.clone());
    let mut tasks = Vec::new();
    for t in 0..8 {
        let s = services.clone();
        tasks.push(tokio::spawn(async move {
            for i in (t..64).step_by(8) {
                let h = format!("u{i}");
                s.register_account(&h).await.unwrap();
                s.join_server(&h, "town").await.unwrap();
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }

    // Founder + 64 members all landed; not one of them was enfranchised by joining.
    assert_eq!(f.services.roles().member_handles("town").await.len(), 65);
    assert_eq!(citizen_count(&f, "town").await, 1, "joining never seats a citizen, even under load");
}

// ─────────────────────────────────────────────────────────────────────────────
// B. Degraded reads — a missing row degrades to a typed error or empty, not a panic
// ─────────────────────────────────────────────────────────────────────────────

/// A vote for a proposal that isn't there (deleted, or a stale/garbage id from a
/// client) is a clean `NoSuchProposal`, not a panic — a partial/inconsistent read
/// degrades gracefully.
#[tokio::test]
async fn a_vote_on_a_missing_proposal_is_a_clean_error() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    assert_eq!(
        f.services.governance().cast_vote("boss", 9_999_999, true).await,
        Err(VoteError::NoSuchProposal(9_999_999)),
    );
}

/// Read paths for entities that don't exist return `None`/empty rather than
/// panicking — the shape a not-yet-replicated write or a deleted row takes when the
/// ports are infallible.
#[tokio::test]
async fn reads_for_missing_entities_degrade_to_empty() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;

    // Unknown user / unknown server on the membership read paths.
    assert_eq!(f.services.member_tier("ghost", "town").await, None);
    assert_eq!(f.services.member_contribution("boss", "no-such-server").await, None);
    assert_eq!(f.services.member_tier("boss", "no-such-server").await, None);

    // Listing surfaces for an unknown server are empty, not a panic.
    assert!(f.services.governance().list_proposals("no-such-server").await.is_empty());
    assert!(f.services.governance().list_rules("no-such-server").await.is_empty());
    assert!(f.services.roles().list_roles("no-such-server").await.is_empty());
    assert!(f.services.roles().member_handles("no-such-server").await.is_empty());
    assert!(f.services.emoji().ranked_emojis("no-such-server", "boss").await.is_empty());
    assert!(f.services.mute().list_members("no-such-server").await.is_empty());

    // A dangling server id resolves to nothing rather than dereferencing a ghost.
    assert_eq!(f.services.server_slug(domain::ServerId(9_999)).await, None);
}

/// Sweeping the due ballots for a server that isn't there is a no-op, not a panic —
/// a background resolver pointed at a vanished server degrades quietly.
#[tokio::test]
async fn resolving_an_unknown_server_is_a_noop() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    f.services.governance().resolve_due("no-such-server").await; // must not panic
    assert!(f.services.governance().list_proposals("no-such-server").await.is_empty());
}
