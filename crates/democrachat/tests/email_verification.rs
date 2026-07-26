//! Integration tests for email verification: the hard-gate signup flow, single-use
//! and expiring tokens, encrypted-at-rest addresses, uniqueness, and resend — all
//! at the `Services` layer (the web login gate itself lives in `adapter-web`).

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{EmailVerificationMode, RegisterError, Services, VaultKey};
use domain::Timestamp;

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
