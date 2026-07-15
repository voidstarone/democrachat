//! Integration tests for the M3 proposal lifecycle: open → vote → close → apply.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{Clock, MembershipStore, ServerStore, Services, UserStore};
use domain::{BallotKind, ProposalKind, ProposalStatus, Tier, Timestamp};

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

/// Directly seat an extra citizen (bypassing the 30-day criteria) so a server can
/// reach a quorum in tests. This is a *test* shortcut on the store, not a
/// use-case — production code has no such path.
async fn seat_citizen(f: &Fixture, handle: &str, slug: &str, contribution: i64) {
    f.services.register_account(handle).await.unwrap();
    f.services.join_server(handle, slug).await.unwrap();
    let user = f.store.find_by_handle(handle).await.unwrap().unwrap();
    let server = f.store.find_by_slug(slug).await.unwrap().unwrap();
    let mut m = f.store.get(user.id, server.id).await.unwrap().unwrap();
    m.tier = Tier::Citizen;
    m.contribution = contribution;
    m.enfranchised_at = Some(f.clock.now());
    f.store.upsert(m).await.unwrap();
}

async fn setup(f: &Fixture) {
    f.services.register_account("ada").await.unwrap();
    f.services.found_server("ada", "Town Square").await.unwrap();    // Seat two more citizens so a 3-citizen electorate can pass a RuleChange
    // (60% approval + 30% quorum).
    seat_citizen(f, "bob", "town-square", 5).await;
    seat_citizen(f, "cid", "town-square", 5).await;
}

#[tokio::test]
async fn a_passed_rule_ballot_applies_after_its_window() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;

    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "Be excellent".into() })
        .await
        .unwrap();
    // Unanimous aye from all three citizens.
    f.services.governance().cast_vote("ada", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("bob", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("cid", p.id.0, true).await.unwrap();

    // Before the window closes, nothing is applied.
    f.services.governance().resolve_due("town-square").await;
    assert!(f.services.governance().list_rules("town-square").await.is_empty());

    // Advance past the 3-day voting window and resolve.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;

    let rules = f.services.governance().list_rules("town-square").await;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].text, "Be excellent");
}

#[tokio::test]
async fn a_failed_ballot_applies_nothing() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "No fun".into() })
        .await
        .unwrap();
    // Only the proposer votes aye; the others vote nay → below 60%.
    f.services.governance().cast_vote("ada", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("bob", p.id.0, false).await.unwrap();
    f.services.governance().cast_vote("cid", p.id.0, false).await.unwrap();

    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;
    assert!(f.services.governance().list_rules("town-square").await.is_empty());

    let ps = f.store_proposals().await;
    assert_eq!(ps[0].status, ProposalStatus::Failed);
}

#[tokio::test]
async fn a_ban_ballot_sanctions_and_silences_the_target() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    // A member to be banned.
    f.services.register_account("troll").await.unwrap();
    f.services.join_server("troll", "town-square").await.unwrap();
    assert!(f.services.chat().post_message("troll", "town-square", "general", "spam").await.is_ok());

    let target = f.store.find_by_handle("troll").await.unwrap().unwrap();
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::Ban { user: target.id })
        .await
        .unwrap();
    f.services.governance().cast_vote("ada", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("bob", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("cid", p.id.0, true).await.unwrap();

    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;

    // The banned member can no longer post.
    assert!(f.services.chat().post_message("troll", "town-square", "general", "again").await.is_err());
}

#[tokio::test]
async fn a_server_cannot_open_a_ballot_it_does_not_govern() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    // Vote-weighting governance is opt-in and not enabled by default.
    let server = f.store.find_by_slug("town-square").await.unwrap().unwrap();
    assert!(!server.governs(BallotKind::SetVoteWeighting));
    let err = f
        .services.governance()
        .open_proposal(
            "ada",
            "town-square",
            ProposalKind::SetVoteWeighting { scheme: domain::VoteWeighting::ByContribution },
        )
        .await
        .unwrap_err();
    assert_eq!(err, app::ProposeError::NotGoverned);
}

#[tokio::test]
async fn only_a_citizen_may_open_or_vote() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    f.services.register_account("newbie").await.unwrap();
    f.services.join_server("newbie", "town-square").await.unwrap();
    // A fresh member is not a citizen.
    let err = f
        .services.governance()
        .open_proposal("newbie", "town-square", ProposalKind::AddRule { text: "x".into() })
        .await
        .unwrap_err();
    assert_eq!(err, app::ProposeError::NotACitizen);
}

/// Citizens can vote to disable automatic rehoming for their server — the
/// federation data-sovereignty control. It is a Constitutional ballot, so it needs
/// a chartered electorate and matures only after the recall timelock.
#[tokio::test]
async fn citizens_can_vote_to_disable_server_rehoming() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("ada").await.unwrap();
    f.services.found_server("ada", "Town Square").await.unwrap();    // Seat 4 more citizens → 5 with the founder, reaching Chartering, where a
    // Constitutional ballot (rehoming policy) is permitted.
    let voters = ["bob", "cid", "dan", "eve"];
    for h in voters {
        seat_citizen(&f, h, "town-square", 5).await;
    }

    // A new server starts with rehoming enabled.
    assert!(!f.store.find_by_slug("town-square").await.unwrap().unwrap().is_rehoming_disabled);

    let p = f
        .services.governance()
        .open_proposal(
            "ada",
            "town-square",
            ProposalKind::SetRehomingPolicy { is_disabled: true },
        )
        .await
        .unwrap();
    for h in ["ada", "bob", "cid", "dan", "eve"] {
        f.services.governance().cast_vote(h, p.id.0, true).await.unwrap();
    }

    // Close the voting window: it Passes, but a Constitutional change is timelocked
    // through the recall window — so the flag is not flipped yet.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;
    assert!(
        !f.store.find_by_slug("town-square").await.unwrap().unwrap().is_rehoming_disabled,
        "not yet effective during the recall timelock"
    );

    // Advance past the 7-day recall window and resolve → the effect applies.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY + 8 * DAY));
    f.services.governance().resolve_due("town-square").await;
    assert!(
        f.store.find_by_slug("town-square").await.unwrap().unwrap().is_rehoming_disabled,
        "citizens' vote disabled automatic rehoming"
    );
}

/// A RuleChange to the jury-sizing rule (the "trials" setting) applies after its
/// voting window — no chartering or timelock required.
#[tokio::test]
async fn citizens_can_change_jury_sizing_for_trials() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    assert!(
        matches!(f.store.find_by_slug("town-square").await.unwrap().unwrap().jury_sizing, domain::JurySizing::Sqrt { .. }),
        "a new server defaults to square-root jury sizing"
    );

    let p = f
        .services.governance()
        .open_proposal(
            "ada",
            "town-square",
            ProposalKind::SetJurySizing { sizing: domain::JurySizing::Fixed { post: 12, comment: 6 } },
        )
        .await
        .unwrap();
    for h in ["ada", "bob", "cid"] {
        f.services.governance().cast_vote(h, p.id.0, true).await.unwrap();
    }
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;

    assert_eq!(
        f.store.find_by_slug("town-square").await.unwrap().unwrap().jury_sizing,
        domain::JurySizing::Fixed { post: 12, comment: 6 },
        "citizens' vote resized the trial jury"
    );
}

/// Citizens can vote to change *what the server votes on* — enabling a ballot kind
/// (here vote-weighting governance) that was not on the default surface.
#[tokio::test]
async fn citizens_can_change_what_the_server_votes_on() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    let before = f.store.find_by_slug("town-square").await.unwrap().unwrap();
    assert!(!before.governs(BallotKind::SetVoteWeighting), "vote-weighting governance is opt-in");

    let mut surface = before.enabled_ballots.clone();
    surface.insert(BallotKind::SetVoteWeighting);
    surface.insert(BallotKind::SetWeightingScope);
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::SetGovernanceSurface { enabled: surface })
        .await
        .unwrap();
    for h in ["ada", "bob", "cid"] {
        f.services.governance().cast_vote(h, p.id.0, true).await.unwrap();
    }
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;

    assert!(
        f.store.find_by_slug("town-square").await.unwrap().unwrap().governs(BallotKind::SetVoteWeighting),
        "the surface now includes vote-weighting governance"
    );
}

/// Amending the franchise criteria (who may vote) is Constitutional: it needs a
/// chartered electorate and matures only after the recall timelock.
#[tokio::test]
async fn citizens_can_amend_who_may_vote() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("ada").await.unwrap();
    f.services.found_server("ada", "Town Square").await.unwrap();
    // Four more citizens → five with the founder, reaching Chartering.
    let voters = ["bob", "cid", "dan", "eve"];
    for h in voters {
        seat_citizen(&f, h, "town-square", 5).await;
    }
    let proposed =
        domain::FranchiseCriteria { min_account_age_days: 60, min_membership_days: 30, min_contribution: 10 };

    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AmendCriteria { proposed: proposed.clone() })
        .await
        .unwrap();
    for h in ["ada", "bob", "cid", "dan", "eve"] {
        f.services.governance().cast_vote(h, p.id.0, true).await.unwrap();
    }

    // Passes at window close, but a Constitutional change is timelocked.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;
    assert_ne!(
        f.store.find_by_slug("town-square").await.unwrap().unwrap().criteria.min_account_age_days, 60,
        "not yet effective during the recall timelock"
    );

    // Past the recall window → the new criteria take effect.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY + 8 * DAY));
    f.services.governance().resolve_due("town-square").await;
    assert_eq!(
        f.store.find_by_slug("town-square").await.unwrap().unwrap().criteria, proposed,
        "citizens' vote amended who may earn the franchise"
    );
}

#[tokio::test]
async fn an_amended_bundle_enacts_all_its_changes_together() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;

    // A proposal to add one rule, then amended to also create a channel: two
    // changes riding one ballot.
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "Be excellent".into() })
        .await
        .unwrap();
    f.services.governance()
        .amend_proposal("bob", p.id.0, ProposalKind::CreateChannel { name: "lounge".into(), topic: "".into(), is_voice: false })
        .await
        .unwrap();

    // One vote decides the whole bundle.
    f.services.governance().cast_vote("ada", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("bob", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("cid", p.id.0, true).await.unwrap();

    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;

    // Both the primary change and the amendment took effect.
    assert_eq!(f.services.governance().list_rules("town-square").await.len(), 1, "the rule was added");
    let channels = f.services.chat().list_channels("town-square").await.unwrap();
    assert!(channels.iter().any(|c| c.name == "lounge"), "the amendment's channel was created");
}

#[tokio::test]
async fn a_passed_ballot_can_charter_a_voice_channel() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    let p = f
        .services.governance()
        .open_proposal(
            "ada",
            "town-square",
            ProposalKind::CreateChannel { name: "lounge".into(), topic: "".into(), is_voice: true },
        )
        .await
        .unwrap();
    f.services.governance().cast_vote("ada", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("bob", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("cid", p.id.0, true).await.unwrap();

    // Before the window closes, resolving changes nothing.
    assert!(!f.services.governance().resolve_due("town-square").await, "nothing due yet");

    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    // The resolution that closes-and-applies reports that it moved state, so the
    // web layer knows to persist and nudge clients.
    assert!(f.services.governance().resolve_due("town-square").await, "closed and applied");
    // A second resolve is a no-op — nothing left to do.
    assert!(!f.services.governance().resolve_due("town-square").await, "idempotent");

    let channels = f.services.chat().list_channels("town-square").await.unwrap();
    let lounge = channels.iter().find(|c| c.name == "lounge").expect("channel chartered");
    assert_eq!(lounge.kind, domain::ChannelKind::Voice, "chartered as a voice channel");
}

#[tokio::test]
async fn a_defeated_bundle_enacts_none_of_its_changes() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "No fun".into() })
        .await
        .unwrap();
    f.services.governance()
        .amend_proposal("ada", p.id.0, ProposalKind::CreateChannel { name: "lounge".into(), topic: "".into(), is_voice: false })
        .await
        .unwrap();
    // The bundle fails its threshold.
    f.services.governance().cast_vote("ada", p.id.0, true).await.unwrap();
    f.services.governance().cast_vote("bob", p.id.0, false).await.unwrap();
    f.services.governance().cast_vote("cid", p.id.0, false).await.unwrap();

    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;

    assert!(f.services.governance().list_rules("town-square").await.is_empty(), "rule not added");
    let channels = f.services.chat().list_channels("town-square").await.unwrap();
    assert!(!channels.iter().any(|c| c.name == "lounge"), "amendment's channel not created");
    assert_eq!(f.store_proposals().await[0].status, ProposalStatus::Failed);
}

#[tokio::test]
async fn a_closed_proposal_takes_no_amendments() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "Be excellent".into() })
        .await
        .unwrap();
    // Close the window.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square").await;

    let err = f
        .services.governance()
        .amend_proposal("bob", p.id.0, ProposalKind::AddRule { text: "too late".into() })
        .await
        .unwrap_err();
    assert_eq!(err, app::ProposeError::Closed);
}

#[tokio::test]
async fn only_a_citizen_may_join_the_debate() {
    let f = fixture(1_000 * DAY);
    setup(&f).await;
    // A non-citizen member.
    f.services.register_account("newbie").await.unwrap();
    f.services.join_server("newbie", "town-square").await.unwrap();

    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "Be excellent".into() })
        .await
        .unwrap();

    assert_eq!(
        f.services.governance().post_discussion("newbie", p.id.0, "aye!").await.unwrap_err(),
        app::VoteError::NotACitizen
    );
    f.services.governance().post_discussion("ada", p.id.0, "I think aye.").await.unwrap();
    let thread = f.services.governance().list_discussion(p.id.0).await;
    assert_eq!(thread.len(), 1);
    assert_eq!(thread[0].body, "I think aye.");
}

impl Fixture {
    async fn store_proposals(&self) -> Vec<domain::Proposal> {
        use app::ProposalStore;
        let server = self.store.find_by_slug("town-square").await.unwrap().unwrap();
        ProposalStore::list_for_server(&*self.store, server.id).await.unwrap()
    }
}
