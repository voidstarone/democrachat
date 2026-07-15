//! Integration tests for the social use-cases (DMs, blocks, friends) against the
//! real in-memory store. Lives in the composition-root crate because that is
//! where `app` and `adapter-store-memory` legitimately meet.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{
    open_sealed, seal_to, DmError, IdentitySecret, PublicIdentity, Services, SocialError,
};
use domain::{DmMessage, DmPolicy, Timestamp, WrappedKey};

const DAY: i64 = 86_400;

fn services(now_secs: i64) -> Services {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(now_secs)));
    Services::new(clock, store.as_stores())
}

/// A deterministic per-handle device key, so a test can both seal to a user and
/// (as that user) open — without threading secrets around. Not how the real client
/// derives keys; just a stable stand-in for one.
fn secret_for(handle: &str) -> IdentitySecret {
    let base = handle.bytes().fold(1u8, |a, b| a.wrapping_add(b));
    let mut hex = String::with_capacity(64);
    for i in 0..32u8 {
        hex.push_str(&format!("{:02x}", base.wrapping_mul(3).wrapping_add(i).wrapping_add(7)));
    }
    IdentitySecret::from_hex(&hex).unwrap()
}

fn with_users(handles: &[&str]) -> Services {
    let s = services(100 * DAY);
    for h in handles {
        s.register_account(h).unwrap();
        // Publish each user's device key so DMs can be sealed to them. The wrapped
        // secret is irrelevant to these tests (the server never opens it), so a
        // placeholder blob is fine — the directory validates only the public key.
        let secret = secret_for(h);
        s.keys().publish_keys(
            h,
            &secret.public().to_hex(),
            WrappedKey { salt: "00".into(), nonce: "00".into(), ciphertext: "00".into() },
        )
        .unwrap();
    }
    s
}

/// The client side of sending: fetch both parties' public keys, seal the plaintext
/// to each, and hand the server only ciphertext. Hex-encodes the sealed bytes for
/// transport (the server stores the string opaquely).
fn send(s: &Services, from: &str, to: &str, text: &str) -> Result<DmMessage, DmError> {
    let to_pub = PublicIdentity::from_hex(&s.keys().public_key_of(to).unwrap()).unwrap();
    let from_pub = PublicIdentity::from_hex(&s.keys().public_key_of(from).unwrap()).unwrap();
    let for_recipient = hex::encode(seal_to(&to_pub, text.as_bytes()));
    let for_sender = hex::encode(seal_to(&from_pub, text.as_bytes()));
    s.social().send_sealed_dm(from, to, &for_recipient, &for_sender)
}

/// The client side of reading: for each message, open the ciphertext sealed to the
/// viewer with the viewer's device key.
fn read(s: &Services, viewer: &str, other: &str) -> Vec<String> {
    let viewer_id = s.find_user(viewer).unwrap().id;
    let secret = secret_for(viewer);
    s.social().conversation(viewer, other)
        .into_iter()
        .map(|m| {
            let ct = if m.sender == viewer_id { m.sealed_for_sender } else { m.sealed_for_recipient };
            let bytes = hex::decode(ct).unwrap();
            String::from_utf8(open_sealed(&secret, &bytes).unwrap()).unwrap()
        })
        .collect()
}

#[test]
fn dms_are_on_by_default_between_strangers() {
    let s = with_users(&["alice", "bob"]);
    send(&s, "alice", "bob", "hey").unwrap();
    // Both parties can read the message — each opens their own sealed copy.
    assert_eq!(read(&s, "bob", "alice"), vec!["hey".to_string()]);
    assert_eq!(read(&s, "alice", "bob"), vec!["hey".to_string()]);
}

#[test]
fn conversation_is_shared_regardless_of_direction() {
    let s = with_users(&["alice", "bob"]);
    send(&s, "alice", "bob", "one").unwrap();
    send(&s, "bob", "alice", "two").unwrap();
    assert_eq!(read(&s, "alice", "bob"), vec!["one".to_string(), "two".to_string()]);
    assert_eq!(read(&s, "bob", "alice"), vec!["one".to_string(), "two".to_string()]);
}

#[test]
fn a_third_party_cannot_open_a_dm() {
    let s = with_users(&["alice", "bob", "eve"]);
    send(&s, "alice", "bob", "secret").unwrap();
    // Eve pulls the stored conversation but holds neither party's device key.
    let stored = s.social().conversation("alice", "bob");
    let eve = secret_for("eve");
    for m in &stored {
        assert!(open_sealed(&eve, &hex::decode(&m.sealed_for_recipient).unwrap()).is_err());
        assert!(open_sealed(&eve, &hex::decode(&m.sealed_for_sender).unwrap()).is_err());
    }
}

#[test]
fn the_stored_dm_carries_no_plaintext() {
    let s = with_users(&["alice", "bob"]);
    send(&s, "alice", "bob", "rendezvous-at-dawn").unwrap();
    // The server-held record is ciphertext only — the plaintext never appears.
    let m = &s.social().conversation("alice", "bob")[0];
    assert!(!m.sealed_for_recipient.contains("rendezvous"));
    assert!(!m.sealed_for_sender.contains("rendezvous"));
}

#[test]
fn cannot_dm_yourself() {
    let s = with_users(&["alice"]);
    assert_eq!(send(&s, "alice", "alice", "hi"), Err(DmError::Self_));
}

#[test]
fn a_dm_with_no_ciphertext_is_rejected() {
    let s = with_users(&["alice", "bob"]);
    // The server is blind to the body; it only insists a ciphertext is present.
    assert_eq!(s.social().send_sealed_dm("alice", "bob", "", ""), Err(DmError::EmptyBody));
}

#[test]
fn a_by_id_dm_lands_exactly_like_a_by_handle_dm() {
    // This is the path a forwarded `Command::SendDm` runs on the sender's home: the
    // owner authorizes by home-stable id, having never seen the caller's handles.
    let s = with_users(&["alice", "bob"]);
    let alice = s.find_user("alice").unwrap().id;
    let bob = s.find_user("bob").unwrap().id;

    let to_pub = PublicIdentity::from_hex(&s.keys().public_key_of("bob").unwrap()).unwrap();
    let from_pub = PublicIdentity::from_hex(&s.keys().public_key_of("alice").unwrap()).unwrap();
    let for_recipient = hex::encode(seal_to(&to_pub, b"by-id"));
    let for_sender = hex::encode(seal_to(&from_pub, b"by-id"));

    s.social().send_sealed_dm_by_id(alice.0, bob.0, &for_recipient, &for_sender).unwrap();

    // The recipient reads it back through the ordinary conversation view.
    assert_eq!(read(&s, "bob", "alice"), vec!["by-id".to_string()]);
}

#[test]
fn a_by_id_dm_still_honours_the_dm_gate() {
    // The owner re-checks `can_dm` itself — a forwarder cannot bypass friends-only
    // by routing the write to the recipient's home.
    let s = with_users(&["alice", "bob"]);
    s.social().set_dm_policy("bob", DmPolicy::FriendsOnly).unwrap();
    let alice = s.find_user("alice").unwrap().id;
    let bob = s.find_user("bob").unwrap().id;
    assert_eq!(
        s.social().send_sealed_dm_by_id(alice.0, bob.0, "aa", "bb"),
        Err(DmError::NotAllowed),
    );
}

#[test]
fn friends_only_blocks_strangers_but_allows_friends() {
    let s = with_users(&["alice", "bob"]);
    s.social().set_dm_policy("bob", DmPolicy::FriendsOnly).unwrap();

    // A stranger cannot reach a friends-only user.
    assert_eq!(send(&s, "alice", "bob", "hi"), Err(DmError::NotAllowed));
    assert!(!s.social().can_dm("alice", "bob"));

    // Become friends: request + accept.
    s.social().request_friend("alice", "bob").unwrap();
    s.social().accept_friend("bob", "alice").unwrap();
    assert!(s.social().are_friends("alice", "bob"));

    // Now the DM goes through.
    assert!(s.social().can_dm("alice", "bob"));
    assert!(send(&s, "alice", "bob", "hi").is_ok());
}

#[test]
fn a_block_silences_dms_both_ways_permanently() {
    let s = with_users(&["alice", "bob"]);
    // They were even friends first.
    s.social().request_friend("alice", "bob").unwrap();
    s.social().accept_friend("bob", "alice").unwrap();

    s.social().block_user("alice", "bob").unwrap();

    // Neither direction works, whatever the policy.
    assert_eq!(send(&s, "alice", "bob", "hi"), Err(DmError::NotAllowed));
    assert_eq!(send(&s, "bob", "alice", "hi"), Err(DmError::NotAllowed));
    assert!(s.social().is_blocked_between("bob", "alice"));
}

#[test]
fn cannot_block_yourself() {
    let s = with_users(&["alice"]);
    assert_eq!(s.social().block_user("alice", "alice"), Err(SocialError::Self_));
}

#[test]
fn a_by_id_block_silences_dms_exactly_like_a_by_handle_block() {
    // This is what each of the two users' homes runs when a `Command::Block` is
    // forwarded: it authorizes by home-stable id, having never seen the handles.
    let s = with_users(&["alice", "bob"]);
    let alice = s.find_user("alice").unwrap().id;
    let bob = s.find_user("bob").unwrap().id;

    s.social().block_user_by_id(alice.0, bob.0).unwrap();

    assert!(s.social().is_blocked_between("alice", "bob"));
    assert_eq!(send(&s, "alice", "bob", "hi"), Err(DmError::NotAllowed));
    assert_eq!(send(&s, "bob", "alice", "hi"), Err(DmError::NotAllowed));
}

#[test]
fn a_by_id_block_is_idempotent() {
    // A block commits on BOTH homes (and a partial commit may be re-driven), so the
    // same block landing twice must converge on one record, never error.
    let s = with_users(&["alice", "bob"]);
    let alice = s.find_user("alice").unwrap().id;
    let bob = s.find_user("bob").unwrap().id;

    s.social().block_user_by_id(alice.0, bob.0).unwrap();
    s.social().block_user_by_id(alice.0, bob.0).expect("re-applying the same block is a no-op");
    assert!(s.social().is_blocked_between("alice", "bob"));
}

#[test]
fn a_by_id_self_block_is_refused() {
    let s = with_users(&["alice"]);
    let alice = s.find_user("alice").unwrap().id;
    assert_eq!(s.social().block_user_by_id(alice.0, alice.0), Err(SocialError::Self_));
}

#[test]
fn accepting_without_a_pending_request_errors() {
    let s = with_users(&["alice", "bob"]);
    assert_eq!(
        s.social().accept_friend("bob", "alice"),
        Err(SocialError::NoPendingRequest)
    );
}

#[test]
fn only_the_addressee_may_accept_a_request() {
    let s = with_users(&["alice", "bob"]);
    s.social().request_friend("alice", "bob").unwrap();
    // The requester cannot accept their own request.
    assert_eq!(
        s.social().accept_friend("alice", "bob"),
        Err(SocialError::NoPendingRequest)
    );
    assert!(!s.social().are_friends("alice", "bob"));
}

#[test]
fn incoming_requests_and_friends_lists_reflect_state() {
    let s = with_users(&["alice", "bob", "carol"]);
    s.social().request_friend("bob", "alice").unwrap();
    s.social().request_friend("carol", "alice").unwrap();

    let mut incoming = s.social().incoming_friend_requests("alice");
    incoming.sort();
    assert_eq!(incoming, vec!["bob".to_string(), "carol".to_string()]);

    s.social().accept_friend("alice", "bob").unwrap();
    assert_eq!(s.social().friends_of("alice"), vec!["bob".to_string()]);
    assert_eq!(s.social().incoming_friend_requests("alice"), vec!["carol".to_string()]);
}

#[test]
fn by_id_friend_request_then_accept_enables_a_friends_only_dm() {
    // The full forwarded-friendship path each home runs: request by id, accept by id,
    // then the friends-only gate opens. Proves the by-id methods drive the same graph
    // as the by-handle ones the single-box path uses.
    let s = with_users(&["alice", "bob"]);
    let alice = s.find_user("alice").unwrap().id;
    let bob = s.find_user("bob").unwrap().id;
    s.social().set_dm_policy("bob", DmPolicy::FriendsOnly).unwrap();

    // Strangers can't reach a friends-only user.
    assert_eq!(send(&s, "alice", "bob", "hi"), Err(DmError::NotAllowed));

    s.social().request_friend_by_id(alice.0, bob.0).unwrap();
    assert_eq!(s.social().incoming_friend_requests("bob"), vec!["alice".to_string()]);
    s.social().accept_friend_by_id(bob.0, alice.0).unwrap();

    assert!(s.social().are_friends("alice", "bob"));
    assert!(send(&s, "alice", "bob", "hi").is_ok());
}

#[test]
fn a_by_id_friend_request_is_idempotent() {
    // A request commits on BOTH homes and may be re-driven — re-affirming converges.
    let s = with_users(&["alice", "bob"]);
    let alice = s.find_user("alice").unwrap().id;
    let bob = s.find_user("bob").unwrap().id;
    s.social().request_friend_by_id(alice.0, bob.0).unwrap();
    s.social().request_friend_by_id(alice.0, bob.0).expect("re-affirming is a no-op");
    assert_eq!(s.social().incoming_friend_requests("bob"), vec!["alice".to_string()]);
}

#[test]
fn only_the_addressee_may_accept_a_request_by_id() {
    let s = with_users(&["alice", "bob"]);
    let alice = s.find_user("alice").unwrap().id;
    let bob = s.find_user("bob").unwrap().id;
    s.social().request_friend_by_id(alice.0, bob.0).unwrap();
    // The requester (alice) cannot accept their own request.
    assert_eq!(s.social().accept_friend_by_id(alice.0, bob.0), Err(SocialError::NoPendingRequest));
    assert!(!s.social().are_friends("alice", "bob"));
}

#[test]
fn dm_partners_are_listed_most_recent_first() {
    let s = with_users(&["alice", "bob", "carol"]);
    send(&s, "alice", "bob", "hi bob").unwrap();
    send(&s, "alice", "carol", "hi carol").unwrap();
    // carol is the most recent partner.
    assert_eq!(
        s.social().dm_partners("alice"),
        vec!["carol".to_string(), "bob".to_string()]
    );
}

#[test]
fn dm_policy_change_persists() {
    let s = with_users(&["alice"]);
    assert_eq!(s.social().dm_policy("alice"), Some(DmPolicy::Everyone));
    s.social().set_dm_policy("alice", DmPolicy::FriendsOnly).unwrap();
    assert_eq!(s.social().dm_policy("alice"), Some(DmPolicy::FriendsOnly));
}
