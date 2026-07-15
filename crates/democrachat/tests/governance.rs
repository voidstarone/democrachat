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
fn seat_citizen(f: &Fixture, handle: &str, slug: &str, contribution: i64) {
    f.services.register_account(handle).unwrap();
    f.services.join_server(handle, slug).unwrap();
    let user = f.store.find_by_handle(handle).unwrap().unwrap();
    let server = f.store.find_by_slug(slug).unwrap().unwrap();
    let mut m = f.store.get(user.id, server.id).unwrap().unwrap();
    m.tier = Tier::Citizen;
    m.contribution = contribution;
    m.enfranchised_at = Some(f.clock.now());
    f.store.upsert(m).unwrap();
}

fn setup(f: &Fixture) {
    f.services.register_account("ada").unwrap();
    f.services.found_server("ada", "Town Square").unwrap();    // Seat two more citizens so a 3-citizen electorate can pass a RuleChange
    // (60% approval + 30% quorum).
    seat_citizen(f, "bob", "town-square", 5);
    seat_citizen(f, "cid", "town-square", 5);
}

#[test]
fn a_passed_rule_ballot_applies_after_its_window() {
    let f = fixture(1_000 * DAY);
    setup(&f);

    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "Be excellent".into() })
        .unwrap();
    // Unanimous aye from all three citizens.
    f.services.governance().cast_vote("ada", p.id.0, true).unwrap();
    f.services.governance().cast_vote("bob", p.id.0, true).unwrap();
    f.services.governance().cast_vote("cid", p.id.0, true).unwrap();

    // Before the window closes, nothing is applied.
    f.services.governance().resolve_due("town-square");
    assert!(f.services.governance().list_rules("town-square").is_empty());

    // Advance past the 3-day voting window and resolve.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");

    let rules = f.services.governance().list_rules("town-square");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].text, "Be excellent");
}

#[test]
fn a_failed_ballot_applies_nothing() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "No fun".into() })
        .unwrap();
    // Only the proposer votes aye; the others vote nay → below 60%.
    f.services.governance().cast_vote("ada", p.id.0, true).unwrap();
    f.services.governance().cast_vote("bob", p.id.0, false).unwrap();
    f.services.governance().cast_vote("cid", p.id.0, false).unwrap();

    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");
    assert!(f.services.governance().list_rules("town-square").is_empty());

    let ps = f.store_proposals();
    assert_eq!(ps[0].status, ProposalStatus::Failed);
}

#[test]
fn a_ban_ballot_sanctions_and_silences_the_target() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    // A member to be banned.
    f.services.register_account("troll").unwrap();
    f.services.join_server("troll", "town-square").unwrap();
    assert!(f.services.chat().post_message("troll", "town-square", "general", "spam").is_ok());

    let target = f.store.find_by_handle("troll").unwrap().unwrap();
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::Ban { user: target.id })
        .unwrap();
    f.services.governance().cast_vote("ada", p.id.0, true).unwrap();
    f.services.governance().cast_vote("bob", p.id.0, true).unwrap();
    f.services.governance().cast_vote("cid", p.id.0, true).unwrap();

    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");

    // The banned member can no longer post.
    assert!(f.services.chat().post_message("troll", "town-square", "general", "again").is_err());
}

#[test]
fn a_server_cannot_open_a_ballot_it_does_not_govern() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    // Vote-weighting governance is opt-in and not enabled by default.
    let server = f.store.find_by_slug("town-square").unwrap().unwrap();
    assert!(!server.governs(BallotKind::SetVoteWeighting));
    let err = f
        .services.governance()
        .open_proposal(
            "ada",
            "town-square",
            ProposalKind::SetVoteWeighting { scheme: domain::VoteWeighting::ByContribution },
        )
        .unwrap_err();
    assert_eq!(err, app::ProposeError::NotGoverned);
}

#[test]
fn only_a_citizen_may_open_or_vote() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    f.services.register_account("newbie").unwrap();
    f.services.join_server("newbie", "town-square").unwrap();
    // A fresh member is not a citizen.
    let err = f
        .services.governance()
        .open_proposal("newbie", "town-square", ProposalKind::AddRule { text: "x".into() })
        .unwrap_err();
    assert_eq!(err, app::ProposeError::NotACitizen);
}

/// Citizens can vote to disable automatic rehoming for their server — the
/// federation data-sovereignty control. It is a Constitutional ballot, so it needs
/// a chartered electorate and matures only after the recall timelock.
#[test]
fn citizens_can_vote_to_disable_server_rehoming() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("ada").unwrap();
    f.services.found_server("ada", "Town Square").unwrap();    // Seat 9 more citizens → 10 with the founder, reaching Chartering, where a
    // Constitutional ballot (rehoming policy) is permitted.
    let voters = ["bob", "cid", "dan", "eve", "fay", "gus", "hal", "ike", "jan"];
    for h in voters {
        seat_citizen(&f, h, "town-square", 5);
    }

    // A new server starts with rehoming enabled.
    assert!(!f.store.find_by_slug("town-square").unwrap().unwrap().is_rehoming_disabled);

    let p = f
        .services.governance()
        .open_proposal(
            "ada",
            "town-square",
            ProposalKind::SetRehomingPolicy { is_disabled: true },
        )
        .unwrap();
    for h in ["ada", "bob", "cid", "dan", "eve", "fay", "gus", "hal", "ike", "jan"] {
        f.services.governance().cast_vote(h, p.id.0, true).unwrap();
    }

    // Close the voting window: it Passes, but a Constitutional change is timelocked
    // through the recall window — so the flag is not flipped yet.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");
    assert!(
        !f.store.find_by_slug("town-square").unwrap().unwrap().is_rehoming_disabled,
        "not yet effective during the recall timelock"
    );

    // Advance past the 7-day recall window and resolve → the effect applies.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY + 8 * DAY));
    f.services.governance().resolve_due("town-square");
    assert!(
        f.store.find_by_slug("town-square").unwrap().unwrap().is_rehoming_disabled,
        "citizens' vote disabled automatic rehoming"
    );
}

/// A RuleChange to the jury-sizing rule (the "trials" setting) applies after its
/// voting window — no chartering or timelock required.
#[test]
fn citizens_can_change_jury_sizing_for_trials() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    assert!(
        matches!(f.store.find_by_slug("town-square").unwrap().unwrap().jury_sizing, domain::JurySizing::Sqrt { .. }),
        "a new server defaults to square-root jury sizing"
    );

    let p = f
        .services.governance()
        .open_proposal(
            "ada",
            "town-square",
            ProposalKind::SetJurySizing { sizing: domain::JurySizing::Fixed { post: 12, comment: 6 } },
        )
        .unwrap();
    for h in ["ada", "bob", "cid"] {
        f.services.governance().cast_vote(h, p.id.0, true).unwrap();
    }
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");

    assert_eq!(
        f.store.find_by_slug("town-square").unwrap().unwrap().jury_sizing,
        domain::JurySizing::Fixed { post: 12, comment: 6 },
        "citizens' vote resized the trial jury"
    );
}

/// Citizens can vote to change *what the server votes on* — enabling a ballot kind
/// (here vote-weighting governance) that was not on the default surface.
#[test]
fn citizens_can_change_what_the_server_votes_on() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    let before = f.store.find_by_slug("town-square").unwrap().unwrap();
    assert!(!before.governs(BallotKind::SetVoteWeighting), "vote-weighting governance is opt-in");

    let mut surface = before.enabled_ballots.clone();
    surface.insert(BallotKind::SetVoteWeighting);
    surface.insert(BallotKind::SetWeightingScope);
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::SetGovernanceSurface { enabled: surface })
        .unwrap();
    for h in ["ada", "bob", "cid"] {
        f.services.governance().cast_vote(h, p.id.0, true).unwrap();
    }
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");

    assert!(
        f.store.find_by_slug("town-square").unwrap().unwrap().governs(BallotKind::SetVoteWeighting),
        "the surface now includes vote-weighting governance"
    );
}

/// Amending the franchise criteria (who may vote) is Constitutional: it needs a
/// chartered electorate and matures only after the recall timelock.
#[test]
fn citizens_can_amend_who_may_vote() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("ada").unwrap();
    f.services.found_server("ada", "Town Square").unwrap();
    let voters = ["bob", "cid", "dan", "eve", "fay", "gus", "hal", "ike", "jan"];
    for h in voters {
        seat_citizen(&f, h, "town-square", 5);
    }
    let proposed =
        domain::FranchiseCriteria { min_account_age_days: 60, min_membership_days: 30, min_contribution: 10 };

    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AmendCriteria { proposed: proposed.clone() })
        .unwrap();
    for h in ["ada", "bob", "cid", "dan", "eve", "fay", "gus", "hal", "ike", "jan"] {
        f.services.governance().cast_vote(h, p.id.0, true).unwrap();
    }

    // Passes at window close, but a Constitutional change is timelocked.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");
    assert_ne!(
        f.store.find_by_slug("town-square").unwrap().unwrap().criteria.min_account_age_days, 60,
        "not yet effective during the recall timelock"
    );

    // Past the recall window → the new criteria take effect.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY + 8 * DAY));
    f.services.governance().resolve_due("town-square");
    assert_eq!(
        f.store.find_by_slug("town-square").unwrap().unwrap().criteria, proposed,
        "citizens' vote amended who may earn the franchise"
    );
}

#[test]
fn an_amended_bundle_enacts_all_its_changes_together() {
    let f = fixture(1_000 * DAY);
    setup(&f);

    // A proposal to add one rule, then amended to also create a channel: two
    // changes riding one ballot.
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "Be excellent".into() })
        .unwrap();
    f.services.governance()
        .amend_proposal("bob", p.id.0, ProposalKind::CreateChannel { name: "lounge".into(), topic: "".into() })
        .unwrap();

    // One vote decides the whole bundle.
    f.services.governance().cast_vote("ada", p.id.0, true).unwrap();
    f.services.governance().cast_vote("bob", p.id.0, true).unwrap();
    f.services.governance().cast_vote("cid", p.id.0, true).unwrap();

    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");

    // Both the primary change and the amendment took effect.
    assert_eq!(f.services.governance().list_rules("town-square").len(), 1, "the rule was added");
    let channels = f.services.chat().list_channels("town-square").unwrap();
    assert!(channels.iter().any(|c| c.name == "lounge"), "the amendment's channel was created");
}

#[test]
fn a_defeated_bundle_enacts_none_of_its_changes() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "No fun".into() })
        .unwrap();
    f.services.governance()
        .amend_proposal("ada", p.id.0, ProposalKind::CreateChannel { name: "lounge".into(), topic: "".into() })
        .unwrap();
    // The bundle fails its threshold.
    f.services.governance().cast_vote("ada", p.id.0, true).unwrap();
    f.services.governance().cast_vote("bob", p.id.0, false).unwrap();
    f.services.governance().cast_vote("cid", p.id.0, false).unwrap();

    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");

    assert!(f.services.governance().list_rules("town-square").is_empty(), "rule not added");
    let channels = f.services.chat().list_channels("town-square").unwrap();
    assert!(!channels.iter().any(|c| c.name == "lounge"), "amendment's channel not created");
    assert_eq!(f.store_proposals()[0].status, ProposalStatus::Failed);
}

#[test]
fn a_closed_proposal_takes_no_amendments() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "Be excellent".into() })
        .unwrap();
    // Close the window.
    f.clock.set(Timestamp(1_000 * DAY + 4 * DAY));
    f.services.governance().resolve_due("town-square");

    let err = f
        .services.governance()
        .amend_proposal("bob", p.id.0, ProposalKind::AddRule { text: "too late".into() })
        .unwrap_err();
    assert_eq!(err, app::ProposeError::Closed);
}

#[test]
fn only_a_citizen_may_join_the_debate() {
    let f = fixture(1_000 * DAY);
    setup(&f);
    // A non-citizen member.
    f.services.register_account("newbie").unwrap();
    f.services.join_server("newbie", "town-square").unwrap();

    let p = f
        .services.governance()
        .open_proposal("ada", "town-square", ProposalKind::AddRule { text: "Be excellent".into() })
        .unwrap();

    assert_eq!(
        f.services.governance().post_discussion("newbie", p.id.0, "aye!").unwrap_err(),
        app::VoteError::NotACitizen
    );
    f.services.governance().post_discussion("ada", p.id.0, "I think aye.").unwrap();
    let thread = f.services.governance().list_discussion(p.id.0);
    assert_eq!(thread.len(), 1);
    assert_eq!(thread[0].body, "I think aye.");
}

impl Fixture {
    fn store_proposals(&self) -> Vec<domain::Proposal> {
        use app::ProposalStore;
        let server = self.store.find_by_slug("town-square").unwrap().unwrap();
        ProposalStore::list_for_server(&*self.store, server.id).unwrap()
    }
}
