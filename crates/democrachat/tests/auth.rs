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

#[test]
fn register_then_authenticate_succeeds() {
    let s = services();
    s.register_with_password("alice", GOOD_PW).unwrap();
    let user = s.authenticate("alice", GOOD_PW).expect("correct password authenticates");
    assert_eq!(user.handle, "alice");
    assert!(user.has_password());
}

#[test]
fn wrong_password_does_not_authenticate() {
    let s = services();
    s.register_with_password("alice", GOOD_PW).unwrap();
    assert!(s.authenticate("alice", "wrong wrong wrong!").is_none());
}

#[test]
fn unknown_handle_does_not_authenticate() {
    let s = services();
    assert!(s.authenticate("nobody", GOOD_PW).is_none());
}

#[test]
fn short_password_is_rejected() {
    let s = services();
    match s.register_with_password("alice", "short") {
        Err(RegisterError::WeakPassword(_)) => {}
        other => panic!("expected WeakPassword, got {other:?}"),
    }
    // And no account was created.
    assert!(s.find_user("alice").is_none());
}

#[test]
fn a_passwordless_account_cannot_authenticate() {
    let s = services();
    // register_account (seed/CLI path) sets no password.
    s.register_account("seed").unwrap();
    let user = s.find_user("seed").unwrap();
    assert!(!user.has_password());
    assert!(s.authenticate("seed", GOOD_PW).is_none());
    assert!(s.authenticate("seed", "").is_none());
}

#[test]
fn set_password_makes_a_seed_account_loginable() {
    let s = services();
    s.register_account("seed").unwrap();
    s.set_password("seed", GOOD_PW).unwrap();
    assert!(s.authenticate("seed", GOOD_PW).is_some());
}

#[test]
fn the_stored_hash_is_not_the_plaintext() {
    let s = services();
    s.register_with_password("alice", GOOD_PW).unwrap();
    let user = s.find_user("alice").unwrap();
    assert_ne!(user.password_hash, GOOD_PW);
    assert!(user.password_hash.starts_with("$argon2")); // PHC Argon2 string
}

#[test]
fn a_duplicate_handle_is_rejected() {
    let s = services();
    s.register_with_password("alice", GOOD_PW).unwrap();
    match s.register_with_password("alice", GOOD_PW) {
        Err(RegisterError::HandleTaken(_)) => {}
        other => panic!("expected HandleTaken, got {other:?}"),
    }
}
