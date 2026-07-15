//! Integration tests for voice channels against the real in-memory store. A voice
//! channel is a superset of a text one — it carries the ordinary message stream and
//! *additionally* advertises `ChannelKind::Voice` for the live audio room (whose
//! roster/signaling lives in the web adapter, not here).

use std::sync::Arc;

use adapter_store_memory::{FixedClock, MemoryStore};
use app::Services;
use domain::{ChannelKind, Timestamp};

const DAY: i64 = 86_400;

fn services() -> Services {
    let store = Arc::new(MemoryStore::new());
    let clock = Arc::new(FixedClock::new(Timestamp(100 * DAY)));
    Services::new(clock, store.as_stores())
}

#[tokio::test]
async fn a_founder_creates_a_voice_channel_tagged_voice() {
    let s = services();
    s.register_account("alice").await.unwrap();
    s.found_server("alice", "Gamers").await.unwrap();

    let ch = s.chat().create_voice_channel("alice", "gamers", "lounge", "hang out").await.unwrap();
    assert_eq!(ch.kind, ChannelKind::Voice);
    assert!(ch.kind.is_voice());

    // It shows up in the channel list carrying its kind.
    let list = s.chat().list_channels("gamers").await.unwrap();
    let found = list.iter().find(|c| c.name == "lounge").unwrap();
    assert_eq!(found.kind, ChannelKind::Voice);
}

#[tokio::test]
async fn a_voice_channel_still_carries_the_text_message_stream() {
    let s = services();
    s.register_account("alice").await.unwrap();
    s.found_server("alice", "Gamers").await.unwrap();
    s.chat().create_voice_channel("alice", "gamers", "lounge", "").await.unwrap();

    s.chat().post_message("alice", "gamers", "lounge", "hi from voice").await.unwrap();

    let msgs = s.chat().channel_messages("gamers", "lounge").await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "hi from voice");
}

#[tokio::test]
async fn an_ordinary_create_defaults_to_text() {
    let s = services();
    s.register_account("alice").await.unwrap();
    s.found_server("alice", "Gamers").await.unwrap();

    let ch = s.chat().create_channel("alice", "gamers", "strategy", "").await.unwrap();
    assert_eq!(ch.kind, ChannelKind::Text);
    assert!(!ch.kind.is_voice());
}
