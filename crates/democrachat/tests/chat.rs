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

async fn contribution(f: &Fixture, handle: &str) -> i64 {
    let user = f.store.find_by_handle(handle).await.unwrap().unwrap();
    let server = f.store.find_by_slug("gamers").await.unwrap().unwrap();
    f.store.get(user.id, server.id).await.unwrap().unwrap().contribution
}

#[tokio::test]
async fn backfill_gives_channelless_servers_their_floor_channels() {
    use app::ServerStore;
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(1_000 * DAY)));
    let services = Services::new(clock.clone(), store.as_stores());
    // A server persisted the old way — inserted straight into the store with no
    // channels (as older datasets hold).
    let sid = store.next_server_id().await.unwrap();
    store.insert_server(domain::Server::new(sid, "old-town", "Old Town", domain::UserId(1), Timestamp(1_000 * DAY))).await.unwrap();
    let _ = &clock;
    assert!(services.chat().list_channels("old-town").await.unwrap().is_empty(), "starts channelless");

    services.backfill_default_channels().await;
    let names: Vec<String> =
        services.chat().list_channels("old-town").await.unwrap().into_iter().map(|c| c.name).collect();
    assert!(names.contains(&"general".to_string()), "#general backfilled");
    assert!(names.contains(&"appeals".to_string()), "#appeals backfilled");

    // Idempotent: a second pass adds nothing.
    services.backfill_default_channels().await;
    assert_eq!(services.chat().list_channels("old-town").await.unwrap().len(), 2);
}

/// A founder starts as a citizen (the bootstrap), so their reactions endorse.
async fn seed_server(f: &Fixture) {
    f.services.register_account("alice").await.unwrap();
    f.services.found_server("alice", "Gamers").await.unwrap();
    let user = f.store.find_by_handle("alice").await.unwrap().unwrap();
    let server = f.store.find_by_slug("gamers").await.unwrap().unwrap();
    assert_eq!(f.store.get(user.id, server.id).await.unwrap().unwrap().tier, Tier::Citizen);
}

#[tokio::test]
async fn a_citizen_reaction_endorses_the_author_but_a_members_does_not() {
    let f = fixture(1_000 * DAY);
    seed_server(&f).await;
    f.services.register_account("bob").await.unwrap();
    f.services.register_account("carol").await.unwrap();
    f.services.join_server("bob", "gamers").await.unwrap();
    f.services.join_server("carol", "gamers").await.unwrap();

    let msg = f.services.chat().post_message("bob", "gamers", "general", "hello world").await.unwrap();

    // carol is only a Member — her reaction must NOT endorse.
    f.services.chat().react("carol", msg.id.0, "👍").await.unwrap();
    assert_eq!(contribution(&f, "bob").await, 0, "a member's reaction is not an endorsement");

    // alice is a Citizen — her reaction endorses bob once.
    f.services.chat().react("alice", msg.id.0, "🎉").await.unwrap();
    assert_eq!(contribution(&f, "bob").await, 1);
    // A second emoji from the same citizen does not double-count.
    f.services.chat().react("alice", msg.id.0, "🚀").await.unwrap();
    assert_eq!(contribution(&f, "bob").await, 1);

    // Withdrawing one of two emojis keeps the endorsement...
    f.services.chat().unreact("alice", msg.id.0, "🎉").await.unwrap();
    assert_eq!(contribution(&f, "bob").await, 1);
    // ...clearing the last withdraws it.
    f.services.chat().unreact("alice", msg.id.0, "🚀").await.unwrap();
    assert_eq!(contribution(&f, "bob").await, 0);
}

#[tokio::test]
async fn self_reactions_never_endorse() {
    let f = fixture(1_000 * DAY);
    seed_server(&f).await;
    let msg = f.services.chat().post_message("alice", "gamers", "general", "look at me").await.unwrap();
    f.services.chat().react("alice", msg.id.0, "👍").await.unwrap();
    assert_eq!(contribution(&f, "alice").await, 0);
}

#[tokio::test]
async fn only_the_author_may_edit() {
    let f = fixture(1_000 * DAY);
    seed_server(&f).await;
    f.services.register_account("bob").await.unwrap();
    f.services.join_server("bob", "gamers").await.unwrap();
    let msg = f.services.chat().post_message("bob", "gamers", "general", "typo").await.unwrap();
    assert!(f.services.chat().edit_message("alice", msg.id.0, "hijack").await.is_err());
    assert!(f.services.chat().edit_message("bob", msg.id.0, "fixed").await.is_ok());
}

#[tokio::test]
async fn non_members_cannot_post_and_only_the_founder_provisions_in_seed() {
    let f = fixture(1_000 * DAY);
    seed_server(&f).await;
    f.services.register_account("stranger").await.unwrap();
    // A non-founder cannot provision a channel even in Seed.
    assert!(f.services.chat().create_channel("stranger", "gamers", "random", "").await.is_err());
    // A non-member cannot post.
    assert!(f.services.chat().post_message("stranger", "gamers", "general", "hi").await.is_err());
}

#[tokio::test]
async fn replies_thread_under_their_parent() {
    let f = fixture(1_000 * DAY);
    seed_server(&f).await;
    let root = f.services.chat().post_message("alice", "gamers", "general", "topic").await.unwrap();
    let reply = f.services.chat().reply_message("alice", root.id.0, "reply").await.unwrap();
    assert_eq!(reply.parent, Some(root.id));

    let msgs = f.services.chat().channel_messages("gamers", "general").await.unwrap();
    let tree = build_message_tree(&msgs);
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].replies.len(), 1);
    assert_eq!(tree[0].replies[0].message.id, reply.id);
}

#[tokio::test]
async fn a_deleted_message_is_tombstoned_and_keeps_its_replies() {
    let f = fixture(1_000 * DAY);
    seed_server(&f).await;
    let root = f.services.chat().post_message("alice", "gamers", "general", "parent").await.unwrap();
    let reply = f.services.chat().reply_message("alice", root.id.0, "child").await.unwrap();
    f.services.chat().delete_message("alice", root.id.0).await.unwrap();

    let msgs = f.services.chat().channel_messages("gamers", "general").await.unwrap();
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
#[tokio::test]
async fn history_sharing_hides_a_members_past_messages_from_later_joiners() {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(1_000 * DAY)));
    let s = Services::new(clock.clone(), store.as_stores());
    s.register_account("alice").await.unwrap();
    s.found_server("alice", "Gamers").await.unwrap();
    s.register_account("bob").await.unwrap();
    s.join_server("bob", "gamers").await.unwrap();
    let early = s.chat().post_message("bob", "gamers", "general", "early hello").await.unwrap();

    // Carol joins ten days later — a "newcomer" relative to bob's early message.
    clock.set(Timestamp(1_010 * DAY));
    s.register_account("carol").await.unwrap();
    s.join_server("carol", "gamers").await.unwrap();
    let late = s.chat().post_message("bob", "gamers", "general", "later hello").await.unwrap();

    async fn ids(s: &Services, viewer: &str) -> Vec<u64> {
        s.chat().channel_messages_for(viewer, "gamers", "general").await.unwrap().iter().map(|m| m.id.0).collect()
    }

    // Default (sharing on): carol sees both.
    assert!(ids(&s, "carol").await.contains(&early.id.0), "sharing is on by default");

    // Bob turns sharing off.
    s.chat().set_history_sharing("bob", "gamers", false).await.unwrap();
    assert!(!ids(&s, "carol").await.contains(&early.id.0), "carol joined after the early message → now hidden");
    assert!(ids(&s, "carol").await.contains(&late.id.0), "a message posted after carol joined stays visible");
    assert!(ids(&s, "bob").await.contains(&early.id.0), "the author always sees their own messages");
    assert!(ids(&s, "alice").await.contains(&early.id.0), "a member present when it was posted keeps seeing it");
}

/// `store_media` validates uploads: it accepts real image/video/audio and rejects
/// empties, oversized files, non-media types, and SVG (a script vector).
#[tokio::test]
async fn store_media_validates_type_and_size() {
    let f = fixture(1_000 * DAY);
    seed_server(&f).await;
    // Accepts a supported type and classifies it.
    let (key, stored_ct, kind) = f.services.chat().store_media("image/png", b"\x89PNG fake bytes").unwrap();
    assert_eq!(stored_ct, "image/png", "the test fixture's passthrough transcoder stores images verbatim");
    assert_eq!(kind, domain::MediaKind::Image);
    assert!(f.services.chat().media_blob(&key).is_some(), "the blob is retrievable");
    assert_eq!(f.services.chat().media_blob(&key).unwrap().0, "image/png");
    // Rejections.
    assert_eq!(f.services.chat().store_media("image/png", b"").unwrap_err(), app::MediaError::Empty);
    assert!(matches!(
        f.services.chat().store_media("application/pdf", b"%PDF").unwrap_err(),
        app::MediaError::UnsupportedType(_)
    ));
    assert!(matches!(
        f.services.chat().store_media("image/svg+xml", b"<svg onload=alert(1)>").unwrap_err(),
        app::MediaError::UnsupportedType(_)
    ));
    // Markup relabelled as an allowed image type is still rejected on its bytes —
    // a script-bearing SVG cannot sneak through as `image/png` (leading BOM and
    // whitespace are skipped before the sniff).
    assert!(matches!(
        f.services.chat().store_media("image/png", b"\xEF\xBB\xBF  <svg onload=alert(1)></svg>").unwrap_err(),
        app::MediaError::UnsupportedType(_)
    ));
    let too_big = vec![0u8; 26 * 1024 * 1024];
    assert_eq!(f.services.chat().store_media("video/mp4", &too_big).unwrap_err(), app::MediaError::TooLarge);
}

/// A message may carry only so many attachments — the cap bounds message size and
/// the disk a single post can claim.
#[tokio::test]
async fn caps_attachments_per_message() {
    use domain::{Attachment, MediaKind};
    let f = fixture(1_000 * DAY);
    seed_server(&f).await;
    let mut eleven: Vec<Attachment> = Vec::new();
    for i in 0..11 {
        let (key, _, _) = f.services.chat().store_media("image/png", format!("img{i}").as_bytes()).unwrap();
        eleven.push(Attachment::new(key, "image/png", MediaKind::Image, "", false));
    }
    assert!(matches!(
        f.services.chat()
            .post_message_with_attachments("alice", "gamers", "general", "", eleven)
            .await
            .unwrap_err(),
        app::MessageError::TooManyAttachments(10)
    ));
    let mut ten: Vec<Attachment> = Vec::new();
    for i in 0..10 {
        let (key, _, _) = f.services.chat().store_media("image/png", format!("img{i}").as_bytes()).unwrap();
        ten.push(Attachment::new(key, "image/png", MediaKind::Image, "", false));
    }
    assert!(f
        .services.chat()
        .post_message_with_attachments("alice", "gamers", "general", "", ten)
        .await
        .is_ok());
}

/// Media lives and dies with its message: deleting a message deletes its blobs, and
/// a media-only message (empty body) is allowed. Attachments are refused on an
/// encrypted channel.
#[tokio::test]
async fn deleting_a_message_deletes_its_media() {
    use domain::{Attachment, MediaKind};
    let f = fixture(1_000 * DAY);
    seed_server(&f).await;
    let (key, _, _) = f.services.chat().store_media("image/png", b"bytes").unwrap();
    let att = Attachment::new(key.clone(), "image/png", MediaKind::Image, "", false);

    // A media-only message (empty body) is accepted.
    let msg = f
        .services.chat()
        .post_message_with_attachments("alice", "gamers", "general", "", vec![att])
        .await
        .unwrap();
    assert_eq!(msg.attachments.len(), 1);
    assert!(f.services.chat().media_blob(&key).is_some());

    f.services.chat().delete_message("alice", msg.id.0).await.unwrap();
    assert!(f.services.chat().media_blob(&key).is_none(), "the blob is gone after the message is deleted");
}

#[tokio::test]
async fn message_search_honours_discord_operators() {
    let f = fixture(1_000 * DAY);
    seed_server(&f).await; // alice (founder)
    f.services.register_account("bob").await.unwrap();
    f.services.join_server("bob", "gamers").await.unwrap();
    f.services.chat().create_channel("alice", "gamers", "notes", "").await.unwrap();
    f.services.chat().post_message("alice", "gamers", "general", "deploy the new build today").await.unwrap();
    f.services.chat().post_message("bob", "gamers", "general", "see https://example.com/plan for details").await.unwrap();
    f.services.chat().post_message("alice", "gamers", "general", "hey @bob can you deploy").await.unwrap();
    f.services.chat().post_message("alice", "gamers", "notes", "deploy runbook notes").await.unwrap();

    let chat = f.services.chat();
    // Plain term: matches across all visible channels.
    assert_eq!(chat.search_messages("alice", "gamers", "deploy").await.len(), 3);
    // from: + in: narrow to one author in one channel.
    let r = chat.search_messages("alice", "gamers", "from:alice in:notes").await;
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].channel_name, "notes");
    // in: scopes without a term.
    assert_eq!(chat.search_messages("alice", "gamers", "in:notes").await.len(), 1);
    // has:link finds the URL-bearing message.
    let r = chat.search_messages("alice", "gamers", "has:link").await;
    assert_eq!(r.len(), 1);
    assert!(r[0].message.body.contains("https://"));
    // to: matches a whole-token @mention (not @bobby).
    assert_eq!(chat.search_messages("alice", "gamers", "to:@bob").await.len(), 1);
    // before: a date preceding the fixture clock (~1972) excludes everything.
    assert!(chat.search_messages("alice", "gamers", "before:1970-06-01 deploy").await.is_empty());
    // after: that same early date keeps all current messages.
    assert_eq!(chat.search_messages("alice", "gamers", "after:1970-06-01 deploy").await.len(), 3);
    // A constraint-free query is refused rather than dumping the server.
    assert!(chat.search_messages("alice", "gamers", "   ").await.is_empty());
    // An unresolvable from: yields nothing.
    assert!(chat.search_messages("alice", "gamers", "from:nobody").await.is_empty());
    // Newest first.
    let r = chat.search_messages("alice", "gamers", "deploy").await;
    assert!(r.windows(2).all(|w| w[0].message.created_at.0 >= w[1].message.created_at.0));
}
