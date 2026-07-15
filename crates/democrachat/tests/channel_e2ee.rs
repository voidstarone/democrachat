//! Integration tests for encrypted channels (M7.4): message bodies sealed under a
//! channel key, per-member key grants, and the two history modes. The whole crypto
//! runs client-side here (as it will in the real client); the server only ever
//! holds opaque ciphertext and sealed grants. Lives in the composition-root crate
//! because that is where `app` (crypto + use-cases) and the store meet.

use std::collections::HashMap;
use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{
    open_channel_message, open_sealed, seal_channel_message, seal_to, ChannelKey, ChannelKeyError,
    IdentitySecret, MessageError, PublicIdentity, Services,
};
use domain::{HistoryMode, Timestamp, WrappedKey};

const DAY: i64 = 86_400;

/// A deterministic per-handle device key (stable stand-in for a real client key),
/// so a test can seal to a user and, as that user, open.
fn secret_for(handle: &str) -> IdentitySecret {
    let base = handle.bytes().fold(1u8, |a, b| a.wrapping_add(b));
    let mut hex = String::with_capacity(64);
    for i in 0..32u8 {
        hex.push_str(&format!("{:02x}", base.wrapping_mul(3).wrapping_add(i).wrapping_add(7)));
    }
    IdentitySecret::from_hex(&hex).unwrap()
}

/// A server founded by `founder` (citizen #1) with the other handles joined as
/// members, everyone's device key published, and one channel created.
fn setup(founder: &str, members: &[&str], channel: &str) -> Services {
    let store = Arc::new(MemoryStore::new());
    let s = Services::new(Arc::new(FixedClock::new(Timestamp(100 * DAY))), store.as_stores());
    for h in std::iter::once(&founder).chain(members.iter()) {
        s.register_account(h).unwrap();
        let secret = secret_for(h);
        s.publish_keys(
            h,
            &secret.public().to_hex(),
            WrappedKey { salt: "00".into(), nonce: "00".into(), ciphertext: "00".into() },
        )
        .unwrap();
    }
    s.found_server(founder, "Town Square").unwrap();
    for m in members {
        s.join_server(m, "town-square").unwrap();
    }
    s.create_channel(founder, "town-square", channel, "").unwrap();
    s
}

/// Client-side: seal the channel `key` for `member`'s device key and file the grant.
fn grant(s: &Services, granter: &str, channel: &str, epoch: u32, member: &str, key: &ChannelKey) {
    let member_pub = PublicIdentity::from_hex(&s.public_key_of(member).unwrap()).unwrap();
    let sealed = hex::encode(seal_to(&member_pub, &key.to_bytes()));
    s.grant_channel_key(granter, "town-square", channel, epoch, member, &sealed).unwrap();
}

/// Client-side: seal `text` under the epoch `key` and post it.
fn post(s: &Services, sender: &str, channel: &str, text: &str, epoch: u32, key: &ChannelKey) {
    let ct = hex::encode(seal_channel_message(key, text.as_bytes()));
    s.post_sealed_message(sender, "town-square", channel, &ct, epoch, None).unwrap();
}

/// Client-side read: recover every channel key `viewer` was granted, then decrypt
/// each message — `None` for a message whose epoch key the viewer doesn't hold.
fn read(s: &Services, viewer: &str, channel: &str) -> Vec<Option<String>> {
    let secret = secret_for(viewer);
    let mut keys: HashMap<u32, ChannelKey> = HashMap::new();
    for g in s.my_channel_grants(viewer, "town-square", channel).unwrap() {
        if let Ok(raw) = open_sealed(&secret, &hex::decode(&g.sealed_key).unwrap()) {
            if let Ok(bytes) = <[u8; 32]>::try_from(raw) {
                keys.insert(g.epoch, ChannelKey::from_bytes(bytes));
            }
        }
    }
    s.channel_messages("town-square", channel)
        .unwrap()
        .into_iter()
        .map(|m| {
            let key = keys.get(&m.key_epoch?)?;
            let bytes = hex::decode(&m.body).ok()?;
            String::from_utf8(open_channel_message(key, &bytes).ok()?).ok()
        })
        .collect()
}

#[test]
fn open_history_lets_a_new_member_read_the_backlog() {
    let s = setup("ada", &["bob"], "general");
    s.enable_channel_encryption("ada", "town-square", "general", HistoryMode::Open).unwrap();

    // One long-lived key (epoch 0), granted to the current members.
    let key0 = ChannelKey::generate();
    grant(&s, "ada", "general", 0, "ada", &key0);
    grant(&s, "ada", "general", 0, "bob", &key0);
    post(&s, "ada", "general", "welcome to the room", 0, &key0);

    assert_eq!(read(&s, "bob", "general"), vec![Some("welcome to the room".to_string())]);

    // Carol joins later; in Open mode she is granted the same long-lived key and can
    // read the whole backlog.
    s.register_account("carol").unwrap();
    let carol_secret = secret_for("carol");
    s.publish_keys(
        "carol",
        &carol_secret.public().to_hex(),
        WrappedKey { salt: "00".into(), nonce: "00".into(), ciphertext: "00".into() },
    )
    .unwrap();
    s.join_server("carol", "town-square").unwrap();
    grant(&s, "ada", "general", 0, "carol", &key0);

    assert_eq!(read(&s, "carol", "general"), vec![Some("welcome to the room".to_string())]);
}

#[test]
fn ephemeral_history_hides_messages_sent_before_a_member_joined() {
    let s = setup("ada", &["bob"], "secret");
    s.enable_channel_encryption("ada", "town-square", "secret", HistoryMode::Ephemeral).unwrap();

    // Epoch 0: the founding members.
    let key0 = ChannelKey::generate();
    grant(&s, "ada", "secret", 0, "ada", &key0);
    grant(&s, "ada", "secret", 0, "bob", &key0);
    post(&s, "ada", "secret", "before carol", 0, &key0);

    // Carol joins → the key ratchets to epoch 1, granted only to current members.
    s.register_account("carol").unwrap();
    let carol_secret = secret_for("carol");
    s.publish_keys(
        "carol",
        &carol_secret.public().to_hex(),
        WrappedKey { salt: "00".into(), nonce: "00".into(), ciphertext: "00".into() },
    )
    .unwrap();
    s.join_server("carol", "town-square").unwrap();
    let key1 = ChannelKey::generate();
    for m in ["ada", "bob", "carol"] {
        grant(&s, "ada", "secret", 1, m, &key1);
    }
    post(&s, "ada", "secret", "after carol", 1, &key1);

    // Carol holds only epoch 1: she reads the post-join message, not the earlier one.
    assert_eq!(
        read(&s, "carol", "secret"),
        vec![None, Some("after carol".to_string())],
        "carol cannot open the pre-join epoch-0 message",
    );
    // Bob was there for both epochs and reads everything.
    assert_eq!(
        read(&s, "bob", "secret"),
        vec![Some("before carol".to_string()), Some("after carol".to_string())],
    );
}

#[test]
fn the_stored_channel_message_is_ciphertext_only() {
    let s = setup("ada", &["bob"], "general");
    s.enable_channel_encryption("ada", "town-square", "general", HistoryMode::Open).unwrap();
    let key0 = ChannelKey::generate();
    grant(&s, "ada", "general", 0, "ada", &key0);
    post(&s, "ada", "general", "top-secret-phrase", 0, &key0);

    let stored = &s.channel_messages("town-square", "general").unwrap()[0];
    assert_eq!(stored.key_epoch, Some(0));
    assert!(!stored.body.contains("top-secret-phrase"), "the server holds ciphertext only");
}

#[test]
fn a_plaintext_post_to_an_encrypted_channel_is_rejected() {
    let s = setup("ada", &["bob"], "general");
    s.enable_channel_encryption("ada", "town-square", "general", HistoryMode::Open).unwrap();
    assert_eq!(
        s.post_message("ada", "town-square", "general", "hi"),
        Err(MessageError::ChannelEncrypted),
    );
}

#[test]
fn a_sealed_post_to_a_plaintext_channel_is_rejected() {
    let s = setup("ada", &["bob"], "general");
    assert_eq!(
        s.post_sealed_message("ada", "town-square", "general", "deadbeef", 0, None),
        Err(MessageError::NotEncrypted),
    );
}

#[test]
fn only_a_citizen_may_enable_channel_encryption() {
    let s = setup("ada", &["bob"], "general");
    // bob is a member, not a citizen.
    assert_eq!(
        s.enable_channel_encryption("bob", "town-square", "general", HistoryMode::Open),
        Err(ChannelKeyError::NotACitizen),
    );
}
