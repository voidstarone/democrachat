//! Integration tests for the tag use-cases (label + discover servers, channels,
//! and users) against the real in-memory store.

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::{Services, TagError};
use domain::Timestamp;

const DAY: i64 = 86_400;

fn services() -> Services {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(100 * DAY)));
    Services::new(clock, store.as_stores())
}

#[tokio::test]
async fn a_founder_tags_a_server_and_it_is_discoverable() {
    let s = services();
    s.register_account("alice").await.unwrap();
    s.found_server("alice", "Gamers").await.unwrap();

    s.tags().set_server_tags("alice", "gamers", "Rust, gaming").await.unwrap();

    let hits = s.tags().servers_with_tag("rust").await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].slug, "gamers");
    assert_eq!(hits[0].tags.iter().collect::<Vec<_>>(), vec!["gaming", "rust"]);

    // The fences make it an exact-tag match, not a prefix one.
    assert!(s.tags().servers_with_tag("rus").await.unwrap().is_empty());
    assert!(s.tags().servers_with_tag("rustlang").await.unwrap().is_empty());
}

#[tokio::test]
async fn only_the_founder_may_tag_a_server() {
    let s = services();
    s.register_account("alice").await.unwrap();
    s.register_account("mallory").await.unwrap();
    s.found_server("alice", "Gamers").await.unwrap();

    let err = s.tags().set_server_tags("mallory", "gamers", "spam").await.unwrap_err();
    assert_eq!(err, TagError::Forbidden);
    assert!(s.tags().servers_with_tag("spam").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_founder_tags_a_channel_and_it_is_discoverable() {
    let s = services();
    s.register_account("alice").await.unwrap();
    s.found_server("alice", "Gamers").await.unwrap();
    s.chat().create_channel("alice", "gamers", "strategy", "").await.unwrap();

    s.tags().set_channel_tags("alice", "gamers", "strategy", "chess, go").await.unwrap();

    let hits = s.tags().channels_with_tag("chess").await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "strategy");
    assert!(s.tags().channels_with_tag("checkers").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_user_tags_their_own_account_and_is_discoverable() {
    let s = services();
    s.register_account("alice").await.unwrap();

    s.tags().set_user_tags("alice", "rustacean, gardener").await.unwrap();

    let hits = s.tags().users_with_tag("gardener").await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].handle, "alice");
}

#[tokio::test]
async fn setting_tags_replaces_the_previous_set() {
    let s = services();
    s.register_account("alice").await.unwrap();
    s.found_server("alice", "Gamers").await.unwrap();

    s.tags().set_server_tags("alice", "gamers", "rust").await.unwrap();
    s.tags().set_server_tags("alice", "gamers", "python").await.unwrap();

    assert!(s.tags().servers_with_tag("rust").await.unwrap().is_empty(), "old tag is gone");
    assert_eq!(s.tags().servers_with_tag("python").await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_blank_search_term_matches_nothing() {
    let s = services();
    s.register_account("alice").await.unwrap();
    s.found_server("alice", "Gamers").await.unwrap();
    s.tags().set_server_tags("alice", "gamers", "rust").await.unwrap();

    assert!(s.tags().servers_with_tag("   ").await.unwrap().is_empty());
    assert!(s.tags().servers_with_tag("|").await.unwrap().is_empty());
}
