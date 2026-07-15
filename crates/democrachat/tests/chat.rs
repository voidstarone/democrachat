//! Integration tests for the chat use-cases, exercised against the real
//! in-memory store. Lives in the composition-root crate because that is where
//! `app` and `adapter-store-memory` legitimately meet (no dependency cycle).

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{MembershipStore, ServerStore, Services, UserStore};
use domain::{build_message_tree, Tier, Timestamp};

const DAY: i64 = 86_400;

struct Fixture {
    services: Services,
    store: Arc<MemoryStore>,
}

fn fixture(now_secs: i64) -> Fixture {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(now_secs)));
    let services = Services::new(clock, store.as_stores());
    Fixture { services, store }
}

fn contribution(f: &Fixture, handle: &str) -> i64 {
    let user = f.store.find_by_handle(handle).unwrap();
    let server = f.store.find_by_slug("gamers").unwrap();
    f.store.get(user.id, server.id).unwrap().contribution
}

/// A founder starts as a citizen (the bootstrap), so their reactions endorse.
fn seed_server(f: &Fixture) {
    f.services.register_account("alice").unwrap();
    f.services.found_server("alice", "Gamers").unwrap();
    let user = f.store.find_by_handle("alice").unwrap();
    let server = f.store.find_by_slug("gamers").unwrap();
    assert_eq!(f.store.get(user.id, server.id).unwrap().tier, Tier::Citizen);
    f.services.create_channel("alice", "gamers", "general", "chat").unwrap();
}

#[test]
fn a_citizen_reaction_endorses_the_author_but_a_members_does_not() {
    let f = fixture(1_000 * DAY);
    seed_server(&f);
    f.services.register_account("bob").unwrap();
    f.services.register_account("carol").unwrap();
    f.services.join_server("bob", "gamers").unwrap();
    f.services.join_server("carol", "gamers").unwrap();

    let msg = f.services.post_message("bob", "gamers", "general", "hello world").unwrap();

    // carol is only a Member — her reaction must NOT endorse.
    f.services.react("carol", msg.id.0, "👍").unwrap();
    assert_eq!(contribution(&f, "bob"), 0, "a member's reaction is not an endorsement");

    // alice is a Citizen — her reaction endorses bob once.
    f.services.react("alice", msg.id.0, "🎉").unwrap();
    assert_eq!(contribution(&f, "bob"), 1);
    // A second emoji from the same citizen does not double-count.
    f.services.react("alice", msg.id.0, "🚀").unwrap();
    assert_eq!(contribution(&f, "bob"), 1);

    // Withdrawing one of two emojis keeps the endorsement...
    f.services.unreact("alice", msg.id.0, "🎉").unwrap();
    assert_eq!(contribution(&f, "bob"), 1);
    // ...clearing the last withdraws it.
    f.services.unreact("alice", msg.id.0, "🚀").unwrap();
    assert_eq!(contribution(&f, "bob"), 0);
}

#[test]
fn self_reactions_never_endorse() {
    let f = fixture(1_000 * DAY);
    seed_server(&f);
    let msg = f.services.post_message("alice", "gamers", "general", "look at me").unwrap();
    f.services.react("alice", msg.id.0, "👍").unwrap();
    assert_eq!(contribution(&f, "alice"), 0);
}

#[test]
fn only_the_author_may_edit() {
    let f = fixture(1_000 * DAY);
    seed_server(&f);
    f.services.register_account("bob").unwrap();
    f.services.join_server("bob", "gamers").unwrap();
    let msg = f.services.post_message("bob", "gamers", "general", "typo").unwrap();
    assert!(f.services.edit_message("alice", msg.id.0, "hijack").is_err());
    assert!(f.services.edit_message("bob", msg.id.0, "fixed").is_ok());
}

#[test]
fn non_members_cannot_post_and_only_the_founder_provisions_in_seed() {
    let f = fixture(1_000 * DAY);
    seed_server(&f);
    f.services.register_account("stranger").unwrap();
    // A non-founder cannot provision a channel even in Seed.
    assert!(f.services.create_channel("stranger", "gamers", "random", "").is_err());
    // A non-member cannot post.
    assert!(f.services.post_message("stranger", "gamers", "general", "hi").is_err());
}

#[test]
fn replies_thread_under_their_parent() {
    let f = fixture(1_000 * DAY);
    seed_server(&f);
    let root = f.services.post_message("alice", "gamers", "general", "topic").unwrap();
    let reply = f.services.reply_message("alice", root.id.0, "reply").unwrap();
    assert_eq!(reply.parent, Some(root.id));

    let msgs = f.services.channel_messages("gamers", "general").unwrap();
    let tree = build_message_tree(&msgs);
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].replies.len(), 1);
    assert_eq!(tree[0].replies[0].message.id, reply.id);
}

#[test]
fn a_deleted_message_is_tombstoned_and_keeps_its_replies() {
    let f = fixture(1_000 * DAY);
    seed_server(&f);
    let root = f.services.post_message("alice", "gamers", "general", "parent").unwrap();
    let reply = f.services.reply_message("alice", root.id.0, "child").unwrap();
    f.services.delete_message("alice", root.id.0).unwrap();

    let msgs = f.services.channel_messages("gamers", "general").unwrap();
    let tree = build_message_tree(&msgs);
    // Root survives as a tombstone with its reply still attached.
    assert_eq!(tree.len(), 1);
    assert!(tree[0].message.is_deleted);
    assert_eq!(tree[0].replies[0].message.id, reply.id);
}

/// A member's personal history-sharing setting hides the messages they posted
/// before a newcomer joined — for that newcomer only. The author and members who
/// were already present keep seeing everything; messages posted after the join
/// stay visible regardless. Sharing is on by default.
#[test]
fn history_sharing_hides_a_members_past_messages_from_later_joiners() {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(1_000 * DAY)));
    let s = Services::new(clock.clone(), store.as_stores());
    s.register_account("alice").unwrap();
    s.found_server("alice", "Gamers").unwrap();
    s.create_channel("alice", "gamers", "general", "chat").unwrap();
    s.register_account("bob").unwrap();
    s.join_server("bob", "gamers").unwrap();
    let early = s.post_message("bob", "gamers", "general", "early hello").unwrap();

    // Carol joins ten days later — a "newcomer" relative to bob's early message.
    clock.set(Timestamp(1_010 * DAY));
    s.register_account("carol").unwrap();
    s.join_server("carol", "gamers").unwrap();
    let late = s.post_message("bob", "gamers", "general", "later hello").unwrap();

    let ids = |viewer: &str| -> Vec<u64> {
        s.channel_messages_for(viewer, "gamers", "general").unwrap().iter().map(|m| m.id.0).collect()
    };

    // Default (sharing on): carol sees both.
    assert!(ids("carol").contains(&early.id.0), "sharing is on by default");

    // Bob turns sharing off.
    s.set_history_sharing("bob", "gamers", false).unwrap();
    assert!(!ids("carol").contains(&early.id.0), "carol joined after the early message → now hidden");
    assert!(ids("carol").contains(&late.id.0), "a message posted after carol joined stays visible");
    assert!(ids("bob").contains(&early.id.0), "the author always sees their own messages");
    assert!(ids("alice").contains(&early.id.0), "a member present when it was posted keeps seeing it");
}

/// `store_media` validates uploads: it accepts real image/video/audio and rejects
/// empties, oversized files, non-media types, and SVG (a script vector).
#[test]
fn store_media_validates_type_and_size() {
    let f = fixture(1_000 * DAY);
    seed_server(&f);
    // Accepts a supported type and classifies it.
    let (key, kind) = f.services.store_media("image/png", b"\x89PNG fake bytes").unwrap();
    assert_eq!(kind, domain::MediaKind::Image);
    assert!(f.services.media_blob(&key).is_some(), "the blob is retrievable");
    assert_eq!(f.services.media_blob(&key).unwrap().0, "image/png");
    // Rejections.
    assert_eq!(f.services.store_media("image/png", b"").unwrap_err(), app::MediaError::Empty);
    assert!(matches!(
        f.services.store_media("application/pdf", b"%PDF").unwrap_err(),
        app::MediaError::UnsupportedType(_)
    ));
    assert!(matches!(
        f.services.store_media("image/svg+xml", b"<svg onload=alert(1)>").unwrap_err(),
        app::MediaError::UnsupportedType(_)
    ));
    let too_big = vec![0u8; 26 * 1024 * 1024];
    assert_eq!(f.services.store_media("video/mp4", &too_big).unwrap_err(), app::MediaError::TooLarge);
}

/// Media lives and dies with its message: deleting a message deletes its blobs, and
/// a media-only message (empty body) is allowed. Attachments are refused on an
/// encrypted channel.
#[test]
fn deleting_a_message_deletes_its_media() {
    use domain::{Attachment, MediaKind};
    let f = fixture(1_000 * DAY);
    seed_server(&f);
    let (key, _) = f.services.store_media("image/png", b"bytes").unwrap();
    let att = Attachment::new(key.clone(), "image/png", MediaKind::Image, "", false);

    // A media-only message (empty body) is accepted.
    let msg = f
        .services
        .post_message_with_attachments("alice", "gamers", "general", "", vec![att])
        .unwrap();
    assert_eq!(msg.attachments.len(), 1);
    assert!(f.services.media_blob(&key).is_some());

    f.services.delete_message("alice", msg.id.0).unwrap();
    assert!(f.services.media_blob(&key).is_none(), "the blob is gone after the message is deleted");
}
