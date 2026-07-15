//! Integration tests for server invite codes, visibility, and the vote-to-close
//! invite policy.
//!
//! Rules under test: an invite grants membership (never the franchise); private
//! servers are hidden from the public directory but reachable by code; any member
//! may mint while the policy is `Open`; and citizens can vote the policy `Closed`,
//! after which minting is refused.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{Clock, InviteError, MembershipStore, ServerStore, Services, UserStore};
use domain::{InvitePolicy, ProposalKind, Tier, Timestamp};

const DAY: i64 = 86_400;

struct Fixture {
    services: Services,
    store: Arc<MemoryStore>,
    clock: Arc<FixedClock>,
}

fn fixture() -> Fixture {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(1_000 * DAY)));
    let services = Services::new(clock.clone(), store.as_stores());
    Fixture { services, store, clock }
}

/// Register `handle`, join `slug`, and seat them as an enfranchised citizen.
async fn seat_citizen(f: &Fixture, handle: &str, slug: &str) {
    f.services.register_account(handle).await.unwrap();
    f.services.join_server(handle, slug).await.unwrap();
    let user = f.store.find_by_handle(handle).await.unwrap().unwrap();
    let server = f.store.find_by_slug(slug).await.unwrap().unwrap();
    let mut m = f.store.get(user.id, server.id).await.unwrap().unwrap();
    m.tier = Tier::Citizen;
    m.contribution = 5;
    m.enfranchised_at = Some(f.clock.now());
    f.store.upsert(m).await.unwrap();
}

#[tokio::test]
async fn an_invite_admits_a_member_but_never_a_voter() {
    let f = fixture();
    f.services.register_account("ada").await.unwrap();
    f.services.register_account("bo").await.unwrap();
    f.services.found_server_with_visibility("ada", "Secret Club", true).await.unwrap();

    let code = f.services.create_invite("ada", "secret-club").await.unwrap();
    let m = f.services.accept_invite("bo", &code).await.unwrap();

    assert!(!m.is_citizen(), "an invite must never grant the franchise");
    let bo = f.store.find_by_handle("bo").await.unwrap().unwrap();
    let server = f.store.find_by_slug("secret-club").await.unwrap().unwrap();
    assert!(f.store.get(bo.id, server.id).await.unwrap().is_some(), "bo is now a member");
}

#[tokio::test]
async fn private_servers_are_hidden_from_the_directory_public_ones_listed() {
    let f = fixture();
    f.services.register_account("ada").await.unwrap();
    f.services.found_server_with_visibility("ada", "Open Town", false).await.unwrap();
    f.services.found_server_with_visibility("ada", "Hidden Cabal", true).await.unwrap();

    let public: Vec<String> =
        f.services.list_public_servers().await.into_iter().map(|(g, _, _)| g.slug).collect();
    assert!(public.contains(&"open-town".to_string()));
    assert!(
        !public.contains(&"hidden-cabal".to_string()),
        "private servers stay out of the directory"
    );
}

#[tokio::test]
async fn a_non_member_cannot_mint_and_a_bad_code_is_rejected() {
    let f = fixture();
    f.services.register_account("ada").await.unwrap();
    f.services.register_account("bo").await.unwrap();
    f.services.found_server_with_visibility("ada", "Club", false).await.unwrap();

    assert_eq!(f.services.create_invite("bo", "club").await, Err(InviteError::NotMember("club".into())));
    assert_eq!(f.services.accept_invite("bo", "not-a-real-code").await, Err(InviteError::InvalidCode));
}

#[tokio::test]
async fn a_revoked_code_no_longer_admits() {
    let f = fixture();
    f.services.register_account("ada").await.unwrap();
    f.services.register_account("bo").await.unwrap();
    f.services.found_server_with_visibility("ada", "Club", false).await.unwrap();

    let code = f.services.create_invite("ada", "club").await.unwrap();
    let invites = f.services.list_invites("ada", "club").await.unwrap();
    assert_eq!(invites.len(), 1);
    f.services.revoke_invite("ada", "club", &invites[0].code_hash).await.unwrap();

    assert_eq!(f.services.accept_invite("bo", &code).await, Err(InviteError::InvalidCode));
    assert!(f.services.list_invites("ada", "club").await.unwrap().is_empty(), "revoked invites drop off");
}

#[tokio::test]
async fn voting_the_policy_closed_seals_minting() {
    let f = fixture();
    f.services.register_account("ada").await.unwrap();
    f.services.found_server("ada", "Club").await.unwrap();
    // Seat two more citizens so a ballot has a real electorate to pass.
    seat_citizen(&f, "bob", "club").await;
    seat_citizen(&f, "cid", "club").await;

    let before = f.store.find_by_slug("club").await.unwrap().unwrap();
    assert_eq!(before.invite_policy, InvitePolicy::Open, "invites start open");

    let p = f
        .services.governance()
        .open_proposal("ada", "club", ProposalKind::SetInvitePolicy { policy: InvitePolicy::Closed })
        .await
        .unwrap();
    f.services.governance().cast_vote("ada", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("bob", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("cid", p.id.0, true).await.unwrap();

    // Advance past the voting window and resolve.
    f.clock.set(Timestamp(1_000 * DAY + 5 * DAY));
    f.services.governance().resolve_due("club").await;

    let after = f.store.find_by_slug("club").await.unwrap().unwrap();
    assert_eq!(after.invite_policy, InvitePolicy::Closed, "the vote closed the door");
    assert_eq!(
        f.services.create_invite("ada", "club").await,
        Err(InviteError::Closed("club".into())),
        "no member may mint once closed"
    );
}
