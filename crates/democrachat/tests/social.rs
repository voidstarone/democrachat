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

async fn with_users(handles: &[&str]) -> Services {
    let s = services(100 * DAY);
    for h in handles {
        s.register_account(h).await.unwrap();
        // Publish each user's device key so DMs can be sealed to them. The wrapped
        // secret is irrelevant to these tests (the server never opens it), so a
        // placeholder blob is fine — the directory validates only the public key.
        let secret = secret_for(h);
        s.keys().publish_keys(
            h,
            &secret.public().to_hex(),
            WrappedKey { salt: "00".into(), nonce: "00".into(), ciphertext: "00".into() },
        )
        .await
        .unwrap();
    }
    s
}

/// The client side of sending: fetch both parties' public keys, seal the plaintext
/// to each, and hand the server only ciphertext. Hex-encodes the sealed bytes for
/// transport (the server stores the string opaquely).
async fn send(s: &Services, from: &str, to: &str, text: &str) -> Result<DmMessage, DmError> {
    let to_pub = PublicIdentity::from_hex(&s.keys().public_key_of(to).await.unwrap()).unwrap();
    let from_pub = PublicIdentity::from_hex(&s.keys().public_key_of(from).await.unwrap()).unwrap();
    let for_recipient = hex::encode(seal_to(&to_pub, text.as_bytes()));
    let for_sender = hex::encode(seal_to(&from_pub, text.as_bytes()));
    s.social().send_sealed_dm(from, to, &for_recipient, &for_sender).await
}

/// The client side of reading: for each message, open the ciphertext sealed to the
/// viewer with the viewer's device key.
async fn read(s: &Services, viewer: &str, other: &str) -> Vec<String> {
    let viewer_id = s.find_user(viewer).await.unwrap().id;
    let secret = secret_for(viewer);
    s.social().conversation(viewer, other)
        .await
        .into_iter()
        .map(|m| {
            let ct = if m.sender == viewer_id { m.sealed_for_sender } else { m.sealed_for_recipient };
            let bytes = hex::decode(ct).unwrap();
            String::from_utf8(open_sealed(&secret, &bytes).unwrap()).unwrap()
        })
        .collect()
}

#[tokio::test]
async fn dms_are_on_by_default_between_strangers() {
    let s = with_users(&["alice", "bob"]).await;
    send(&s, "alice", "bob", "hey").await.unwrap();
    // Both parties can read the message — each opens their own sealed copy.
    assert_eq!(read(&s, "bob", "alice").await, vec!["hey".to_string()]);
    assert_eq!(read(&s, "alice", "bob").await, vec!["hey".to_string()]);
}

#[tokio::test]
async fn conversation_is_shared_regardless_of_direction() {
    let s = with_users(&["alice", "bob"]).await;
    send(&s, "alice", "bob", "one").await.unwrap();
    send(&s, "bob", "alice", "two").await.unwrap();
    assert_eq!(read(&s, "alice", "bob").await, vec!["one".to_string(), "two".to_string()]);
    assert_eq!(read(&s, "bob", "alice").await, vec!["one".to_string(), "two".to_string()]);
}

#[tokio::test]
async fn a_third_party_cannot_open_a_dm() {
    let s = with_users(&["alice", "bob", "eve"]).await;
    send(&s, "alice", "bob", "secret").await.unwrap();
    // Eve pulls the stored conversation but holds neither party's device key.
    let stored = s.social().conversation("alice", "bob").await;
    let eve = secret_for("eve");
    for m in &stored {
        assert!(open_sealed(&eve, &hex::decode(&m.sealed_for_recipient).unwrap()).is_err());
        assert!(open_sealed(&eve, &hex::decode(&m.sealed_for_sender).unwrap()).is_err());
    }
}

#[tokio::test]
async fn the_stored_dm_carries_no_plaintext() {
    let s = with_users(&["alice", "bob"]).await;
    send(&s, "alice", "bob", "rendezvous-at-dawn").await.unwrap();
    // The server-held record is ciphertext only — the plaintext never appears.
    let stored = s.social().conversation("alice", "bob").await;
    let m = &stored[0];
    assert!(!m.sealed_for_recipient.contains("rendezvous"));
    assert!(!m.sealed_for_sender.contains("rendezvous"));
}

#[tokio::test]
async fn cannot_dm_yourself() {
    let s = with_users(&["alice"]).await;
    assert_eq!(send(&s, "alice", "alice", "hi").await, Err(DmError::Self_));
}

#[tokio::test]
async fn a_dm_with_no_ciphertext_is_rejected() {
    let s = with_users(&["alice", "bob"]).await;
    // The server is blind to the body; it only insists a ciphertext is present.
    assert_eq!(s.social().send_sealed_dm("alice", "bob", "", "").await, Err(DmError::EmptyBody));
}

#[tokio::test]
async fn a_by_id_dm_lands_exactly_like_a_by_handle_dm() {
    // This is the path a forwarded `Command::SendDm` runs on the sender's home: the
    // owner authorizes by home-stable id, having never seen the caller's handles.
    let s = with_users(&["alice", "bob"]).await;
    let alice = s.find_user("alice").await.unwrap().id;
    let bob = s.find_user("bob").await.unwrap().id;

    let to_pub = PublicIdentity::from_hex(&s.keys().public_key_of("bob").await.unwrap()).unwrap();
    let from_pub = PublicIdentity::from_hex(&s.keys().public_key_of("alice").await.unwrap()).unwrap();
    let for_recipient = hex::encode(seal_to(&to_pub, b"by-id"));
    let for_sender = hex::encode(seal_to(&from_pub, b"by-id"));

    s.social().send_sealed_dm_by_id(alice.0, bob.0, &for_recipient, &for_sender).await.unwrap();

    // The recipient reads it back through the ordinary conversation view.
    assert_eq!(read(&s, "bob", "alice").await, vec!["by-id".to_string()]);
}

#[tokio::test]
async fn a_by_id_dm_still_honours_the_dm_gate() {
    // The owner re-checks `can_dm` itself — a forwarder cannot bypass friends-only
    // by routing the write to the recipient's home.
    let s = with_users(&["alice", "bob"]).await;
    s.social().set_dm_policy("bob", DmPolicy::FriendsOnly).await.unwrap();
    let alice = s.find_user("alice").await.unwrap().id;
    let bob = s.find_user("bob").await.unwrap().id;
    assert_eq!(
        s.social().send_sealed_dm_by_id(alice.0, bob.0, "aa", "bb").await,
        Err(DmError::NotAllowed),
    );
}

#[tokio::test]
async fn friends_only_blocks_strangers_but_allows_friends() {
    let s = with_users(&["alice", "bob"]).await;
    s.social().set_dm_policy("bob", DmPolicy::FriendsOnly).await.unwrap();

    // A stranger cannot reach a friends-only user.
    assert_eq!(send(&s, "alice", "bob", "hi").await, Err(DmError::NotAllowed));
    assert!(!s.social().can_dm("alice", "bob").await);

    // Become friends: request + accept.
    s.social().request_friend("alice", "bob").await.unwrap();
    s.social().accept_friend("bob", "alice").await.unwrap();
    assert!(s.social().are_friends("alice", "bob").await);

    // Now the DM goes through.
    assert!(s.social().can_dm("alice", "bob").await);
    assert!(send(&s, "alice", "bob", "hi").await.is_ok());
}

#[tokio::test]
async fn a_block_silences_dms_both_ways_permanently() {
    let s = with_users(&["alice", "bob"]).await;
    // They were even friends first.
    s.social().request_friend("alice", "bob").await.unwrap();
    s.social().accept_friend("bob", "alice").await.unwrap();

    s.social().block_user("alice", "bob").await.unwrap();

    // Neither direction works, whatever the policy.
    assert_eq!(send(&s, "alice", "bob", "hi").await, Err(DmError::NotAllowed));
    assert_eq!(send(&s, "bob", "alice", "hi").await, Err(DmError::NotAllowed));
    assert!(s.social().is_blocked_between("bob", "alice").await);
}

#[tokio::test]
async fn cannot_block_yourself() {
    let s = with_users(&["alice"]).await;
    assert_eq!(s.social().block_user("alice", "alice").await, Err(SocialError::Self_));
}

#[tokio::test]
async fn a_by_id_block_silences_dms_exactly_like_a_by_handle_block() {
    // This is what each of the two users' homes runs when a `Command::Block` is
    // forwarded: it authorizes by home-stable id, having never seen the handles.
    let s = with_users(&["alice", "bob"]).await;
    let alice = s.find_user("alice").await.unwrap().id;
    let bob = s.find_user("bob").await.unwrap().id;

    s.social().block_user_by_id(alice.0, bob.0).await.unwrap();

    assert!(s.social().is_blocked_between("alice", "bob").await);
    assert_eq!(send(&s, "alice", "bob", "hi").await, Err(DmError::NotAllowed));
    assert_eq!(send(&s, "bob", "alice", "hi").await, Err(DmError::NotAllowed));
}

#[tokio::test]
async fn a_by_id_block_is_idempotent() {
    // A block commits on BOTH homes (and a partial commit may be re-driven), so the
    // same block landing twice must converge on one record, never error.
    let s = with_users(&["alice", "bob"]).await;
    let alice = s.find_user("alice").await.unwrap().id;
    let bob = s.find_user("bob").await.unwrap().id;

    s.social().block_user_by_id(alice.0, bob.0).await.unwrap();
    s.social().block_user_by_id(alice.0, bob.0).await.expect("re-applying the same block is a no-op");
    assert!(s.social().is_blocked_between("alice", "bob").await);
}

#[tokio::test]
async fn a_by_id_self_block_is_refused() {
    let s = with_users(&["alice"]).await;
    let alice = s.find_user("alice").await.unwrap().id;
    assert_eq!(s.social().block_user_by_id(alice.0, alice.0).await, Err(SocialError::Self_));
}

#[tokio::test]
async fn accepting_without_a_pending_request_errors() {
    let s = with_users(&["alice", "bob"]).await;
    assert_eq!(
        s.social().accept_friend("bob", "alice").await,
        Err(SocialError::NoPendingRequest)
    );
}

#[tokio::test]
async fn only_the_addressee_may_accept_a_request() {
    let s = with_users(&["alice", "bob"]).await;
    s.social().request_friend("alice", "bob").await.unwrap();
    // The requester cannot accept their own request.
    assert_eq!(
        s.social().accept_friend("alice", "bob").await,
        Err(SocialError::NoPendingRequest)
    );
    assert!(!s.social().are_friends("alice", "bob").await);
}

#[tokio::test]
async fn incoming_requests_and_friends_lists_reflect_state() {
    let s = with_users(&["alice", "bob", "carol"]).await;
    s.social().request_friend("bob", "alice").await.unwrap();
    s.social().request_friend("carol", "alice").await.unwrap();

    let mut incoming = s.social().incoming_friend_requests("alice").await;
    incoming.sort();
    assert_eq!(incoming, vec!["bob".to_string(), "carol".to_string()]);

    s.social().accept_friend("alice", "bob").await.unwrap();
    assert_eq!(s.social().friends_of("alice").await, vec!["bob".to_string()]);
    assert_eq!(s.social().incoming_friend_requests("alice").await, vec!["carol".to_string()]);
}

#[tokio::test]
async fn by_id_friend_request_then_accept_enables_a_friends_only_dm() {
    // The full forwarded-friendship path each home runs: request by id, accept by id,
    // then the friends-only gate opens. Proves the by-id methods drive the same graph
    // as the by-handle ones the single-box path uses.
    let s = with_users(&["alice", "bob"]).await;
    let alice = s.find_user("alice").await.unwrap().id;
    let bob = s.find_user("bob").await.unwrap().id;
    s.social().set_dm_policy("bob", DmPolicy::FriendsOnly).await.unwrap();

    // Strangers can't reach a friends-only user.
    assert_eq!(send(&s, "alice", "bob", "hi").await, Err(DmError::NotAllowed));

    s.social().request_friend_by_id(alice.0, bob.0).await.unwrap();
    assert_eq!(s.social().incoming_friend_requests("bob").await, vec!["alice".to_string()]);
    s.social().accept_friend_by_id(bob.0, alice.0).await.unwrap();

    assert!(s.social().are_friends("alice", "bob").await);
    assert!(send(&s, "alice", "bob", "hi").await.is_ok());
}

#[tokio::test]
async fn a_by_id_friend_request_is_idempotent() {
    // A request commits on BOTH homes and may be re-driven — re-affirming converges.
    let s = with_users(&["alice", "bob"]).await;
    let alice = s.find_user("alice").await.unwrap().id;
    let bob = s.find_user("bob").await.unwrap().id;
    s.social().request_friend_by_id(alice.0, bob.0).await.unwrap();
    s.social().request_friend_by_id(alice.0, bob.0).await.expect("re-affirming is a no-op");
    assert_eq!(s.social().incoming_friend_requests("bob").await, vec!["alice".to_string()]);
}

#[tokio::test]
async fn only_the_addressee_may_accept_a_request_by_id() {
    let s = with_users(&["alice", "bob"]).await;
    let alice = s.find_user("alice").await.unwrap().id;
    let bob = s.find_user("bob").await.unwrap().id;
    s.social().request_friend_by_id(alice.0, bob.0).await.unwrap();
    // The requester (alice) cannot accept their own request.
    assert_eq!(s.social().accept_friend_by_id(alice.0, bob.0).await, Err(SocialError::NoPendingRequest));
    assert!(!s.social().are_friends("alice", "bob").await);
}

#[tokio::test]
async fn dm_partners_are_listed_most_recent_first() {
    let s = with_users(&["alice", "bob", "carol"]).await;
    send(&s, "alice", "bob", "hi bob").await.unwrap();
    send(&s, "alice", "carol", "hi carol").await.unwrap();
    // carol is the most recent partner.
    assert_eq!(
        s.social().dm_partners("alice").await,
        vec!["carol".to_string(), "bob".to_string()]
    );
}

#[tokio::test]
async fn dm_policy_change_persists() {
    let s = with_users(&["alice"]).await;
    assert_eq!(s.social().dm_policy("alice").await, Some(DmPolicy::Everyone));
    s.social().set_dm_policy("alice", DmPolicy::FriendsOnly).await.unwrap();
    assert_eq!(s.social().dm_policy("alice").await, Some(DmPolicy::FriendsOnly));
}
