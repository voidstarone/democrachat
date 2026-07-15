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
    ChannelKeyError, Clock, DmError, EmojiError, EnfranchiseError, EnfranchiseOutcome, FoundError,
    IdentitySecret, KeyError, MembershipStore, MessageError, MuteError, ProposeError, ServerStore,
    Services, SessionSigner, SocialError, UserStore, VoteError, wrap_secret,
};
use domain::{
    DmPolicy, FranchiseCriteria, HistoryMode, ProposalKind, ProposalStatus, Tier, Timestamp, Unmet,
    WrappedKey,
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

async fn found(f: &Fixture, founder: &str, name: &str) {
    f.services.register_account(founder).await.unwrap();
    f.services.found_server(founder, name).await.unwrap();
}

async fn register_and_join(f: &Fixture, handle: &str, slug: &str) {
    f.services.register_account(handle).await.unwrap();
    f.services.join_server(handle, slug).await.unwrap();
}

/// Directly seat an enfranchised citizen (a *test* shortcut on the store — there
/// is no such use-case in production). Used only to stand up an established
/// electorate the red team then tries to overwhelm.
async fn seat_citizen(f: &Fixture, handle: &str, slug: &str) {
    register_and_join(f, handle, slug).await;
    let u = f.store.find_by_handle(handle).await.unwrap().unwrap();
    let s = f.store.find_by_slug(slug).await.unwrap().unwrap();
    let mut m = f.store.get(u.id, s.id).await.unwrap().unwrap();
    m.tier = Tier::Citizen;
    m.contribution = 5;
    m.enfranchised_at = Some(f.clock.now());
    f.store.upsert(m).await.unwrap();
}

async fn set_contribution(f: &Fixture, handle: &str, slug: &str, contribution: i64) {
    let u = f.store.find_by_handle(handle).await.unwrap().unwrap();
    let s = f.store.find_by_slug(slug).await.unwrap().unwrap();
    let mut m = f.store.get(u.id, s.id).await.unwrap().unwrap();
    m.contribution = contribution;
    f.store.upsert(m).await.unwrap();
}

async fn citizen_count(f: &Fixture, slug: &str) -> u64 {
    let s = f.store.find_by_slug(slug).await.unwrap().unwrap();
    f.store.citizen_count(s.id).await.unwrap()
}

// ─────────────────────────────────────────────────────────────────────────────
// A. Capture by numbers — the franchise
// ─────────────────────────────────────────────────────────────────────────────

/// A flood of freshly-registered members clears no franchise criterion, so not
/// one of them can enfranchise — numbers alone buy nothing.
#[tokio::test]
async fn a_flood_of_fresh_members_cannot_enfranchise() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    for i in 0..200 {
        register_and_join(&f, &format!("mob{i}"), "town").await;
    }
    let mut admitted = 0;
    for i in 0..200 {
        // Fresh accounts: too young, too new to the server, zero contribution.
        match f.services.try_enfranchise(&format!("mob{i}"), "town").await.unwrap() {
            EnfranchiseOutcome::NotEligible(unmet) => {
                assert!(unmet.contains(&Unmet::AccountTooYoung { need_days: 30, have_days: 0 }));
            }
            EnfranchiseOutcome::Admitted => admitted += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(admitted, 0, "no fresh member is ever admitted");
    assert_eq!(citizen_count(&f, "town").await, 1, "the founder remains the sole citizen");
}

/// Even a flood of *fully qualified* members cannot swamp an established
/// electorate: Layer 2's rate cap admits only ~10% of the roll per 30-day window,
/// so 300 qualified newcomers add a handful of citizens, not a majority.
#[tokio::test]
async fn the_rate_cap_bounds_a_flood_of_qualified_members() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    // Stand up an established electorate of 100 citizens (founder + 99).
    for i in 0..99 {
        seat_citizen(&f, &format!("cit{i}"), "town").await;
    }
    assert_eq!(citizen_count(&f, "town").await, 100);

    // 300 attackers join now, then wait out the age criteria and earn contribution.
    for i in 0..300 {
        register_and_join(&f, &format!("mob{i}"), "town").await;
    }
    f.clock.set(Timestamp(1_040 * DAY)); // +40d: clears account age (30) & dwell (14)
    for i in 0..300 {
        set_contribution(&f, &format!("mob{i}"), "town", 5).await;
    }

    let mut admitted = 0;
    let mut capped = 0;
    for i in 0..300 {
        match f.services.try_enfranchise(&format!("mob{i}"), "town").await.unwrap() {
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
    assert_eq!(citizen_count(&f, "town").await, 100 + admitted);
    assert!(citizen_count(&f, "town").await < 114, "the mob cannot reach a majority in one move");
}

/// Defence in depth: Layer 2 is independent of Layer 1. Even if a server's
/// criteria are trivialized to nothing (so every fresh account "qualifies"), the
/// rate cap still bounds admissions — a 1-citizen server admits only its floor of
/// 5 per window, no matter how large the flood.
#[tokio::test]
async fn the_rate_cap_holds_even_when_criteria_are_trivialized() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    // Simulate a maximally-open constitution (as if every criterion were amended
    // to zero). Layer 2 must still hold on its own.
    let mut s = f.store.find_by_slug("town").await.unwrap().unwrap();
    s.criteria = FranchiseCriteria { min_account_age_days: 0, min_membership_days: 0, min_contribution: 0 };
    f.store.update_server(s).await.unwrap();

    for i in 0..100 {
        register_and_join(&f, &format!("mob{i}"), "town").await;
    }
    // Age the founding out of the rate-cap window so we isolate a clean floor (the
    // founder's own enfranchisement at t0 would otherwise consume a slot).
    f.clock.set(Timestamp(1_040 * DAY));
    let mut admitted = 0;
    for i in 0..100 {
        match f.services.try_enfranchise(&format!("mob{i}"), "town").await.unwrap() {
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
#[tokio::test]
async fn members_cannot_vote_no_matter_how_many() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    let p = f
        .services.governance()
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "no capture".into() })
        .await
        .unwrap();

    for i in 0..250 {
        register_and_join(&f, &format!("mob{i}"), "town").await;
        assert_eq!(
            f.services.governance().cast_vote(&format!("mob{i}"), p.id.0, true).await,
            Err(VoteError::NotACitizen),
            "a mere member can never cast a ballot"
        );
    }
    // Not one mob ballot registered.
    assert_eq!(f.services.governance().proposal_head_counts(p.id.0).await, (0, 0));
}

/// A member cannot even open a proposal — the agenda itself is citizens-only.
#[tokio::test]
async fn members_cannot_open_proposals() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    register_and_join(&f, "nobody", "town").await;
    assert_eq!(
        f.services.governance().open_proposal("nobody", "town", ProposalKind::AddRule { text: "x".into() }).await,
        Err(ProposeError::NotACitizen),
    );
}

/// A lone (or minority) citizen cannot ram a moderation ballot through a full
/// electorate: quorum is measured against *all* established citizens, so one aye
/// out of a hundred fails, and the target is never sanctioned.
#[tokio::test]
async fn a_lone_citizen_cannot_pass_a_capture_ballot() {
    let f = fixture(1_000 * DAY);
    found(&f, "founder", "Town").await;
    for i in 0..99 {
        seat_citizen(&f, &format!("cit{i}"), "town").await; // 100 honest citizens total
    }
    seat_citizen(&f, "attacker", "town").await; // + 1 attacker citizen = 101
    let victim = f.store.find_by_handle("cit0").await.unwrap().unwrap();

    let p = f
        .services.governance()
        .open_proposal("attacker", "town", ProposalKind::Ban { user: victim.id })
        .await
        .unwrap();
    f.services.governance().cast_vote("attacker", p.id.0, true).await.unwrap(); // votes alone

    f.clock.set(Timestamp(1_000 * DAY + 5 * DAY)); // past the 3-day window
    f.services.governance().resolve_due("town").await;

    let closed = f.services.governance().list_proposals("town").await.into_iter().find(|x| x.id == p.id).unwrap();
    assert_eq!(closed.status, ProposalStatus::Failed, "one vote of 101 fails quorum");
    let server = f.store.find_by_slug("town").await.unwrap().unwrap();
    assert!(
        !f.store.get(victim.id, server.id).await.unwrap().unwrap().is_sanctioned,
        "the target keeps their standing — no capture"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// B. No backdoor into the franchise
// ─────────────────────────────────────────────────────────────────────────────

/// There is no "admit my friend" path: `try_enfranchise` refuses an unqualified
/// member even when the founder wants them in.
#[tokio::test]
async fn there_is_no_founder_override_of_the_criteria() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    register_and_join(&f, "buddy", "town").await;
    match f.services.try_enfranchise("buddy", "town").await.unwrap() {
        EnfranchiseOutcome::NotEligible(_) => {}
        other => panic!("an unqualified member must not be admitted, got {other:?}"),
    }
    assert!(!f.store.get(
        f.store.find_by_handle("buddy").await.unwrap().unwrap().id,
        f.store.find_by_slug("town").await.unwrap().unwrap().id,
    ).await.unwrap().unwrap().is_citizen());
}

/// A franchise-barred puppet account (e.g. a staff/content bot) can never be
/// enfranchised, even when every ordinary criterion is satisfied.
#[tokio::test]
async fn a_franchise_barred_puppet_never_gets_in() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    register_and_join(&f, "puppet", "town").await;
    // Bar the account and otherwise fully qualify it.
    let mut u = f.store.find_by_handle("puppet").await.unwrap().unwrap();
    u.is_franchise_barred = true;
    f.store.update_user(u).await.unwrap();
    f.clock.set(Timestamp(1_050 * DAY));
    set_contribution(&f, "puppet", "town", 50).await;

    match f.services.try_enfranchise("puppet", "town").await.unwrap() {
        EnfranchiseOutcome::NotEligible(unmet) => assert_eq!(unmet, vec![Unmet::Barred]),
        other => panic!("a barred account must be refused, got {other:?}"),
    }
}

/// A barred account cannot even found a server (which would seat it as citizen #1).
#[tokio::test]
async fn a_barred_account_cannot_found_a_server() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("villain").await.unwrap();
    let mut u = f.store.find_by_handle("villain").await.unwrap().unwrap();
    u.is_franchise_barred = true;
    f.store.update_user(u).await.unwrap();
    assert_eq!(f.services.found_server("villain", "Lair").await, Err(FoundError::FounderBarred));
}

/// Ballot weight is not a side door into the franchise: even a member handed a
/// huge vote weight (as a passed `GrantVoteWeight` would) still cannot vote,
/// because they are not a citizen.
#[tokio::test]
async fn granting_vote_weight_to_a_non_citizen_does_not_let_them_vote() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    register_and_join(&f, "member", "town").await;
    // Simulate the effect of a passed GrantVoteWeight ballot on a non-citizen.
    let u = f.store.find_by_handle("member").await.unwrap().unwrap();
    let s = f.store.find_by_slug("town").await.unwrap().unwrap();
    let mut m = f.store.get(u.id, s.id).await.unwrap().unwrap();
    m.granted_weight = 1_000;
    f.store.upsert(m).await.unwrap();

    let p = f
        .services.governance()
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .await
        .unwrap();
    assert_eq!(
        f.services.governance().cast_vote("member", p.id.0, true).await,
        Err(VoteError::NotACitizen),
        "weight without citizenship is inert"
    );
}

/// A sanctioned citizen is stripped of the franchise: they may not vote, so a
/// captured-then-banned account can't keep voting.
#[tokio::test]
async fn a_sanctioned_citizen_cannot_vote() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    seat_citizen(&f, "rogue", "town").await;
    // Sanction them (as a passed Ban would).
    let u = f.store.find_by_handle("rogue").await.unwrap().unwrap();
    let s = f.store.find_by_slug("town").await.unwrap().unwrap();
    let mut m = f.store.get(u.id, s.id).await.unwrap().unwrap();
    m.is_sanctioned = true;
    f.store.upsert(m).await.unwrap();

    let p = f
        .services.governance()
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .await
        .unwrap();
    assert_eq!(f.services.governance().cast_vote("rogue", p.id.0, true).await, Err(VoteError::NotACitizen));
}

// ─────────────────────────────────────────────────────────────────────────────
// C. Ballot-process abuse
// ─────────────────────────────────────────────────────────────────────────────

/// One citizen, one ballot: hammering `cast_vote` doesn't stuff the box — the
/// vote is upserted, so a citizen's tenth aye is still just one aye.
#[tokio::test]
async fn a_citizen_cannot_stuff_the_ballot_box() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    let p = f
        .services.governance()
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .await
        .unwrap();
    for _ in 0..10 {
        f.services.governance().cast_vote("boss", p.id.0, true).await.unwrap();
    }
    assert_eq!(f.services.governance().proposal_head_counts(p.id.0).await, (1, 0), "ten ayes collapse to one");
}

/// Votes cast after the window has closed are rejected — no last-second reversal
/// once a ballot has been resolved.
#[tokio::test]
async fn votes_after_the_window_are_rejected() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    let p = f
        .services.governance()
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .await
        .unwrap();
    f.clock.set(Timestamp(1_000 * DAY + 5 * DAY));
    f.services.governance().resolve_due("town").await; // closes the ballot
    assert_eq!(f.services.governance().cast_vote("boss", p.id.0, true).await, Err(VoteError::Closed));
}

/// Training wheels: a server in the Seed phase cannot amend its own constitution
/// (franchise criteria), so a founder can't lower the bar to zero before the
/// electorate is large enough for the percentage math to protect it.
#[tokio::test]
async fn the_constitution_cannot_be_amended_during_seed() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await; // 1 citizen → Seed phase
    let trivial =
        FranchiseCriteria { min_account_age_days: 0, min_membership_days: 0, min_contribution: 0 };
    assert_eq!(
        f.services.governance().open_proposal("boss", "town", ProposalKind::AmendCriteria { proposed: trivial }).await,
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
#[tokio::test]
async fn a_sanctioned_member_cannot_post() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;    register_and_join(&f, "loudmouth", "town").await;
    // They can post before sanction.
    f.services.chat().post_message("loudmouth", "town", "general", "hello").await.unwrap();
    // Sanction them (as a passed Ban ballot would).
    let u = f.store.find_by_handle("loudmouth").await.unwrap().unwrap();
    let s = f.store.find_by_slug("town").await.unwrap().unwrap();
    let mut m = f.store.get(u.id, s.id).await.unwrap().unwrap();
    m.is_sanctioned = true;
    f.store.upsert(m).await.unwrap();

    assert_eq!(
        f.services.chat().post_message("loudmouth", "town", "general", "again").await,
        Err(MessageError::Sanctioned("loudmouth".into())),
    );
}

/// A non-member cannot post into a server's channels by numbers or otherwise —
/// membership is required to speak.
#[tokio::test]
async fn a_non_member_cannot_post() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;    f.services.register_account("outsider").await.unwrap(); // registered but never joined
    assert_eq!(
        f.services.chat().post_message("outsider", "town", "general", "let me in").await,
        Err(MessageError::NotAMember("outsider".into())),
    );
}

/// No plaintext downgrade: once a channel is encrypted, a plaintext post is
/// refused (the client must seal under the channel key). An attacker cannot leak
/// a would-be-private message onto the plaintext feed.
#[tokio::test]
async fn plaintext_cannot_be_posted_to_an_encrypted_channel() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    f.services.chat().create_channel("boss", "town", "secret", "").await.unwrap();
    f.services.channel_keys().enable_channel_encryption("boss", "town", "secret", HistoryMode::Ephemeral).await.unwrap();
    assert_eq!(
        f.services.chat().post_message("boss", "town", "secret", "leak").await,
        Err(MessageError::ChannelEncrypted),
    );
}

/// A permanent block cannot be worked around: once blocked, the blocked user can
/// never DM the blocker (there is no unblock, and the block overrides any DM
/// policy). Harassment by re-contact is shut down.
#[tokio::test]
async fn a_blocked_user_can_never_dm_the_blocker() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("victim").await.unwrap();
    f.services.register_account("harasser").await.unwrap();
    // Open DMs by default, so it's the block — not a policy — that stops them.
    f.services.social().send_sealed_dm("harasser", "victim", "aa", "bb").await.unwrap();
    f.services.social().block_user("victim", "harasser").await.unwrap();
    assert_eq!(
        f.services.social().send_sealed_dm("harasser", "victim", "aa", "bb").await,
        Err(DmError::NotAllowed),
        "a block is a hard, permanent wall",
    );
    assert!(!f.services.social().can_dm("harasser", "victim").await);
}

/// A friends-only inbox refuses strangers: a spammer cannot reach a user who has
/// closed their DMs, but a genuine accepted friend can.
#[tokio::test]
async fn a_friends_only_inbox_refuses_strangers() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("target").await.unwrap();
    f.services.register_account("spammer").await.unwrap();
    f.services.register_account("pal").await.unwrap();
    f.services.social().set_dm_policy("target", DmPolicy::FriendsOnly).await.unwrap();

    assert_eq!(
        f.services.social().send_sealed_dm("spammer", "target", "aa", "bb").await,
        Err(DmError::NotAllowed),
        "a stranger can't slip into a friends-only inbox",
    );
    // A mutual friend gets through.
    f.services.social().request_friend("pal", "target").await.unwrap();
    f.services.social().accept_friend("target", "pal").await.unwrap();
    assert!(f.services.social().send_sealed_dm("pal", "target", "aa", "bb").await.is_ok());
}

/// Self-directed social actions are refused — you can't block, befriend, or DM
/// yourself (a class of nonsense/abuse input the surface must reject).
#[tokio::test]
async fn self_directed_social_actions_are_refused() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("solo").await.unwrap();
    assert_eq!(f.services.social().block_user("solo", "solo").await, Err(SocialError::Self_));
    assert_eq!(f.services.social().request_friend("solo", "solo").await, Err(SocialError::Self_));
    assert_eq!(f.services.social().send_sealed_dm("solo", "solo", "aa", "bb").await, Err(DmError::Self_));
}

/// An invite grants membership, never the franchise — even at scale. A thousand
/// invited accounts are a thousand voteless members; the electorate is untouched.
#[tokio::test]
async fn mass_invites_never_grant_the_franchise() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("host").await.unwrap();
    f.services.found_server_with_visibility("host", "Club", true).await.unwrap();
    let code = f.services.create_invite("host", "club").await.unwrap();
    for i in 0..1_000 {
        f.services.register_account(&format!("guest{i}")).await.unwrap();
        let m = f.services.accept_invite(&format!("guest{i}"), &code).await.unwrap();
        assert!(!m.is_citizen(), "an invite never seats a citizen");
    }
    assert_eq!(citizen_count(&f, "club").await, 1, "only the founder can vote");
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
#[tokio::test]
async fn a_citizen_of_one_server_cannot_vote_in_another() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss_a", "Alpha").await;
    found(&f, "boss_b", "Beta").await;
    seat_citizen(&f, "alice", "alpha").await; // a citizen of Alpha only

    let p = f
        .services.governance()
        .open_proposal("boss_b", "beta", ProposalKind::AddRule { text: "beta rule".into() })
        .await
        .unwrap();
    assert_eq!(
        f.services.governance().cast_vote("alice", p.id.0, true).await,
        Err(VoteError::NotACitizen),
        "Alpha citizenship carries no vote in Beta",
    );
    assert_eq!(f.services.governance().proposal_head_counts(p.id.0).await, (0, 0), "no cross-server ballot lands");
}

/// The agenda is per-server too: a citizen of one server cannot open a proposal in
/// a server they do not belong to.
#[tokio::test]
async fn a_citizen_of_one_server_cannot_open_a_proposal_in_another() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss_a", "Alpha").await;
    found(&f, "boss_b", "Beta").await;
    seat_citizen(&f, "alice", "alpha").await;
    assert_eq!(
        f.services.governance().open_proposal("alice", "beta", ProposalKind::AddRule { text: "x".into() }).await,
        Err(ProposeError::NotACitizen),
    );
}

/// Membership is per-server: a member of one server cannot post into another
/// server's channels — no speaking across a wall you never joined.
#[tokio::test]
async fn a_member_of_one_server_cannot_post_in_another() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss_a", "Alpha").await;
    found(&f, "boss_b", "Beta").await;
    register_and_join(&f, "alice", "alpha").await;
    assert_eq!(
        f.services.chat().post_message("alice", "beta", "general", "hello from Alpha").await,
        Err(MessageError::NotAMember("alice".into())),
    );
}

/// Police powers are per-server: an officer sworn in one server has no moderation
/// authority in another and cannot mute there.
#[tokio::test]
async fn police_powers_do_not_cross_servers() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss_a", "Alpha").await;
    found(&f, "boss_b", "Beta").await;
    // Make alice a police officer of Alpha (store shortcut — a passed AppointPolice
    // ballot would do the same in Alpha, and none of it reaches Beta).
    seat_citizen(&f, "alice", "alpha").await;
    let ua = f.store.find_by_handle("alice").await.unwrap().unwrap();
    let sa = f.store.find_by_slug("alpha").await.unwrap().unwrap();
    let mut m = f.store.get(ua.id, sa.id).await.unwrap().unwrap();
    m.is_police = true;
    f.store.upsert(m).await.unwrap();
    // A target who is a genuine member of Beta.
    register_and_join(&f, "victim", "beta").await;

    assert_eq!(
        f.services.mute().mute_member("alice", "beta", "victim").await,
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
#[tokio::test]
async fn a_passed_ballot_applies_exactly_once() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    seat_citizen(&f, "cit1", "town").await;
    seat_citizen(&f, "cit2", "town").await; // 3 citizens — a real electorate

    let p = f
        .services.governance()
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "no capture".into() })
        .await
        .unwrap();
    for c in ["boss", "cit1", "cit2"] {
        f.services.governance().cast_vote(c, p.id.0, true).await.unwrap();
    }
    f.clock.set(Timestamp(1_000 * DAY + 5 * DAY)); // past the voting window
    f.services.governance().resolve_due("town").await;
    let closed = f.services.governance().list_proposals("town").await.into_iter().find(|x| x.id == p.id).unwrap();
    assert!(matches!(closed.status, ProposalStatus::Passed { .. }));
    assert_eq!(f.services.governance().list_rules("town").await.len(), 1, "the rule enacts on the first sweep");

    // Hammer the resolver: a closed ballot is never re-applied.
    f.services.governance().resolve_due("town").await;
    f.services.governance().resolve_due("town").await;
    assert_eq!(f.services.governance().list_rules("town").await.len(), 1, "extra sweeps enact nothing further");
}

/// An already-enfranchised citizen cannot be enfranchised again — no double-seating
/// and no second rate-cap slot burned to re-admit someone already in.
#[tokio::test]
async fn a_citizen_cannot_be_enfranchised_twice() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    seat_citizen(&f, "cit", "town").await;
    assert!(matches!(
        f.services.try_enfranchise("cit", "town").await,
        Err(EnfranchiseError::AlreadyCitizen(_)),
    ));
    assert_eq!(citizen_count(&f, "town").await, 2, "the roll is unchanged by the re-attempt");
}

/// Enfranchisement requires membership: a registered account that never joined the
/// server cannot be admitted to its franchise.
#[tokio::test]
async fn a_non_member_cannot_be_enfranchised() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    f.services.register_account("outsider").await.unwrap(); // registered, never joined
    assert!(matches!(
        f.services.try_enfranchise("outsider", "town").await,
        Err(EnfranchiseError::NotAMember(_)),
    ));
}

/// Revoke a seated citizen's franchise (drop them back to a mere member). Mirrors
/// what a passed sanction or a failed re-qualification does to the roll; used to
/// prove revoked standing silently stops counting.
async fn revoke_franchise(f: &Fixture, handle: &str, slug: &str) {
    let u = f.store.find_by_handle(handle).await.unwrap().unwrap();
    let s = f.store.find_by_slug(slug).await.unwrap().unwrap();
    let mut m = f.store.get(u.id, s.id).await.unwrap().unwrap();
    m.tier = Tier::Member;
    m.enfranchised_at = None;
    f.store.upsert(m).await.unwrap();
}

/// Pass a proposal unanimously among the named citizens and sweep it in.
async fn pass_ballot(f: &Fixture, proposer: &str, slug: &str, voters: &[&str], kind: ProposalKind) {
    let p = f.services.governance().open_proposal(proposer, slug, kind).await.unwrap();
    for v in voters {
        f.services.governance().cast_vote(v, p.id.0, true).await.unwrap();
    }
    let now = f.clock.now().0;
    f.clock.set(Timestamp(now + 4 * DAY));
    f.services.governance().resolve_due(slug).await;
    f.clock.set(Timestamp(now)); // restore the clock for the caller
}

// ─────────────────────────────────────────────────────────────────────────────
// H. Encrypted-channel key-grant abuse — sealed material stays scoped
// ─────────────────────────────────────────────────────────────────────────────

/// Turning a channel encrypted is a citizens-only act: a mere member cannot flip a
/// channel to encrypted (nor, by the same door, downgrade the room's guarantees).
#[tokio::test]
async fn enabling_channel_encryption_is_citizens_only() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    f.services.chat().create_channel("boss", "town", "vault", "").await.unwrap();
    register_and_join(&f, "member", "town").await;
    assert_eq!(
        f.services.channel_keys().enable_channel_encryption("member", "town", "vault", HistoryMode::Ephemeral).await,
        Err(ChannelKeyError::NotACitizen),
    );
}

/// An outsider cannot inject channel-key material: minting a grant requires the
/// granter to be a member of the server, so a non-member's grant is refused.
#[tokio::test]
async fn an_outsider_cannot_mint_a_channel_key_grant() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    f.services.chat().create_channel("boss", "town", "vault", "").await.unwrap();
    f.services.channel_keys().enable_channel_encryption("boss", "town", "vault", HistoryMode::Ephemeral).await.unwrap();
    register_and_join(&f, "insider", "town").await;
    f.services.register_account("outsider").await.unwrap(); // registered, never joined
    assert_eq!(
        f.services.channel_keys().grant_channel_key("outsider", "town", "vault", 0, "insider", "sealed").await,
        Err(ChannelKeyError::NotAMember("outsider".into())),
    );
}

/// A grant cannot be published against a plaintext channel — no smuggling in a
/// bogus "key" for a room that has no encryption to key.
#[tokio::test]
async fn a_grant_needs_an_encrypted_channel() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    f.services.chat().create_channel("boss", "town", "lobby", "").await.unwrap(); // never encrypted
    register_and_join(&f, "member", "town").await;
    assert_eq!(
        f.services.channel_keys().grant_channel_key("boss", "town", "lobby", 0, "member", "sealed").await,
        Err(ChannelKeyError::NotEncrypted),
    );
}

/// A sealed key grant is scoped to its grantee: one member cannot read the key
/// blob sealed to another. The server hands each member only their own grants.
#[tokio::test]
async fn a_key_grant_is_not_visible_to_other_members() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    f.services.chat().create_channel("boss", "town", "vault", "").await.unwrap();
    f.services.channel_keys().enable_channel_encryption("boss", "town", "vault", HistoryMode::Ephemeral).await.unwrap();
    register_and_join(&f, "alice", "town").await;
    register_and_join(&f, "mallory", "town").await;

    f.services.channel_keys().grant_channel_key("boss", "town", "vault", 0, "alice", "alices-sealed-key").await.unwrap();

    assert_eq!(f.services.channel_keys().my_channel_grants("alice", "town", "vault").await.unwrap().len(), 1);
    assert!(
        f.services.channel_keys().my_channel_grants("mallory", "town", "vault").await.unwrap().is_empty(),
        "a member sees no grant sealed to someone else",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// I. Key directory — no identity takeover
// ─────────────────────────────────────────────────────────────────────────────

/// Publishing is keyed to the authenticated caller: an attacker publishing their
/// own entry cannot overwrite a victim's public key, so no MITM by directory
/// poisoning. Each handle's entry is its own.
#[tokio::test]
async fn publishing_cannot_overwrite_another_users_identity() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("victim").await.unwrap();
    f.services.register_account("attacker").await.unwrap();

    let victim_secret = IdentitySecret::generate();
    let victim_pub = victim_secret.public().to_hex();
    let vw = wrap_secret("victim-password-value", &victim_secret).unwrap();
    f.services.keys().publish_keys("victim", &victim_pub, WrappedKey::from(vw)).await.unwrap();

    // The attacker publishes their own (different) identity.
    let attacker_secret = IdentitySecret::generate();
    let aw = wrap_secret("attacker-password-val", &attacker_secret).unwrap();
    f.services.keys().publish_keys("attacker", &attacker_secret.public().to_hex(), WrappedKey::from(aw)).await.unwrap();

    // The victim's directory entry is untouched — a sender still fetches the real key.
    assert_eq!(f.services.keys().public_key_of("victim").await.unwrap(), victim_pub);
    assert_ne!(f.services.keys().public_key_of("attacker").await.unwrap(), victim_pub);
}

/// Republishing under a foreign handle is impossible: there is no cross-user
/// publish path, so `my_keys` only ever returns the caller's own wrapped secret.
#[tokio::test]
async fn my_keys_never_yields_another_users_secret() {
    let f = fixture(1_000 * DAY);
    f.services.register_account("owner").await.unwrap();
    let secret = IdentitySecret::generate();
    let wrapped = wrap_secret("owner-password-here-0", &secret).unwrap();
    f.services.keys().publish_keys("owner", &secret.public().to_hex(), WrappedKey::from(wrapped.clone())).await.unwrap();

    // A different caller has no entry at all — not the owner's.
    f.services.register_account("snoop").await.unwrap();
    assert!(matches!(f.services.keys().my_keys("snoop").await, Err(KeyError::NotPublished(_))));
    // The owner's own call returns exactly the owner's blob.
    assert_eq!(f.services.keys().my_keys("owner").await.unwrap().wrapped_secret, WrappedKey::from(wrapped));
}

// ─────────────────────────────────────────────────────────────────────────────
// J. Emoji ranking — capture by numbers, again
// ─────────────────────────────────────────────────────────────────────────────

/// The emoji ranking is a citizens-only franchise too: a mere member cannot add an
/// emoji, so the roster can't be spammed by fresh accounts.
#[tokio::test]
async fn a_mere_member_cannot_add_an_emoji() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    register_and_join(&f, "member", "town").await;
    assert_eq!(
        f.services.emoji().add_emoji("member", "town", "shipit", "http://img/x.png").await,
        Err(EmojiError::NotACitizen),
    );
}

/// A flood of members has zero weight on the ranking: every non-citizen vote is
/// refused, so numbers alone move no emoji up the list.
#[tokio::test]
async fn a_flood_of_members_cannot_swing_the_emoji_ranking() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    let e = f.services.emoji().add_emoji("boss", "town", "party", "http://img/p.png").await.unwrap();
    for i in 0..200 {
        register_and_join(&f, &format!("mob{i}"), "town").await;
        assert_eq!(
            f.services.emoji().vote_emoji(&format!("mob{i}"), "town", e.id.0, true).await,
            Err(EmojiError::NotACitizen),
        );
    }
    let ranked = f.services.emoji().ranked_emojis("town", "boss").await;
    assert_eq!(ranked.iter().find(|r| r.id == e.id.0).unwrap().score, 0, "no member vote counts");
}

/// One citizen, one emoji vote: hammering `vote_emoji` upserts, so a citizen's
/// hundredth up-vote is still worth exactly one.
#[tokio::test]
async fn an_emoji_vote_is_one_per_citizen() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    let e = f.services.emoji().add_emoji("boss", "town", "fire", "http://img/f.png").await.unwrap();
    for _ in 0..100 {
        f.services.emoji().vote_emoji("boss", "town", e.id.0, true).await.unwrap();
    }
    let ranked = f.services.emoji().ranked_emojis("town", "boss").await;
    assert_eq!(ranked.iter().find(|r| r.id == e.id.0).unwrap().score, 1, "a hundred ayes collapse to one");
}

/// A revoked franchise silently stops counting toward the ranking without touching
/// the stored vote — so a de-citizened account cannot keep propping up an emoji.
#[tokio::test]
async fn a_revoked_franchise_stops_counting_toward_emoji_ranking() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    seat_citizen(&f, "cit", "town").await;
    let e = f.services.emoji().add_emoji("boss", "town", "star", "http://img/s.png").await.unwrap();
    f.services.emoji().vote_emoji("cit", "town", e.id.0, true).await.unwrap();
    assert_eq!(
        f.services.emoji().ranked_emojis("town", "boss").await.iter().find(|r| r.id == e.id.0).unwrap().score,
        1,
    );

    revoke_franchise(&f, "cit", "town").await;
    assert_eq!(
        f.services.emoji().ranked_emojis("town", "boss").await.iter().find(|r| r.id == e.id.0).unwrap().score,
        0,
        "the revoked citizen's vote no longer counts",
    );
}

/// A known emoji's name cannot be hijacked: a second citizen cannot re-add the same
/// `:name:` to swap its image for a lookalike, so `:shipit:` can't be poisoned.
#[tokio::test]
async fn an_existing_emoji_name_cannot_be_hijacked() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    seat_citizen(&f, "cit", "town").await;
    f.services.emoji().add_emoji("boss", "town", "shipit", "http://img/real.png").await.unwrap();
    assert_eq!(
        f.services.emoji().add_emoji("cit", "town", "shipit", "http://evil/fake.png").await,
        Err(EmojiError::NameTaken("shipit".into())),
    );
}

/// The emoji franchise is per-server: a citizen of one server cannot vote on
/// another server's emoji.
#[tokio::test]
async fn a_citizen_cannot_vote_emoji_in_a_foreign_server() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss_a", "Alpha").await;
    found(&f, "boss_b", "Beta").await;
    seat_citizen(&f, "alice", "alpha").await;
    let e = f.services.emoji().add_emoji("boss_b", "beta", "wave", "http://img/w.png").await.unwrap();
    assert_eq!(
        f.services.emoji().vote_emoji("alice", "beta", e.id.0, true).await,
        Err(EmojiError::NotACitizen),
        "Alpha citizenship carries no emoji vote in Beta",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// K. Roles are cosmetic — never a side door into the franchise
// ─────────────────────────────────────────────────────────────────────────────

/// A role confers nothing but membership in a mention group: assigning a mere
/// member to a role (by ballot) does not let them vote — they are still not a
/// citizen. Roles cannot be used to smuggle power to a non-citizen.
#[tokio::test]
async fn holding_a_role_grants_no_vote() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    seat_citizen(&f, "cit1", "town").await;
    seat_citizen(&f, "cit2", "town").await; // a real electorate of 3
    register_and_join(&f, "mole", "town").await; // a mere member

    let citizens = ["boss", "cit1", "cit2"];
    pass_ballot(&f, "boss", "town", &citizens, ProposalKind::CreateRole { name: "Ops".into() }).await;
    let role = f.services.roles().list_roles("town").await.into_iter().find(|r| r.name == "ops").unwrap();
    let mole = f.store.find_by_handle("mole").await.unwrap().unwrap();
    pass_ballot(&f, "boss", "town", &citizens, ProposalKind::AssignRole { user: mole.id, role: role.id }).await;

    // The mole now holds the role...
    assert!(f.services.roles().role_holders("town", "ops").await.contains(&"mole".to_string()));

    // ...but still cannot cast a ballot.
    let p = f
        .services.governance()
        .open_proposal("boss", "town", ProposalKind::AddRule { text: "x".into() })
        .await
        .unwrap();
    assert_eq!(
        f.services.governance().cast_vote("mole", p.id.0, true).await,
        Err(VoteError::NotACitizen),
        "a role holder without citizenship is voteless",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// L. Mute abuse — the instant-mute power stays bounded
// ─────────────────────────────────────────────────────────────────────────────

/// An officer cannot mute themselves (officers are unmutable), so the police power
/// can't be turned into a self-inflicted or reciprocal silencing loophole.
#[tokio::test]
async fn an_officer_cannot_mute_themselves() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    seat_citizen(&f, "cop", "town").await;
    let u = f.store.find_by_handle("cop").await.unwrap().unwrap();
    let s = f.store.find_by_slug("town").await.unwrap().unwrap();
    let mut m = f.store.get(u.id, s.id).await.unwrap().unwrap();
    m.is_police = true;
    f.store.upsert(m).await.unwrap();
    assert_eq!(
        f.services.mute().mute_member("cop", "town", "cop").await,
        Err(MuteError::CannotMutePolice),
    );
}

/// A police officer cannot mute a non-member: the target must belong to the server,
/// so the power can't reach across the wall to an outsider.
#[tokio::test]
async fn an_officer_cannot_mute_a_non_member() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    seat_citizen(&f, "cop", "town").await;
    let u = f.store.find_by_handle("cop").await.unwrap().unwrap();
    let s = f.store.find_by_slug("town").await.unwrap().unwrap();
    let mut m = f.store.get(u.id, s.id).await.unwrap().unwrap();
    m.is_police = true;
    f.store.upsert(m).await.unwrap();
    f.services.register_account("stranger").await.unwrap(); // registered, never joined
    assert_eq!(
        f.services.mute().mute_member("cop", "town", "stranger").await,
        Err(MuteError::NotAMember("stranger".into())),
    );
}

/// A muted member is silenced everywhere but #appeals — so a captured-then-muted
/// critic cannot keep posting to the general floor.
#[tokio::test]
async fn a_muted_member_cannot_post_to_the_floor() {
    let f = fixture(1_000 * DAY);
    found(&f, "boss", "Town").await;
    register_and_join(&f, "loud", "town").await;
    f.services.chat().post_message("loud", "town", "general", "hi").await.unwrap();

    let u = f.store.find_by_handle("loud").await.unwrap().unwrap();
    let s = f.store.find_by_slug("town").await.unwrap().unwrap();
    let mut m = f.store.get(u.id, s.id).await.unwrap().unwrap();
    m.mute(None);
    f.store.upsert(m).await.unwrap();

    assert_eq!(
        f.services.chat().post_message("loud", "town", "general", "again").await,
        Err(MessageError::Muted("loud".into())),
    );
}
