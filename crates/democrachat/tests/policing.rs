//! Integration tests for the policing / mute / appeals feature: police appointed
//! by ballot, instant mutes and unmutes, vote-imposed and vote-lifted mutes, the
//! 24-hour re-mute cooldown, and the restricted #appeals channel.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{Clock, MembershipStore, MessageError, MuteError, ServerStore, Services, UserStore};
use domain::{ProposalKind, Tier, Timestamp, UserId};

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

/// ada (founder) + bob + cid as citizens, plus "target" as a plain member.
async fn setup(f: &Fixture) {
    f.services.register_account("ada").await.unwrap();
    f.services.found_server("ada", "Town Square").await.unwrap();
    seat_citizen(f, "bob", "town-square").await;
    seat_citizen(f, "cid", "town-square").await;
    f.services.register_account("target").await.unwrap();
    f.services.join_server("target", "town-square").await.unwrap();
}

async fn uid(f: &Fixture, handle: &str) -> UserId {
    f.store.find_by_handle(handle).await.unwrap().unwrap().id
}

/// Open a ballot, carry it unanimously through the three citizens, then close the
/// window and apply it.
async fn pass(f: &Fixture, proposer: &str, kind: ProposalKind) {
    let p = f.services.governance().open_proposal(proposer, "town-square", kind).await.unwrap();
    for c in ["ada", "bob", "cid"] {
        f.services.governance().cast_vote(c, p.id.0, true).await.unwrap();
    }
    let now = f.clock.now();
    f.clock.set(now.plus_days(4));
    f.services.governance().resolve_due("town-square").await;
}

async fn is_police(f: &Fixture, handle: &str) -> bool {
    let (police, _) = f.services.mute().police_and_mute_status(handle, "town-square").await.unwrap();
    police
}
async fn is_muted(f: &Fixture, handle: &str) -> bool {
    let (_, muted) = f.services.mute().police_and_mute_status(handle, "town-square").await.unwrap();
    muted
}

#[tokio::test]
async fn police_are_appointed_and_dismissed_by_ballot() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    assert!(!is_police(&f, "ada").await);
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "ada").await }).await;
    assert!(is_police(&f, "ada").await, "the ballot appointed ada as police");
    pass(&f, "bob", ProposalKind::DismissPolice { user: uid(&f, "ada").await }).await;
    assert!(!is_police(&f, "ada").await, "a later ballot dismissed her");
}

#[tokio::test]
async fn a_police_officer_instantly_mutes_and_unmutes() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "ada").await }).await;

    // Before the mute the target can post in #general.
    f.services.chat().post_message("target", "town-square", "general", "hi all").await.unwrap();

    f.services.mute().mute_member("ada", "town-square", "target").await.unwrap();
    assert!(is_muted(&f, "target").await);
    // Muted → cannot post in an ordinary channel …
    assert_eq!(
        f.services.chat().post_message("target", "town-square", "general", "still here?").await,
        Err(MessageError::Muted("target".into())),
    );
    // … but *can* post in #appeals to plead their case.
    f.services.chat().post_message("target", "town-square", "appeals", "please unmute me").await.unwrap();

    f.services.mute().unmute_member("ada", "town-square", "target").await.unwrap();
    assert!(!is_muted(&f, "target").await);
    f.services.chat().post_message("target", "town-square", "general", "thanks").await.unwrap();
}

#[tokio::test]
async fn a_non_officer_cannot_mute() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    // bob is a citizen but not police.
    assert_eq!(
        f.services.mute().mute_member("bob", "town-square", "target").await.unwrap_err(),
        MuteError::NotPolice,
    );
}

#[tokio::test]
async fn police_cannot_be_muted() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "ada").await }).await;
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "bob").await }).await;
    assert_eq!(
        f.services.mute().mute_member("ada", "town-square", "bob").await.unwrap_err(),
        MuteError::CannotMutePolice,
    );
}

#[tokio::test]
async fn citizens_can_impose_and_lift_a_mute_by_vote() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, "ada", ProposalKind::Mute { user: uid(&f, "target").await }).await;
    assert!(is_muted(&f, "target").await, "a Mute ballot silenced the target");

    pass(&f, "ada", ProposalKind::LiftMute { user: uid(&f, "target").await }).await;
    assert!(!is_muted(&f, "target").await, "a LiftMute ballot restored them");
    // A vote-imposed mute has no officer, so lifting it bars no one.
    f.services.mute().mute_member("ada", "town-square", "target").await.ok(); // ada isn't police here → NotPolice, ignored
}

#[tokio::test]
async fn a_vote_lift_bars_the_muting_officer_for_24h() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "ada").await }).await;

    // ada mutes target; the electorate overturns it by vote.
    f.services.mute().mute_member("ada", "town-square", "target").await.unwrap();
    pass(&f, "ada", ProposalKind::LiftMute { user: uid(&f, "target").await }).await;
    assert!(!is_muted(&f, "target").await);

    // ada is now barred from re-muting target within the 24h window.
    assert_eq!(
        f.services.mute().mute_member("ada", "town-square", "target").await.unwrap_err(),
        MuteError::RemuteBlocked,
    );

    // Past the cooldown, ada may mute again.
    let now = f.clock.now();
    f.clock.set(now.plus_days(2));
    f.services.mute().mute_member("ada", "town-square", "target").await.unwrap();
    assert!(is_muted(&f, "target").await);
}

#[tokio::test]
async fn the_appeals_channel_is_hidden_from_ordinary_members() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    // "target" is a plain, unmuted member: no vote, not police, not muted.
    let visible = f.services.chat().visible_channels("target", "town-square").await.unwrap();
    assert!(
        !visible.iter().any(|c| c.name == "appeals"),
        "an ordinary member does not see #appeals",
    );
    // Reading it directly is refused, as if it did not exist.
    assert!(matches!(
        f.services.chat().channel_messages_for("target", "town-square", "appeals").await,
        Err(MessageError::NoSuchChannel(_)),
    ));

    // A citizen (voter) does see it.
    let seen_by_citizen = f.services.chat().visible_channels("bob", "town-square").await.unwrap();
    assert!(seen_by_citizen.iter().any(|c| c.name == "appeals"), "a voter sees #appeals");

    // And once muted, the target sees it too (to appeal).
    pass(&f, "ada", ProposalKind::Mute { user: uid(&f, "target").await }).await;
    let seen_when_muted = f.services.chat().visible_channels("target", "town-square").await.unwrap();
    assert!(seen_when_muted.iter().any(|c| c.name == "appeals"), "a muted member sees #appeals");
}
