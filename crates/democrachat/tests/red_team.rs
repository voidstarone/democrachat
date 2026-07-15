//! Adversarial ("red team") tests: deliberate attempts to break the platform's
//! security and governance guarantees. The headline threat is **capture by
//! numbers** — an attacker who floods a server with new members (or even with
//! qualified members) must not be able to seize the franchise, the vote, or any
//! moderation power. Every test here tries to do exactly that and asserts it
//! fails.
//!
//! The franchise is the crown jewel: it can only ever be *earned* (Layer 1
//! criteria) and only ever admitted at a bounded *rate* (Layer 2 cap), decisions
//! need real *quorum + approval* among established citizens (Layer 3), and there
//! is no admin/founder/role bypass anywhere. These tests pin those invariants.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{
    Clock, DmError, EnfranchiseError, EnfranchiseOutcome, FoundError, MembershipStore, MessageError,
    MuteError, ProposeError, ServerStore, Services, SessionSigner, SocialError, UserStore, VoteError,
};
use domain::{
    DmPolicy, FranchiseCriteria, HistoryMode, ProposalKind, ProposalStatus, Tier, Timestamp, Unmet,
};

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

fn found(f: &Fixture, founder: &str, name: &str) {
    f.services.register_account(founder).unwrap();
    f.services.found_server(founder, name).unwrap();
}

fn register_and_join(f: &Fixture, handle: &str, slug: &str) {
    f.services.register_account(handle).unwrap();
    f.services.join_server(handle, slug).unwrap();
}

/// Directly seat an enfranchised citizen (a *test* shortcut on the store — there
/// is no such use-case in production). Used only to stand up an established
/// electorate the red team then tries to overwhelm.
fn seat_citizen(f: &Fixture, handle: &str, slug: &str) {
    register_and_join(f, handle, slug);
    let u = f.store.find_by_handle(handle).unwrap();
    let s = f.store.find_by_slug(slug).unwrap();
    let mut m = f.store.get(u.id, s.id).unwrap();
    m.tier = Tier::Citizen;
    m.contribution = 5;
    m.enfranchised_at = Some(f.clock.now());
    f.store.upsert(m);
}

fn set_contribution(f: &Fixture, handle: &str, slug: &str, contribution: i64) {
    let u = f.store.find_by_handle(handle).unwrap();
    let s = f.store.find_by_slug(slug).unwrap();
    let mut m = f.store.get(u.id, s.id).unwrap();
    m.contribution = contribution;
    f.store.upsert(m);
}

fn citizen_count(f: &Fixture, slug: &str) -> u64 {
    let s = f.store.find_by_slug(slug).unwrap();
    f.store.citizen_count(s.id)
}

// ─────────────────────────────────────────────────────────────────────────────
// A. Capture by numbers — the franchise
// ─────────────────────────────────────────────────────────────────────────────

/// A flood of freshly-registered members clears no franchise criterion, so not
/// one of them can enfranchise — numbers alone buy nothing.
#[test]
fn a_flood_of_fresh_members_cannot_enfranchise() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    for i in 0..200 {
        register_and_join(&f, &format!("mob{i}"), "town");
    }
    let mut admitted = 0;
    for i in 0..200 {
        // Fresh accounts: too young, too new to the server, zero contribution.
        match f.services.try_enfranchise(&format!("mob{i}"), "town").unwrap() {
            EnfranchiseOutcome::NotEligible(unmet) => {
                assert!(unmet.contains(&Unmet::AccountTooYoung { need_days: 30, have_days: 0 }));
            }
            EnfranchiseOutcome::Admitted => admitted += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(admitted, 0, "no fresh member is ever admitted");
    assert_eq!(citizen_count(&f, "town"), 1, "the founder remains the sole citizen");
}

/// Even a flood of *fully qualified* members cannot swamp an established
/// electorate: Layer 2's rate cap admits only ~10% of the roll per 30-day window,
/// so 300 qualified newcomers add a handful of citizens, not a majority.
#[test]
fn the_rate_cap_bounds_a_flood_of_qualified_members() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    // Stand up an established electorate of 100 citizens (founder + 99).
    for i in 0..99 {
        seat_citizen(&f, &format!("cit{i}"), "town");
    }
    assert_eq!(citizen_count(&f, "town"), 100);

    // 300 attackers join now, then wait out the age criteria and earn contribution.
    for i in 0..300 {
        register_and_join(&f, &format!("mob{i}"), "town");
    }
    f.clock.set(Timestamp(1_040 * DAY)); // +40d: clears account age (30) & dwell (14)
    for i in 0..300 {
        set_contribution(&f, &format!("mob{i}"), "town", 5);
    }

    let mut admitted = 0;
    let mut capped = 0;
    for i in 0..300 {
        match f.services.try_enfranchise(&format!("mob{i}"), "town").unwrap() {
            EnfranchiseOutcome::Admitted => admitted += 1,
            EnfranchiseOutcome::RateCapped { .. } => capped += 1,
            other => panic!("a qualified member should be admitted or capped, got {other:?}"),
        }
    }
    // ~10% of 100 in one window (a touch more, as each admission grows the roll).
    assert!(
        (10..=13).contains(&admitted),
        "the flood is bounded to roughly a tenth of the electorate, got {admitted}"
    );
    assert_eq!(capped, 300 - admitted, "everyone else is delayed, never denied");
    // The attackers remain a tiny minority of the electorate.
    assert_eq!(citizen_count(&f, "town"), 100 + admitted);
    assert!(citizen_count(&f, "town") < 114, "the mob cannot reach a majority in one move");
}

/// Defence in depth: Layer 2 is independent of Layer 1. Even if a server's
/// criteria are trivialized to nothing (so every fresh account "qualifies"), the
/// rate cap still bounds admissions — a 1-citizen server admits only its floor of
/// 5 per window, no matter how large the flood.
#[test]
fn the_rate_cap_holds_even_when_criteria_are_trivialized() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    // Simulate a maximally-open constitution (as if every criterion were amended
    // to zero). Layer 2 must still hold on its own.
    let mut s = f.store.find_by_slug("town").unwrap();
    s.criteria = FranchiseCriteria { min_account_age_days: 0, min_membership_days: 0, min_contribution: 0 };
    f.store.update_server(s);

    for i in 0..100 {
        register_and_join(&f, &format!("mob{i}"), "town");
    }
    // Age the founding out of the rate-cap window so we isolate a clean floor (the
    // founder's own enfranchisement at t0 would otherwise consume a slot).
    f.clock.set(Timestamp(1_040 * DAY));
    let mut admitted = 0;
    for i in 0..100 {
        match f.services.try_enfranchise(&format!("mob{i}"), "town").unwrap() {
            EnfranchiseOutcome::Admitted => admitted += 1,
            EnfranchiseOutcome::RateCapped { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(admitted, 5, "the floor of 5/window governs a tiny server even with open criteria");
}

/// The other side of the coin: no matter how many members exist, none of them can
/// *vote*. A ballot's electorate is citizens only, so a flood of members has zero
/// weight on any decision.
#[test]
fn members_cannot_vote_no_matter_how_many() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    let p = f
        .services
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "no capture".into() })
        .unwrap();

    for i in 0..250 {
        register_and_join(&f, &format!("mob{i}"), "town");
        assert_eq!(
            f.services.cast_vote(&format!("mob{i}"), p.id.0, true),
            Err(VoteError::NotACitizen),
            "a mere member can never cast a ballot"
        );
    }
    // Not one mob ballot registered.
    assert_eq!(f.services.proposal_head_counts(p.id.0), (0, 0));
}

/// A member cannot even open a proposal — the agenda itself is citizens-only.
#[test]
fn members_cannot_open_proposals() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    register_and_join(&f, "nobody", "town");
    assert_eq!(
        f.services.open_proposal("nobody", "town", ProposalKind::AddRule { text: "x".into() }),
        Err(ProposeError::NotACitizen),
    );
}

/// A lone (or minority) citizen cannot ram a moderation ballot through a full
/// electorate: quorum is measured against *all* established citizens, so one aye
/// out of a hundred fails, and the target is never sanctioned.
#[test]
fn a_lone_citizen_cannot_pass_a_capture_ballot() {
    let f = fixture(1_000 * DAY);
    found(&f, "founder", "Town");
    for i in 0..99 {
        seat_citizen(&f, &format!("cit{i}"), "town"); // 100 honest citizens total
    }
    seat_citizen(&f, "attacker", "town"); // + 1 attacker citizen = 101
    let victim = f.store.find_by_handle("cit0").unwrap();

    let p = f
        .services
        .open_proposal("attacker", "town", ProposalKind::Ban { user: victim.id })
        .unwrap();
    f.services.cast_vote("attacker", p.id.0, true).unwrap(); // votes alone

    f.clock.set(Timestamp(1_000 * DAY + 5 * DAY)); // past the 3-day window
    f.services.resolve_due("town");

    let closed = f.services.list_proposals("town").into_iter().find(|x| x.id == p.id).unwrap();
    assert_eq!(closed.status, ProposalStatus::Failed, "one vote of 101 fails quorum");
    let server = f.store.find_by_slug("town").unwrap();
    assert!(
        !f.store.get(victim.id, server.id).unwrap().is_sanctioned,
        "the target keeps their standing — no capture"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// B. No backdoor into the franchise
// ─────────────────────────────────────────────────────────────────────────────

/// There is no "admit my friend" path: `try_enfranchise` refuses an unqualified
/// member even when the founder wants them in.
#[test]
fn there_is_no_founder_override_of_the_criteria() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    register_and_join(&f, "buddy", "town");
    match f.services.try_enfranchise("buddy", "town").unwrap() {
        EnfranchiseOutcome::NotEligible(_) => {}
        other => panic!("an unqualified member must not be admitted, got {other:?}"),
    }
    assert!(!f.store.get(
        f.store.find_by_handle("buddy").unwrap().id,
        f.store.find_by_slug("town").unwrap().id,
    ).unwrap().is_citizen());
}

/// A franchise-barred puppet account (e.g. a staff/content bot) can never be
/// enfranchised, even when every ordinary criterion is satisfied.
#[test]
fn a_franchise_barred_puppet_never_gets_in() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    register_and_join(&f, "puppet", "town");
    // Bar the account and otherwise fully qualify it.
    let mut u = f.store.find_by_handle("puppet").unwrap();
    u.is_franchise_barred = true;
    f.store.update_user(u);
    f.clock.set(Timestamp(1_050 * DAY));
    set_contribution(&f, "puppet", "town", 50);

    match f.services.try_enfranchise("puppet", "town").unwrap() {
        EnfranchiseOutcome::NotEligible(unmet) => assert_eq!(unmet, vec![Unmet::Barred]),
        other => panic!("a barred account must be refused, got {other:?}"),
    }
}

/// A barred account cannot even found a server (which would seat it as citizen #1).
#[test]
fn a_barred_account_cannot_found_a_server() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("villain").unwrap();
    let mut u = f.store.find_by_handle("villain").unwrap();
    u.is_franchise_barred = true;
    f.store.update_user(u);
    assert_eq!(f.services.found_server("villain", "Lair"), Err(FoundError::FounderBarred));
}

/// Ballot weight is not a side door into the franchise: even a member handed a
/// huge vote weight (as a passed `GrantVoteWeight` would) still cannot vote,
/// because they are not a citizen.
#[test]
fn granting_vote_weight_to_a_non_citizen_does_not_let_them_vote() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    register_and_join(&f, "member", "town");
    // Simulate the effect of a passed GrantVoteWeight ballot on a non-citizen.
    let u = f.store.find_by_handle("member").unwrap();
    let s = f.store.find_by_slug("town").unwrap();
    let mut m = f.store.get(u.id, s.id).unwrap();
    m.granted_weight = 1_000;
    f.store.upsert(m);

    let p = f
        .services
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .unwrap();
    assert_eq!(
        f.services.cast_vote("member", p.id.0, true),
        Err(VoteError::NotACitizen),
        "weight without citizenship is inert"
    );
}

/// A sanctioned citizen is stripped of the franchise: they may not vote, so a
/// captured-then-banned account can't keep voting.
#[test]
fn a_sanctioned_citizen_cannot_vote() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    seat_citizen(&f, "rogue", "town");
    // Sanction them (as a passed Ban would).
    let u = f.store.find_by_handle("rogue").unwrap();
    let s = f.store.find_by_slug("town").unwrap();
    let mut m = f.store.get(u.id, s.id).unwrap();
    m.is_sanctioned = true;
    f.store.upsert(m);

    let p = f
        .services
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .unwrap();
    assert_eq!(f.services.cast_vote("rogue", p.id.0, true), Err(VoteError::NotACitizen));
}

// ─────────────────────────────────────────────────────────────────────────────
// C. Ballot-process abuse
// ─────────────────────────────────────────────────────────────────────────────

/// One citizen, one ballot: hammering `cast_vote` doesn't stuff the box — the
/// vote is upserted, so a citizen's tenth aye is still just one aye.
#[test]
fn a_citizen_cannot_stuff_the_ballot_box() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    let p = f
        .services
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .unwrap();
    for _ in 0..10 {
        f.services.cast_vote("boss", p.id.0, true).unwrap();
    }
    assert_eq!(f.services.proposal_head_counts(p.id.0), (1, 0), "ten ayes collapse to one");
}

/// Votes cast after the window has closed are rejected — no last-second reversal
/// once a ballot has been resolved.
#[test]
fn votes_after_the_window_are_rejected() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    let p = f
        .services
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .unwrap();
    f.clock.set(Timestamp(1_000 * DAY + 5 * DAY));
    f.services.resolve_due("town"); // closes the ballot
    assert_eq!(f.services.cast_vote("boss", p.id.0, true), Err(VoteError::Closed));
}

/// Training wheels: a server in the Seed phase cannot amend its own constitution
/// (franchise criteria), so a founder can't lower the bar to zero before the
/// electorate is large enough for the percentage math to protect it.
#[test]
fn the_constitution_cannot_be_amended_during_seed() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town"); // 1 citizen → Seed phase
    let trivial =
        FranchiseCriteria { min_account_age_days: 0, min_membership_days: 0, min_contribution: 0 };
    assert_eq!(
        f.services.open_proposal("boss", "town", ProposalKind::AmendCriteria { proposed: trivial }),
        Err(ProposeError::NotAllowedInPhase),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// D. Session-cookie forgery
// ─────────────────────────────────────────────────────────────────────────────

/// A stolen-and-tampered session cookie is rejected: the HMAC binds uid+expiry,
/// so neither a swapped uid, an extended expiry, nor a flipped MAC bit verifies.
#[test]
fn a_forged_session_cookie_is_rejected() {
    let signer = SessionSigner::from_secret("a-sufficiently-long-shared-secret");
    let token = signer.sign(7, 2_000);
    let mac = token.rsplit('.').next().unwrap().to_string();

    // Genuine token still verifies.
    assert_eq!(signer.verify(&token), Some((7, 2_000)));
    // Impersonate another account by swapping the uid but keeping the MAC.
    assert_eq!(signer.verify(&format!("42.2000.{mac}")), None);
    // Extend the session's life.
    assert_eq!(signer.verify(&format!("7.99999999999.{mac}")), None);
    // Flip the last hex nibble of the MAC.
    let mut bytes = mac.into_bytes();
    let last = bytes.last_mut().unwrap();
    *last = if *last == b'0' { b'1' } else { b'0' };
    let flipped = String::from_utf8(bytes).unwrap();
    assert_eq!(signer.verify(&format!("7.2000.{flipped}")), None);
}

/// A cookie minted under a different secret (a foreign or guessed key) never
/// verifies — sessions aren't portable across the fleet secret.
#[test]
fn a_cookie_from_a_foreign_secret_is_rejected() {
    let real = SessionSigner::from_secret("the-real-server-secret-value-x");
    let attacker = SessionSigner::from_secret("the-attackers-own-secret-value");
    let forged = attacker.sign(1, 5_000);
    assert_eq!(real.verify(&forged), None);
    // Structurally-plausible garbage is rejected too.
    assert_eq!(real.verify("1.5000.deadbeef"), None);
    assert_eq!(real.verify("1.5000."), None);
    assert_eq!(real.verify(""), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// E. Silencing, harassment, and channel-downgrade attacks
// ─────────────────────────────────────────────────────────────────────────────

/// A banned (sanctioned) member is silenced: their posts are refused, without
/// their history being deleted. So capturing then banning a critic stops them
/// speaking — and equally, a sanction can't be evaded by just posting anyway.
#[test]
fn a_sanctioned_member_cannot_post() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");    register_and_join(&f, "loudmouth", "town");
    // They can post before sanction.
    f.services.post_message("loudmouth", "town", "general", "hello").unwrap();
    // Sanction them (as a passed Ban ballot would).
    let u = f.store.find_by_handle("loudmouth").unwrap();
    let s = f.store.find_by_slug("town").unwrap();
    let mut m = f.store.get(u.id, s.id).unwrap();
    m.is_sanctioned = true;
    f.store.upsert(m);

    assert_eq!(
        f.services.post_message("loudmouth", "town", "general", "again"),
        Err(MessageError::Sanctioned("loudmouth".into())),
    );
}

/// A non-member cannot post into a server's channels by numbers or otherwise —
/// membership is required to speak.
#[test]
fn a_non_member_cannot_post() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");    f.services.register_account("outsider").unwrap(); // registered but never joined
    assert_eq!(
        f.services.post_message("outsider", "town", "general", "let me in"),
        Err(MessageError::NotAMember("outsider".into())),
    );
}

/// No plaintext downgrade: once a channel is encrypted, a plaintext post is
/// refused (the client must seal under the channel key). An attacker cannot leak
/// a would-be-private message onto the plaintext feed.
#[test]
fn plaintext_cannot_be_posted_to_an_encrypted_channel() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    f.services.create_channel("boss", "town", "secret", "").unwrap();
    f.services.enable_channel_encryption("boss", "town", "secret", HistoryMode::Ephemeral).unwrap();
    assert_eq!(
        f.services.post_message("boss", "town", "secret", "leak"),
        Err(MessageError::ChannelEncrypted),
    );
}

/// A permanent block cannot be worked around: once blocked, the blocked user can
/// never DM the blocker (there is no unblock, and the block overrides any DM
/// policy). Harassment by re-contact is shut down.
#[test]
fn a_blocked_user_can_never_dm_the_blocker() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("victim").unwrap();
    f.services.register_account("harasser").unwrap();
    // Open DMs by default, so it's the block — not a policy — that stops them.
    f.services.send_sealed_dm("harasser", "victim", "aa", "bb").unwrap();
    f.services.block_user("victim", "harasser").unwrap();
    assert_eq!(
        f.services.send_sealed_dm("harasser", "victim", "aa", "bb"),
        Err(DmError::NotAllowed),
        "a block is a hard, permanent wall",
    );
    assert!(!f.services.can_dm("harasser", "victim"));
}

/// A friends-only inbox refuses strangers: a spammer cannot reach a user who has
/// closed their DMs, but a genuine accepted friend can.
#[test]
fn a_friends_only_inbox_refuses_strangers() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("target").unwrap();
    f.services.register_account("spammer").unwrap();
    f.services.register_account("pal").unwrap();
    f.services.set_dm_policy("target", DmPolicy::FriendsOnly).unwrap();

    assert_eq!(
        f.services.send_sealed_dm("spammer", "target", "aa", "bb"),
        Err(DmError::NotAllowed),
        "a stranger can't slip into a friends-only inbox",
    );
    // A mutual friend gets through.
    f.services.request_friend("pal", "target").unwrap();
    f.services.accept_friend("target", "pal").unwrap();
    assert!(f.services.send_sealed_dm("pal", "target", "aa", "bb").is_ok());
}

/// Self-directed social actions are refused — you can't block, befriend, or DM
/// yourself (a class of nonsense/abuse input the surface must reject).
#[test]
fn self_directed_social_actions_are_refused() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("solo").unwrap();
    assert_eq!(f.services.block_user("solo", "solo"), Err(SocialError::Self_));
    assert_eq!(f.services.request_friend("solo", "solo"), Err(SocialError::Self_));
    assert_eq!(f.services.send_sealed_dm("solo", "solo", "aa", "bb"), Err(DmError::Self_));
}

/// An invite grants membership, never the franchise — even at scale. A thousand
/// invited accounts are a thousand voteless members; the electorate is untouched.
#[test]
fn mass_invites_never_grant_the_franchise() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("host").unwrap();
    f.services.found_server_with_visibility("host", "Club", true).unwrap();
    let code = f.services.create_invite("host", "club").unwrap();
    for i in 0..1_000 {
        f.services.register_account(&format!("guest{i}")).unwrap();
        let m = f.services.accept_invite(&format!("guest{i}"), &code).unwrap();
        assert!(!m.is_citizen(), "an invite never seats a citizen");
    }
    assert_eq!(citizen_count(&f, "club"), 1, "only the founder can vote");
}

// ─────────────────────────────────────────────────────────────────────────────
// F. Cross-server isolation — standing in one server buys nothing in another
// ─────────────────────────────────────────────────────────────────────────────
//
// Every authority in the system is scoped to a single server: citizenship, the
// agenda, the floor to speak, and police powers. An attacker who has *earned*
// standing in one community must not be able to spend it in another. These tests
// stand up two servers and try to reach across.

/// Citizenship is per-server: a citizen of one server has no ballot in another's
/// decision, so their vote is refused and never counted.
#[test]
fn a_citizen_of_one_server_cannot_vote_in_another() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss_a", "Alpha");
    found(&f, "boss_b", "Beta");
    seat_citizen(&f, "alice", "alpha"); // a citizen of Alpha only

    let p = f
        .services
        .open_proposal("boss_b", "beta", ProposalKind::AddRule { text: "beta rule".into() })
        .unwrap();
    assert_eq!(
        f.services.cast_vote("alice", p.id.0, true),
        Err(VoteError::NotACitizen),
        "Alpha citizenship carries no vote in Beta",
    );
    assert_eq!(f.services.proposal_head_counts(p.id.0), (0, 0), "no cross-server ballot lands");
}

/// The agenda is per-server too: a citizen of one server cannot open a proposal in
/// a server they do not belong to.
#[test]
fn a_citizen_of_one_server_cannot_open_a_proposal_in_another() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss_a", "Alpha");
    found(&f, "boss_b", "Beta");
    seat_citizen(&f, "alice", "alpha");
    assert_eq!(
        f.services.open_proposal("alice", "beta", ProposalKind::AddRule { text: "x".into() }),
        Err(ProposeError::NotACitizen),
    );
}

/// Membership is per-server: a member of one server cannot post into another
/// server's channels — no speaking across a wall you never joined.
#[test]
fn a_member_of_one_server_cannot_post_in_another() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss_a", "Alpha");
    found(&f, "boss_b", "Beta");
    register_and_join(&f, "alice", "alpha");
    assert_eq!(
        f.services.post_message("alice", "beta", "general", "hello from Alpha"),
        Err(MessageError::NotAMember("alice".into())),
    );
}

/// Police powers are per-server: an officer sworn in one server has no moderation
/// authority in another and cannot mute there.
#[test]
fn police_powers_do_not_cross_servers() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss_a", "Alpha");
    found(&f, "boss_b", "Beta");
    // Make alice a police officer of Alpha (store shortcut — a passed AppointPolice
    // ballot would do the same in Alpha, and none of it reaches Beta).
    seat_citizen(&f, "alice", "alpha");
    let ua = f.store.find_by_handle("alice").unwrap();
    let sa = f.store.find_by_slug("alpha").unwrap();
    let mut m = f.store.get(ua.id, sa.id).unwrap();
    m.is_police = true;
    f.store.upsert(m);
    // A target who is a genuine member of Beta.
    register_and_join(&f, "victim", "beta");

    assert_eq!(
        f.services.mute_member("alice", "beta", "victim"),
        Err(MuteError::NotPolice),
        "an Alpha officer has no badge in Beta",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// G. Replay / idempotency — an action applies once, not once per attempt
// ─────────────────────────────────────────────────────────────────────────────

/// Sweeping the due ballots is idempotent: a passed rule enacts exactly once, and
/// repeated sweeps neither duplicate it nor re-open the closed ballot. An attacker
/// cannot amplify a single win by forcing extra resolutions.
#[test]
fn a_passed_ballot_applies_exactly_once() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    seat_citizen(&f, "cit1", "town");
    seat_citizen(&f, "cit2", "town"); // 3 citizens — a real electorate

    let p = f
        .services
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "no capture".into() })
        .unwrap();
    for c in ["boss", "cit1", "cit2"] {
        f.services.cast_vote(c, p.id.0, true).unwrap();
    }
    f.clock.set(Timestamp(1_000 * DAY + 5 * DAY)); // past the voting window
    f.services.resolve_due("town");
    let closed = f.services.list_proposals("town").into_iter().find(|x| x.id == p.id).unwrap();
    assert!(matches!(closed.status, ProposalStatus::Passed { .. }));
    assert_eq!(f.services.list_rules("town").len(), 1, "the rule enacts on the first sweep");

    // Hammer the resolver: a closed ballot is never re-applied.
    f.services.resolve_due("town");
    f.services.resolve_due("town");
    assert_eq!(f.services.list_rules("town").len(), 1, "extra sweeps enact nothing further");
}

/// An already-enfranchised citizen cannot be enfranchised again — no double-seating
/// and no second rate-cap slot burned to re-admit someone already in.
#[test]
fn a_citizen_cannot_be_enfranchised_twice() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    seat_citizen(&f, "cit", "town");
    assert!(matches!(
        f.services.try_enfranchise("cit", "town"),
        Err(EnfranchiseError::AlreadyCitizen(_)),
    ));
    assert_eq!(citizen_count(&f, "town"), 2, "the roll is unchanged by the re-attempt");
}

/// Enfranchisement requires membership: a registered account that never joined the
/// server cannot be admitted to its franchise.
#[test]
fn a_non_member_cannot_be_enfranchised() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town");
    f.services.register_account("outsider").unwrap(); // registered, never joined
    assert!(matches!(
        f.services.try_enfranchise("outsider", "town"),
        Err(EnfranchiseError::NotAMember(_)),
    ));
}
