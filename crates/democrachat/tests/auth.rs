//! Integration tests for the authentication use-cases: password registration,
//! login, and the passwordless-account guard.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{RegisterError, Services};
use domain::Timestamp;

const DAY: i64 = 86_400;
const GOOD_PW: &str = "correct horse!!!"; // 16 chars — clears the floor

fn services() -> Services {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(100 * DAY)));
    Services::new(clock, store.as_stores())
}

#[tokio::test]
async fn register_then_authenticate_succeeds() {
    let s = services();
    s.register_with_password("alice", GOOD_PW).await.unwrap();
    let user = s.authenticate("alice", GOOD_PW).await.expect("correct password authenticates");
    assert_eq!(user.handle, "alice");
    assert!(user.has_password());
}

#[tokio::test]
async fn wrong_password_does_not_authenticate() {
    let s = services();
    s.register_with_password("alice", GOOD_PW).await.unwrap();
    assert!(s.authenticate("alice", "wrong wrong wrong!").await.is_none());
}

#[tokio::test]
async fn unknown_handle_does_not_authenticate() {
    let s = services();
    assert!(s.authenticate("nobody", GOOD_PW).await.is_none());
}

#[tokio::test]
async fn short_password_is_rejected() {
    let s = services();
    match s.register_with_password("alice", "short").await {
        Err(RegisterError::WeakPassword(_)) => {}
        other => panic!("expected WeakPassword, got {other:?}"),
    }
    // And no account was created.
    assert!(s.find_user("alice").await.is_none());
}

#[tokio::test]
async fn a_passwordless_account_cannot_authenticate() {
    let s = services();
    // register_account (seed/CLI path) sets no password.
    s.register_account("seed").await.unwrap();
    let user = s.find_user("seed").await.unwrap();
    assert!(!user.has_password());
    assert!(s.authenticate("seed", GOOD_PW).await.is_none());
    assert!(s.authenticate("seed", "").await.is_none());
}

#[tokio::test]
async fn set_password_makes_a_seed_account_loginable() {
    let s = services();
    s.register_account("seed").await.unwrap();
    s.set_password("seed", GOOD_PW).await.unwrap();
    assert!(s.authenticate("seed", GOOD_PW).await.is_some());
}

#[tokio::test]
async fn the_stored_hash_is_not_the_plaintext() {
    let s = services();
    s.register_with_password("alice", GOOD_PW).await.unwrap();
    let user = s.find_user("alice").await.unwrap();
    assert_ne!(user.password_hash, GOOD_PW);
    assert!(user.password_hash.starts_with("$argon2")); // PHC Argon2 string
}

#[tokio::test]
async fn a_duplicate_handle_is_rejected() {
    let s = services();
    s.register_with_password("alice", GOOD_PW).await.unwrap();
    match s.register_with_password("alice", GOOD_PW).await {
        Err(RegisterError::HandleTaken(_)) => {}
        other => panic!("expected HandleTaken, got {other:?}"),
    }
}
