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
fn seat_citizen(f: &Fixture, handle: &str, slug: &str) {
    f.services.register_account(handle).unwrap();
    f.services.join_server(handle, slug).unwrap();
    let user = f.store.find_by_handle(handle).unwrap();
    let server = f.store.find_by_slug(slug).unwrap();
    let mut m = f.store.get(user.id, server.id).unwrap();
    m.tier = Tier::Citizen;
    m.contribution = 5;
    m.enfranchised_at = Some(f.clock.now());
    f.store.upsert(m);
}

#[test]
fn an_invite_admits_a_member_but_never_a_voter() {
    let f = fixture();
    f.services.register_account("ada").unwrap();
    f.services.register_account("bo").unwrap();
    f.services.found_server_with_visibility("ada", "Secret Club", true).unwrap();

    let code = f.services.create_invite("ada", "secret-club").unwrap();
    let m = f.services.accept_invite("bo", &code).unwrap();

    assert!(!m.is_citizen(), "an invite must never grant the franchise");
    let bo = f.store.find_by_handle("bo").unwrap();
    let server = f.store.find_by_slug("secret-club").unwrap();
    assert!(f.store.get(bo.id, server.id).is_some(), "bo is now a member");
}

#[test]
fn private_servers_are_hidden_from_the_directory_public_ones_listed() {
    let f = fixture();
    f.services.register_account("ada").unwrap();
    f.services.found_server_with_visibility("ada", "Open Town", false).unwrap();
    f.services.found_server_with_visibility("ada", "Hidden Cabal", true).unwrap();

    let public: Vec<String> =
        f.services.list_public_servers().into_iter().map(|(g, _, _)| g.slug).collect();
    assert!(public.contains(&"open-town".to_string()));
    assert!(
        !public.contains(&"hidden-cabal".to_string()),
        "private servers stay out of the directory"
    );
}

#[test]
fn a_non_member_cannot_mint_and_a_bad_code_is_rejected() {
    let f = fixture();
    f.services.register_account("ada").unwrap();
    f.services.register_account("bo").unwrap();
    f.services.found_server_with_visibility("ada", "Club", false).unwrap();

    assert_eq!(f.services.create_invite("bo", "club"), Err(InviteError::NotMember("club".into())));
    assert_eq!(f.services.accept_invite("bo", "not-a-real-code"), Err(InviteError::InvalidCode));
}

#[test]
fn a_revoked_code_no_longer_admits() {
    let f = fixture();
    f.services.register_account("ada").unwrap();
    f.services.register_account("bo").unwrap();
    f.services.found_server_with_visibility("ada", "Club", false).unwrap();

    let code = f.services.create_invite("ada", "club").unwrap();
    let invites = f.services.list_invites("ada", "club").unwrap();
    assert_eq!(invites.len(), 1);
    f.services.revoke_invite("ada", "club", &invites[0].code_hash).unwrap();

    assert_eq!(f.services.accept_invite("bo", &code), Err(InviteError::InvalidCode));
    assert!(f.services.list_invites("ada", "club").unwrap().is_empty(), "revoked invites drop off");
}

#[test]
fn voting_the_policy_closed_seals_minting() {
    let f = fixture();
    f.services.register_account("ada").unwrap();
    f.services.found_server("ada", "Club").unwrap();
    // Seat two more citizens so a ballot has a real electorate to pass.
    seat_citizen(&f, "bob", "club");
    seat_citizen(&f, "cid", "club");

    let before = f.store.find_by_slug("club").unwrap();
    assert_eq!(before.invite_policy, InvitePolicy::Open, "invites start open");

    let p = f
        .services
        .open_proposal("ada", "club", ProposalKind::SetInvitePolicy { policy: InvitePolicy::Closed })
        .unwrap();
    f.services.cast_vote("ada", p.id.0, true).unwrap();
    f.services.cast_vote("bob", p.id.0, true).unwrap();
    f.services.cast_vote("cid", p.id.0, true).unwrap();

    // Advance past the voting window and resolve.
    f.clock.set(Timestamp(1_000 * DAY + 5 * DAY));
    f.services.resolve_due("club");

    let after = f.store.find_by_slug("club").unwrap();
    assert_eq!(after.invite_policy, InvitePolicy::Closed, "the vote closed the door");
    assert_eq!(
        f.services.create_invite("ada", "club"),
        Err(InviteError::Closed("club".into())),
        "no member may mint once closed"
    );
}
