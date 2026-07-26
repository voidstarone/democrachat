//! Integration tests for email verification: the hard-gate signup flow, single-use
//! and expiring tokens, encrypted-at-rest addresses, uniqueness, and resend — all
//! at the `Services` layer (the web login gate itself lives in `adapter-web`).

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{
    EmailVerificationMode, EnfranchiseOutcome, MembershipStore, RegisterError, ServerStore,
    Services, UserStore, VaultKey,
};
use domain::{Tier, Timestamp, Unmet};

const DAY: i64 = 86_400;
const GOOD_PW: &str = "correct horse!!!"; // 16 chars — clears the floor

fn key(seed: &str) -> VaultKey {
    VaultKey::from_hex(&seed.repeat(32)).unwrap()
}

/// Services in hard-verification mode with an email key; returns the clock handle so
/// a test can advance time to expire a token.
fn hard_services() -> (Services, Arc<FixedClock>) {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(100 * DAY)));
    let s = Services::new(clock.clone(), store.as_stores())
        .with_email_policy(EmailVerificationMode::Hard, Some(key("ab")));
    (s, clock)
}

#[tokio::test]
async fn hard_mode_register_is_unverified_and_issues_a_token() {
    let (s, _clock) = hard_services();
    let reg = s
        .register_with_password("alice", "alice@example.com", GOOD_PW)
        .await
        .unwrap();
    assert!(reg.verification_token.is_some(), "a token must be issued to email");
    assert!(!reg.user.is_email_verified(), "account starts unverified");
    assert!(reg.user.has_email(), "an (encrypted) email is stored");
    // authenticate still returns the account (the verified gate is the web layer's job).
    assert!(s.authenticate("alice", GOOD_PW).await.is_some());
}

#[tokio::test]
async fn the_stored_email_is_ciphertext_not_plaintext() {
    let (s, _clock) = hard_services();
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    let user = s.find_user("alice").await.unwrap();
    assert!(!user.email_enc.contains("alice@example.com"), "address must not be stored in the clear");
    assert!(!user.email_enc.is_empty());
}

#[tokio::test]
async fn verifying_marks_the_account_and_is_single_use() {
    let (s, _clock) = hard_services();
    let token = s
        .register_with_password("alice", "alice@example.com", GOOD_PW)
        .await
        .unwrap()
        .verification_token
        .unwrap();

    let verified = s.verify_email(&token).await.unwrap().expect("valid token verifies");
    assert!(verified.is_email_verified());
    assert!(s.find_user("alice").await.unwrap().is_email_verified());
    // ...and the token cannot be replayed.
    assert!(s.verify_email(&token).await.unwrap().is_none(), "token is single-use");
}

#[tokio::test]
async fn an_unknown_token_does_not_verify() {
    let (s, _clock) = hard_services();
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    assert!(s.verify_email("not-a-real-token").await.unwrap().is_none());
    assert!(!s.find_user("alice").await.unwrap().is_email_verified());
}

#[tokio::test]
async fn an_expired_token_does_not_verify() {
    let (s, clock) = hard_services();
    let token = s
        .register_with_password("alice", "alice@example.com", GOOD_PW)
        .await
        .unwrap()
        .verification_token
        .unwrap();
    // Tokens live 24h; jump two days ahead.
    clock.set(Timestamp(102 * DAY));
    assert!(s.verify_email(&token).await.unwrap().is_none(), "expired token is rejected");
    assert!(!s.find_user("alice").await.unwrap().is_email_verified());
}

#[tokio::test]
async fn a_duplicate_email_is_rejected_case_insensitively() {
    let (s, _clock) = hard_services();
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    match s.register_with_password("bob", "ALICE@Example.com", GOOD_PW).await {
        Err(RegisterError::EmailTaken) => {}
        other => panic!("expected EmailTaken, got {other:?}"),
    }
    assert!(s.find_user("bob").await.is_none(), "no account created on clash");
}

#[tokio::test]
async fn a_malformed_email_is_rejected() {
    let (s, _clock) = hard_services();
    match s.register_with_password("alice", "not-an-email", GOOD_PW).await {
        Err(RegisterError::InvalidEmail(_)) => {}
        other => panic!("expected InvalidEmail, got {other:?}"),
    }
}

#[tokio::test]
async fn resend_returns_the_address_and_a_working_token_until_verified() {
    let (s, _clock) = hard_services();
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();

    let target = s.issue_resend("alice").await.unwrap().expect("unverified account can resend");
    assert_eq!(target.email, "alice@example.com", "resend recovers the plaintext address");
    assert!(s.verify_email(&target.token).await.unwrap().is_some());
    // Once verified, there is nothing to resend.
    assert!(s.issue_resend("alice").await.unwrap().is_none());
}

/// Services in soft-verification mode: the door is open, the ballot box is not.
fn soft_services() -> (Services, Arc<FixedClock>, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(100 * DAY)));
    let s = Services::new(clock.clone(), store.as_stores())
        .with_email_policy(EmailVerificationMode::Soft, Some(key("ef")));
    (s, clock, store)
}

/// Push a server out of its founding phase by seating citizens straight into the
/// store — a *test* shortcut, not a use-case — so that the ordinary franchise
/// criteria apply to whoever joins next. Seed lasts until five citizens.
async fn charter(store: &MemoryStore, s: &Services, slug: &str, extra: u32) {
    let server = store.find_by_slug(slug).await.unwrap().unwrap();
    for i in 0..extra {
        let handle = format!("cit{i}");
        s.register_account(&handle).await.unwrap(); // seed accounts are confirmed
        s.join_server(&handle, slug).await.unwrap();
        let user = store.find_by_handle(&handle).await.unwrap().unwrap();
        let mut m = store.get(user.id, server.id).await.unwrap().unwrap();
        m.tier = Tier::Citizen;
        m.enfranchised_at = Some(s.now());
        store.upsert(m).await.unwrap();
    }
}

/// Soft mode still creates the account unconfirmed and still emails a link — the
/// difference from hard is only what an unconfirmed account may *do*.
#[tokio::test]
async fn soft_mode_registers_unconfirmed_but_lets_the_account_sign_in() {
    let (s, _clock, _store) = soft_services();
    let reg = s
        .register_with_password("alice", "alice@example.com", GOOD_PW)
        .await
        .unwrap();
    assert!(reg.verification_token.is_some(), "soft mode still issues a link to email");
    assert!(!reg.user.is_email_verified(), "the account starts unconfirmed");
    assert!(
        s.authenticate("alice", GOOD_PW).await.is_some(),
        "soft mode must not gate sign-in — that is hard mode's job"
    );
}

/// Seat a host and a fully-qualified-but-unconfirmed member on a server, with the
/// clock wound past the default 28-day dwell.
async fn qualified_but_unconfirmed(s: &Services, clock: &FixedClock, store: &MemoryStore) {
    s.register_account("host").await.unwrap(); // seed account: already confirmed
    s.found_server("host", "Town Square").await.unwrap();
    charter(store, s, "town-square", 4).await; // 5 citizens: past Seed, no waiver
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    s.join_server("alice", "town-square").await.unwrap();
    clock.set(Timestamp(140 * DAY)); // 40 days of membership — well past the dwell
}

/// Confirm alice's address the way she would after a month of lurking: her signup
/// link died at the 24-hour mark, so she asks for a fresh one.
async fn confirm_via_resend(s: &Services) {
    let target = s.issue_resend("alice").await.unwrap().expect("an unconfirmed account can resend");
    s.verify_email(&target.token).await.unwrap().expect("the fresh token confirms the address");
}

/// The headline rule: time served, criteria met, and still no vote — because the
/// address is unconfirmed. Confirming it, and nothing else, enfranchises her.
#[tokio::test]
async fn soft_mode_withholds_the_franchise_until_the_address_is_confirmed() {
    let (s, clock, store) = soft_services();
    qualified_but_unconfirmed(&s, &clock, &store).await;

    let elig = s.eligibility("alice", "town-square").await.unwrap();
    assert!(!elig.is_eligible());
    assert_eq!(
        elig.unmet,
        vec![Unmet::EmailUnverified],
        "the address must be the *only* thing left — this is what the UI banners"
    );
    match s.try_enfranchise("alice", "town-square").await.unwrap() {
        EnfranchiseOutcome::NotEligible(unmet) => assert_eq!(unmet, vec![Unmet::EmailUnverified]),
        other => panic!("expected NotEligible, got {other:?}"),
    }

    confirm_via_resend(&s).await;

    assert!(s.eligibility("alice", "town-square").await.unwrap().is_eligible());
    assert!(matches!(
        s.try_enfranchise("alice", "town-square").await.unwrap(),
        EnfranchiseOutcome::Admitted
    ));
}

/// The automatic sweep runs on the same domain rule, so it cannot admit round the
/// back what `try_enfranchise` refuses at the front.
#[tokio::test]
async fn the_automatic_sweep_does_not_admit_an_unconfirmed_member() {
    let (s, clock, store) = soft_services();
    qualified_but_unconfirmed(&s, &clock, &store).await;

    assert_eq!(s.auto_enfranchise("town-square").await, 0, "nobody may be swept in unconfirmed");

    confirm_via_resend(&s).await;
    assert_eq!(s.auto_enfranchise("town-square").await, 1, "confirming makes her sweepable");
}

/// A founder votes in their own server from the first moment, confirmed or not —
/// but on a 28-day clock.
#[tokio::test]
async fn a_founder_votes_immediately_on_trust() {
    let (s, _clock, _store) = soft_services();
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    s.found_server("alice", "Alice Town").await.expect("founding does not wait on an email");

    assert_eq!(s.member_tier("alice", "alice-town").await, Some(Tier::Citizen));
    let trust = s.trusted_franchise("alice", "alice-town").await;
    assert_eq!(trust.days_left, Some(28), "the vote is held on trust, with a deadline");
    assert!(!trust.lapsed);
}

/// ...and so does everyone who joins while the server is still finding its feet.
#[tokio::test]
async fn a_seed_phase_joiner_is_enfranchised_the_day_they_arrive() {
    let (s, _clock, store) = soft_services();
    s.register_account("host").await.unwrap();
    s.found_server("host", "Town Square").await.unwrap();
    s.register_with_password("bob", "bob@example.com", GOOD_PW).await.unwrap();
    s.join_server("bob", "town-square").await.unwrap();

    assert_eq!(s.auto_enfranchise("town-square").await, 1, "no dwell for a founding member");
    assert_eq!(s.member_tier("bob", "town-square").await, Some(Tier::Citizen));
    assert_eq!(s.trusted_franchise("bob", "town-square").await.days_left, Some(28));
    // The waiver is the founding window's, not bob's: once the server charters, the
    // next arrival waits like anyone else.
    charter(&store, &s, "town-square", 4).await;
    s.register_with_password("carol", "carol@example.com", GOOD_PW).await.unwrap();
    s.join_server("carol", "town-square").await.unwrap();
    assert_eq!(s.auto_enfranchise("town-square").await, 0, "the head start has closed");
}

/// The deadline has teeth: the day it passes the vote stops counting, without
/// waiting for any sweep to notice.
#[tokio::test]
async fn an_unconfirmed_founding_vote_stops_counting_the_day_it_expires() {
    let (s, clock, _store) = soft_services();
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    s.found_server("alice", "Alice Town").await.unwrap();

    // Day 27: still a voter — proposing is citizens-only, so it stands in for the
    // franchise itself.
    clock.set(Timestamp(127 * DAY));
    s.governance()
        .open_proposal("alice", "alice-town", domain::ProposalKind::AddRule { text: "be kind".into() })
        .await
        .expect("a founding member may propose while their vote stands");

    // Day 28: the grace is spent. No sweep has run — the vote is simply gone.
    clock.set(Timestamp(128 * DAY));
    match s
        .governance()
        .open_proposal("alice", "alice-town", domain::ProposalKind::AddRule { text: "or not".into() })
        .await
    {
        Err(app::ProposeError::NotACitizen) => {}
        other => panic!("expected NotACitizen once the deadline passed, got {other:?}"),
    }

    // The reconcile pass then makes the loss visible where the member reads their
    // standing, rather than leaving a citizen whose ballots quietly don't count.
    assert_eq!(s.reconcile_trusted_franchise("alice-town").await, 1);
    assert_eq!(s.member_tier("alice", "alice-town").await, Some(Tier::Member));
    let trust = s.trusted_franchise("alice", "alice-town").await;
    assert!(trust.lapsed, "the UI needs to know this vote was lost, not merely absent");
    assert_eq!(trust.days_left, None);
}

/// Confirming inside the window keeps the vote without interruption.
#[tokio::test]
async fn confirming_in_time_keeps_the_founding_vote() {
    let (s, clock, _store) = soft_services();
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    s.found_server("alice", "Alice Town").await.unwrap();

    clock.set(Timestamp(110 * DAY)); // day 10 of 28
    let target = s.issue_resend("alice").await.unwrap().unwrap();
    s.verify_email(&target.token).await.unwrap().unwrap();
    s.reconcile_trusted_franchise("alice-town").await;

    assert_eq!(s.member_tier("alice", "alice-town").await, Some(Tier::Citizen));
    let trust = s.trusted_franchise("alice", "alice-town").await;
    assert_eq!(trust.days_left, None, "no deadline left to meet");
    assert!(!trust.lapsed);

    // And it keeps counting past what would have been the deadline.
    clock.set(Timestamp(200 * DAY));
    assert_eq!(s.member_tier("alice", "alice-town").await, Some(Tier::Citizen));
    s.governance()
        .open_proposal("alice", "alice-town", domain::ProposalKind::AddRule { text: "be kind".into() })
        .await
        .expect("a confirmed citizen votes indefinitely");
}

/// Letting it lapse is recoverable — but only by confirming, and only once. The
/// stale deadline is what stops a second grace being handed out.
#[tokio::test]
async fn a_lapsed_founder_gets_the_vote_back_by_confirming_and_not_otherwise() {
    let (s, clock, _store) = soft_services();
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    s.found_server("alice", "Alice Town").await.unwrap();

    clock.set(Timestamp(130 * DAY)); // two days past the deadline
    assert_eq!(s.reconcile_trusted_franchise("alice-town").await, 1);
    assert_eq!(s.member_tier("alice", "alice-town").await, Some(Tier::Member));

    // Still in Seed, still unconfirmed: the founding waiver must not seat her again.
    assert_eq!(s.auto_enfranchise("alice-town").await, 0, "a lapse is not a fresh grace");
    assert_eq!(
        s.eligibility("alice", "alice-town").await.unwrap().unmet,
        vec![Unmet::EmailUnverified],
        "and she is told exactly what is missing"
    );

    // Confirming restores it on the ordinary path — by now she has served the dwell
    // the waiver excused her.
    let target = s.issue_resend("alice").await.unwrap().unwrap();
    s.verify_email(&target.token).await.unwrap().unwrap();
    s.reconcile_trusted_franchise("alice-town").await;
    assert_eq!(s.auto_enfranchise("alice-town").await, 1, "confirming re-admits her");
    assert_eq!(s.member_tier("alice", "alice-town").await, Some(Tier::Citizen));
    assert_eq!(s.trusted_franchise("alice", "alice-town").await.days_left, None);
}

/// With verification off there is no deadline to meet: a founder's vote is
/// unconditional.
#[tokio::test]
async fn off_mode_puts_no_deadline_on_a_founding_vote() {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(100 * DAY)));
    let s = Services::new(clock.clone(), store.as_stores())
        .with_email_policy(EmailVerificationMode::Off, Some(key("ef")));
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    s.found_server("alice", "Alice Town").await.unwrap();
    assert_eq!(s.trusted_franchise("alice", "alice-town").await.days_left, None);

    clock.set(Timestamp(200 * DAY));
    assert_eq!(s.reconcile_trusted_franchise("alice-town").await, 0);
    assert_eq!(s.member_tier("alice", "alice-town").await, Some(Tier::Citizen));
}

/// With verification off, none of this applies: the address is not a franchise
/// question and founding is unrestricted.
#[tokio::test]
async fn off_mode_leaves_the_franchise_alone() {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(100 * DAY)));
    let s = Services::new(clock.clone(), store.as_stores())
        .with_email_policy(EmailVerificationMode::Off, Some(key("ef")));
    s.register_with_password("alice", "alice@example.com", GOOD_PW).await.unwrap();
    s.found_server("alice", "Alice Town").await.expect("off mode does not gate founding");
}

#[tokio::test]
async fn off_mode_creates_verified_accounts_with_no_token() {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(100 * DAY)));
    // A key is present, but verification is off: the address is still encrypted, yet
    // the account is immediately usable and no token is issued.
    let s = Services::new(clock, store.as_stores())
        .with_email_policy(EmailVerificationMode::Off, Some(key("cd")));
    let reg = s
        .register_with_password("alice", "alice@example.com", GOOD_PW)
        .await
        .unwrap();
    assert!(reg.verification_token.is_none(), "off mode issues no token");
    assert!(reg.user.is_email_verified(), "off mode accounts are usable at once");
    assert!(reg.user.has_email(), "the address is still stored (encrypted)");
    assert!(!s.find_user("alice").await.unwrap().email_enc.contains("alice@example.com"));
}
