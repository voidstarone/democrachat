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

/// ada (founder) + bob + cid as citizens, plus "target" as a plain member.
fn setup(f: &Fixture) {
    f.services.register_account("ada").unwrap();
    f.services.found_server("ada", "Town Square").unwrap();
    seat_citizen(f, "bob", "town-square");
    seat_citizen(f, "cid", "town-square");
    f.services.register_account("target").unwrap();
    f.services.join_server("target", "town-square").unwrap();
}

fn uid(f: &Fixture, handle: &str) -> UserId {
    f.store.find_by_handle(handle).unwrap().id
}

/// Open a ballot, carry it unanimously through the three citizens, then close the
/// window and apply it.
fn pass(f: &Fixture, proposer: &str, kind: ProposalKind) {
    let p = f.services.open_proposal(proposer, "town-square", kind).unwrap();
    for c in ["ada", "bob", "cid"] {
        f.services.cast_vote(c, p.id.0, true).unwrap();
    }
    let now = f.clock.now();
    f.clock.set(now.plus_days(4));
    f.services.resolve_due("town-square");
}

fn is_police(f: &Fixture, handle: &str) -> bool {
    let (police, _) = f.services.police_and_mute_status(handle, "town-square").unwrap();
    police
}
fn is_muted(f: &Fixture, handle: &str) -> bool {
    let (_, muted) = f.services.police_and_mute_status(handle, "town-square").unwrap();
    muted
}

#[test]
fn police_are_appointed_and_dismissed_by_ballot() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    assert!(!is_police(&f, "ada"));
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "ada") });
    assert!(is_police(&f, "ada"), "the ballot appointed ada as police");
    pass(&f, "bob", ProposalKind::DismissPolice { user: uid(&f, "ada") });
    assert!(!is_police(&f, "ada"), "a later ballot dismissed her");
}

#[test]
fn a_police_officer_instantly_mutes_and_unmutes() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "ada") });

    // Before the mute the target can post in #general.
    f.services.post_message("target", "town-square", "general", "hi all").unwrap();

    f.services.mute_member("ada", "town-square", "target").unwrap();
    assert!(is_muted(&f, "target"));
    // Muted → cannot post in an ordinary channel …
    assert_eq!(
        f.services.post_message("target", "town-square", "general", "still here?"),
        Err(MessageError::Muted("target".into())),
    );
    // … but *can* post in #appeals to plead their case.
    f.services.post_message("target", "town-square", "appeals", "please unmute me").unwrap();

    f.services.unmute_member("ada", "town-square", "target").unwrap();
    assert!(!is_muted(&f, "target"));
    f.services.post_message("target", "town-square", "general", "thanks").unwrap();
}

#[test]
fn a_non_officer_cannot_mute() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    // bob is a citizen but not police.
    assert_eq!(
        f.services.mute_member("bob", "town-square", "target").unwrap_err(),
        MuteError::NotPolice,
    );
}

#[test]
fn police_cannot_be_muted() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "ada") });
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "bob") });
    assert_eq!(
        f.services.mute_member("ada", "town-square", "bob").unwrap_err(),
        MuteError::CannotMutePolice,
    );
}

#[test]
fn citizens_can_impose_and_lift_a_mute_by_vote() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    pass(&f, "ada", ProposalKind::Mute { user: uid(&f, "target") });
    assert!(is_muted(&f, "target"), "a Mute ballot silenced the target");

    pass(&f, "ada", ProposalKind::LiftMute { user: uid(&f, "target") });
    assert!(!is_muted(&f, "target"), "a LiftMute ballot restored them");
    // A vote-imposed mute has no officer, so lifting it bars no one.
    f.services.mute_member("ada", "town-square", "target").ok(); // ada isn't police here → NotPolice, ignored
}

#[test]
fn a_vote_lift_bars_the_muting_officer_for_24h() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    pass(&f, "ada", ProposalKind::AppointPolice { user: uid(&f, "ada") });

    // ada mutes target; the electorate overturns it by vote.
    f.services.mute_member("ada", "town-square", "target").unwrap();
    pass(&f, "ada", ProposalKind::LiftMute { user: uid(&f, "target") });
    assert!(!is_muted(&f, "target"));

    // ada is now barred from re-muting target within the 24h window.
    assert_eq!(
        f.services.mute_member("ada", "town-square", "target").unwrap_err(),
        MuteError::RemuteBlocked,
    );

    // Past the cooldown, ada may mute again.
    let now = f.clock.now();
    f.clock.set(now.plus_days(2));
    f.services.mute_member("ada", "town-square", "target").unwrap();
    assert!(is_muted(&f, "target"));
}

#[test]
fn the_appeals_channel_is_hidden_from_ordinary_members() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    // "target" is a plain, unmuted member: no vote, not police, not muted.
    let visible = f.services.visible_channels("target", "town-square").unwrap();
    assert!(
        !visible.iter().any(|c| c.name == "appeals"),
        "an ordinary member does not see #appeals",
    );
    // Reading it directly is refused, as if it did not exist.
    assert!(matches!(
        f.services.channel_messages_for("target", "town-square", "appeals"),
        Err(MessageError::NoSuchChannel(_)),
    ));

    // A citizen (voter) does see it.
    let seen_by_citizen = f.services.visible_channels("bob", "town-square").unwrap();
    assert!(seen_by_citizen.iter().any(|c| c.name == "appeals"), "a voter sees #appeals");

    // And once muted, the target sees it too (to appeal).
    pass(&f, "ada", ProposalKind::Mute { user: uid(&f, "target") });
    let seen_when_muted = f.services.visible_channels("target", "town-square").unwrap();
    assert!(seen_when_muted.iter().any(|c| c.name == "appeals"), "a muted member sees #appeals");
}
