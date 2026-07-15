//! Integration tests for governed roles and `@mention` resolution.
//!
//! Roles are created and populated only by ballot, so these tests drive the full
//! open → vote → close → apply lifecycle and then check both the role state and
//! how a message body's mentions resolve.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{Clock, MembershipStore, MentionKind, ServerStore, Services, UserStore};
use domain::{ProposalKind, Tier, Timestamp};

const DAY: i64 = 86_400;

struct Fixture {
    services: Services,
    clock: Arc<FixedClock>,
    store: Arc<MemoryStore>,
}

fn fixture(now_secs: i64) -> Fixture {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(now_secs)));
    let services = Services::new(clock.clone(), store.as_stores());
    Fixture { services, clock, store }
}

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

async fn setup(f: &Fixture) {
    f.services.register_account("ada").await.unwrap();
    f.services.found_server("ada", "Town Square").await.unwrap();    seat_citizen(f, "bob", "town-square").await;
    seat_citizen(f, "cid", "town-square").await;
}

/// Pass a proposal unanimously and apply it after the voting window.
async fn pass(f: &Fixture, kind: ProposalKind) {
    let p = f.services.governance().open_proposal("ada", "town-square", kind).await.unwrap();
    for v in ["ada", "bob", "cid"] {
        f.services.governance().cast_vote(v, p.id.0, true).await.unwrap();
    }
    let now = f.clock.now().0;
    f.clock.set(Timestamp(now + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;
}

#[tokio::test]
async fn a_role_is_created_only_by_ballot() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    assert!(f.services.roles().list_roles("town-square").await.is_empty());

    pass(&f, ProposalKind::CreateRole { name: "Mapmakers".into() }).await;

    let roles = f.services.roles().list_roles("town-square").await;
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].name, "mapmakers"); // normalized
}

#[tokio::test]
async fn a_member_is_assigned_to_a_role_by_ballot_and_shows_as_a_holder() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, ProposalKind::CreateRole { name: "mapmakers".into() }).await;
    let role = f.services.roles().list_roles("town-square").await[0].clone();
    let bob = f.store.find_by_handle("bob").await.unwrap().unwrap();

    pass(&f, ProposalKind::AssignRole { user: bob.id, role: role.id }).await;

    assert_eq!(f.services.roles().role_holders("town-square", "mapmakers").await, vec!["bob".to_string()]);
}

#[tokio::test]
async fn a_role_mention_resolves_to_its_holders() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, ProposalKind::CreateRole { name: "mapmakers".into() }).await;
    let role = f.services.roles().list_roles("town-square").await[0].clone();
    let bob = f.store.find_by_handle("bob").await.unwrap().unwrap();
    pass(&f, ProposalKind::AssignRole { user: bob.id, role: role.id }).await;

    let resolved = f.services.roles().resolve_mentions("town-square", "ping @mapmakers please").await;
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].kind, MentionKind::Role);
    assert_eq!(resolved[0].handles, vec!["bob".to_string()]);
}

#[tokio::test]
async fn the_citizens_standing_role_addresses_every_citizen() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    // ada, bob, cid are citizens; add a plain member.
    f.services.register_account("newbie").await.unwrap();
    f.services.join_server("newbie", "town-square").await.unwrap();

    let resolved = f.services.roles().resolve_mentions("town-square", "@citizens assemble").await;
    assert_eq!(resolved[0].kind, MentionKind::StandingRole);
    let mut handles = resolved[0].handles.clone();
    handles.sort();
    assert_eq!(handles, vec!["ada".to_string(), "bob".to_string(), "cid".to_string()]);
    assert!(!handles.contains(&"newbie".to_string()));
}

#[tokio::test]
async fn a_user_mention_resolves_to_that_member() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    let resolved = f.services.roles().resolve_mentions("town-square", "hi @bob").await;
    assert_eq!(resolved[0].kind, MentionKind::User);
    assert_eq!(resolved[0].handles, vec!["bob".to_string()]);
}

#[tokio::test]
async fn an_unknown_or_non_member_mention_is_marked_unknown() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    // "stranger" is a registered account but not a member of this server.
    f.services.register_account("stranger").await.unwrap();
    let resolved = f.services.roles().resolve_mentions("town-square", "@nobody and @stranger").await;
    assert!(resolved.iter().all(|m| m.kind == MentionKind::Unknown));
    assert!(resolved.iter().all(|m| m.handles.is_empty()));
}

#[tokio::test]
async fn deleting_a_role_removes_its_assignments() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, ProposalKind::CreateRole { name: "temp".into() }).await;
    let role = f.services.roles().list_roles("town-square").await[0].clone();
    let bob = f.store.find_by_handle("bob").await.unwrap().unwrap();
    pass(&f, ProposalKind::AssignRole { user: bob.id, role: role.id }).await;
    assert_eq!(f.services.roles().role_holders("town-square", "temp").await.len(), 1);

    pass(&f, ProposalKind::DeleteRole { role: role.id }).await;
    assert!(f.services.roles().list_roles("town-square").await.is_empty());
    assert!(f.services.roles().role_holders("town-square", "temp").await.is_empty());
}

#[tokio::test]
async fn a_roles_colour_is_the_plurality_of_citizen_votes_and_shows_in_the_popover() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, ProposalKind::CreateRole { name: "crew".into() }).await;
    let role = f.services.roles().list_roles("town-square").await[0].clone();
    let bob = f.store.find_by_handle("bob").await.unwrap().unwrap();
    pass(&f, ProposalKind::AssignRole { user: bob.id, role: role.id }).await;

    // No votes yet → no colour.
    let (_, color) = f.services.roles().roles_with_color("town-square").await[0].clone();
    assert_eq!(color, None);

    // Two citizens pick red, one picks blue → red wins the plurality.
    f.services.roles().vote_role_color("ada", "town-square", role.id.0, "#ff0000").await.unwrap();
    f.services.roles().vote_role_color("bob", "town-square", role.id.0, "#FF0000").await.unwrap();
    f.services.roles().vote_role_color("cid", "town-square", role.id.0, "#0000ff").await.unwrap();

    let (_, color) = f.services.roles().roles_with_color("town-square").await[0].clone();
    assert_eq!(color.as_ref().map(|c| c.as_str()), Some("#ff0000"));

    // The popover for bob (a holder) shows the winning colour and bob's own vote.
    let ur = f.services.roles().user_roles("town-square", "bob", "bob").await.unwrap();
    assert_eq!(ur.tier, Tier::Citizen);
    assert!(ur.standing.contains(&"citizens".to_string()));
    assert_eq!(ur.roles.len(), 1);
    assert_eq!(ur.roles[0].color.as_deref(), Some("#ff0000"));
    assert_eq!(ur.roles[0].my_color.as_deref(), Some("#ff0000"));

    // A re-vote replaces the prior one — cid switches to red, now unanimous.
    f.services.roles().vote_role_color("cid", "town-square", role.id.0, "#ff0000").await.unwrap();
    let (_, color) = f.services.roles().roles_with_color("town-square").await[0].clone();
    assert_eq!(color.as_ref().map(|c| c.as_str()), Some("#ff0000"));
}

#[tokio::test]
async fn only_franchised_citizens_may_vote_a_roles_colour() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, ProposalKind::CreateRole { name: "crew".into() }).await;
    let role = f.services.roles().list_roles("town-square").await[0].clone();

    // A plain member (not enfranchised) can't vote a colour.
    f.services.register_account("newbie").await.unwrap();
    f.services.join_server("newbie", "town-square").await.unwrap();
    assert!(f.services.roles().vote_role_color("newbie", "town-square", role.id.0, "#123456").await.is_err());

    // A bad hex value is rejected.
    assert!(f.services.roles().vote_role_color("ada", "town-square", role.id.0, "nope").await.is_err());

    // A non-member target has no popover.
    assert!(f.services.roles().user_roles("town-square", "stranger", "ada").await.is_none());
}

#[tokio::test]
async fn mentioned_handles_dedupes_across_user_and_role() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, ProposalKind::CreateRole { name: "crew".into() }).await;
    let role = f.services.roles().list_roles("town-square").await[0].clone();
    let bob = f.store.find_by_handle("bob").await.unwrap().unwrap();
    pass(&f, ProposalKind::AssignRole { user: bob.id, role: role.id }).await;

    // @bob appears directly and via @crew — should be listed once.
    let handles = f.services.roles().mentioned_handles("town-square", "@bob and @crew").await;
    assert_eq!(handles, vec!["bob".to_string()]);
}
