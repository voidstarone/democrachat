//! Integration tests for the server-blind key directory (M7.2). Exercises the
//! whole client↔server contract against the real store: the client generates and
//! wraps its device identity locally, publishes the public key + opaque blob, and
//! the server can never open the secret — only hand it back for the user to unwrap
//! with their password. Lives in the composition-root crate because that is where
//! `app` (crypto + use-cases) and `adapter-store-memory` legitimately meet.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{
    open_sealed, seal_to, unwrap_secret, wrap_secret, IdentitySecret, KeyError, PublicIdentity,
    Services,
};
use domain::{Timestamp, WrappedKey};

const DAY: i64 = 86_400;

fn with_users(handles: &[&str]) -> Services {
    let store = Arc::new(MemoryStore::new());
    let s = Services::new(Arc::new(FixedClock::new(Timestamp(100 * DAY))), store.as_stores());
    for h in handles {
        s.register_account(h).unwrap();
    }
    s
}

/// The whole happy path: a client generates an identity, wraps it under a password,
/// publishes it, later fetches its own entry, and unwraps back to the same key —
/// all without the server ever holding the secret in the clear.
#[test]
fn a_user_publishes_keys_and_recovers_them_with_their_password() {
    let s = with_users(&["alice"]);

    // Client-side: generate + wrap under the password.
    let secret = IdentitySecret::generate();
    let public_hex = secret.public().to_hex();
    let wrapped = wrap_secret("correct horse battery staple", &secret).unwrap();

    s.keys().publish_keys("alice", &public_hex, WrappedKey::from(wrapped.clone())).unwrap();

    // Server hands the entry back verbatim; the public key round-trips.
    let mine = s.keys().my_keys("alice").unwrap();
    assert_eq!(mine.public_key, public_hex);
    assert_eq!(WrappedKey::from(wrapped), mine.wrapped_secret);

    // Client-side recovery on a "new device": unwrap with the password.
    let recovered = unwrap_secret("correct horse battery staple", &mine.wrapped_secret.into()).unwrap();
    assert_eq!(recovered.public().to_hex(), public_hex);
}

/// The server is blind: the wrong password cannot open the stored blob, and the
/// server has no path to do it either (it only stores/returns).
#[test]
fn the_wrapped_secret_is_useless_without_the_password() {
    let s = with_users(&["alice"]);
    let secret = IdentitySecret::generate();
    let wrapped = wrap_secret("the-real-password-000", &secret).unwrap();
    s.keys().publish_keys("alice", &secret.public().to_hex(), WrappedKey::from(wrapped)).unwrap();

    let mine = s.keys().my_keys("alice").unwrap();
    let err = unwrap_secret("wrong-password-guess-1", &mine.wrapped_secret.into());
    assert!(err.is_err(), "wrong password must not unwrap the secret");
}

/// Another user's published public key is enough to seal a message to them; only
/// the recipient (who can unwrap their own secret) can open it. This is the end the
/// directory exists to serve.
#[test]
fn a_sender_seals_to_a_fetched_public_key_and_only_the_owner_opens_it() {
    let s = with_users(&["alice", "bob"]);

    let bob_secret = IdentitySecret::generate();
    let bob_wrapped = wrap_secret("bobs-passphrase-here0", &bob_secret).unwrap();
    s.keys().publish_keys("bob", &bob_secret.public().to_hex(), WrappedKey::from(bob_wrapped)).unwrap();

    // Alice fetches bob's PUBLIC key (no secret is ever exposed) and seals to it.
    let bob_pub_hex = s.keys().public_key_of("bob").unwrap();
    let bob_pub = PublicIdentity::from_hex(&bob_pub_hex).unwrap();
    let sealed = seal_to(&bob_pub, b"meet me at the docks");

    // Bob recovers his secret from his own entry and opens the message.
    let bob_mine = s.keys().my_keys("bob").unwrap();
    let bob_recovered = unwrap_secret("bobs-passphrase-here0", &bob_mine.wrapped_secret.into()).unwrap();
    assert_eq!(open_sealed(&bob_recovered, &sealed).unwrap(), b"meet me at the docks");

    // Alice — the sender — cannot open her own sealed box (anonymous-sender seal).
    let alice_secret = IdentitySecret::generate();
    assert!(open_sealed(&alice_secret, &sealed).is_err());
}

/// Publishing a malformed public key is refused, so the directory a sender relies
/// on never holds an unusable key.
#[test]
fn a_malformed_public_key_is_rejected() {
    let s = with_users(&["alice"]);
    let secret = IdentitySecret::generate();
    let wrapped = wrap_secret("some-long-password-01", &secret).unwrap();
    let err = s.keys().publish_keys("alice", "not-a-real-key", WrappedKey::from(wrapped)).unwrap_err();
    assert!(matches!(err, KeyError::BadPublicKey(_)));
}

/// Fetching keys for a user who has not published yet is a clean "not published",
/// distinct from "no such user".
#[test]
fn fetching_before_publishing_reports_not_published() {
    let s = with_users(&["alice"]);
    assert!(matches!(s.keys().my_keys("alice"), Err(KeyError::NotPublished(_))));
    assert!(matches!(s.keys().public_key_of("alice"), Err(KeyError::NotPublished(_))));
    assert!(matches!(s.keys().public_key_of("ghost"), Err(KeyError::NoSuchUser(_))));
}
