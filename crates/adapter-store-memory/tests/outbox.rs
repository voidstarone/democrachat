//! The store's change-capture outbox drives real replication: mutations made
//! through the ordinary driven ports become a signed feed (`sign_feed`) that a
//! peer applies through the same authorize→apply gate (`ingest`) it uses for any
//! untrusted peer. This exercises the whole producer half of M5 end-to-end with
//! the real store, not a fake source.

use std::sync::{Arc, Mutex};

use adapter_store_memory::MemoryStore;
use app::{ChannelStore, DmStore, MessageStore, ServerStore};
use async_trait::async_trait;
use domain::{Channel, DmMessage, Message, NodeId, Server, ServerId, Timestamp, UserId};
use federation::{
    ingest, sign_feed, ChangeSink, ChangeSource, InMemoryRegistry, NodeKeypair, OwnedScope,
    OwnershipRegistry, ScopeResolver, SignedPart,
};

/// This test's servers carry their scope directly, so no parent lookup is needed.
struct NoParents;
#[async_trait]
impl ScopeResolver for NoParents {
    async fn proposal_server(&self, _: u64) -> Option<u64> {
        None
    }
    async fn message_server(&self, _: u64) -> Option<u64> {
        None
    }
}

/// Records the `(entity, id)` of every applied change, in order.
#[derive(Default)]
struct RecordingSink(Mutex<Vec<(String, u64)>>);
#[async_trait]
impl ChangeSink for RecordingSink {
    async fn apply(&self, part: &SignedPart) -> Result<(), String> {
        let id = part
            .payload
            .get("id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        self.0.lock().unwrap().push((part.entity.clone(), id));
        Ok(())
    }
}

#[tokio::test]
async fn store_mutations_replicate_through_sign_feed_and_ingest() {
    // Node A owns Server(7); one shared control plane models A and B agreeing on
    // ownership and A's public key.
    let node_a = NodeKeypair::generate(NodeId(1));
    let reg = InMemoryRegistry::new();
    reg.publish_key(NodeId(1), &node_a.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap();

    // A's store, on node 1. Make ordinary mutations through the driven ports.
    let store = Arc::new(MemoryStore::new().with_node(NodeId(1)));
    let now = Timestamp(1_000);
    let server_id = ServerId(7);
    ServerStore::insert_server(&*store, Server::new(server_id, "town", "Town", UserId(1), now));

    let chan_id = ChannelStore::next_channel_id(&*store);
    ChannelStore::insert_channel(&*store, Channel::new(chan_id, server_id, "general", "", now));

    let msg_id = MessageStore::next_message_id(&*store);
    MessageStore::insert_message(
        &*store,
        Message::new(msg_id, chan_id, server_id, UserId(1), "hello", None, now),
    );

    // Produce A's signed feed straight from the store's outbox…
    let feed = sign_feed(&*store, &node_a, &reg, &NoParents, 0, 100).await;
    assert_eq!(feed.len(), 3, "server + channel + message were all captured");
    // Every event is stamped for the scope A owns.
    for event in &feed {
        assert!(event.verify(&node_a.public()).is_ok(), "A signs its own feed");
    }

    // …and let node B ingest it against the same control plane.
    let sink = RecordingSink::default();
    let res = ingest(&reg, &NoParents, &sink, &feed).await;
    assert_eq!(res.applied, 3);
    assert!(res.rejected.is_empty(), "nothing is rejected");
    let applied = sink.0.lock().unwrap().clone();
    assert_eq!(
        applied,
        vec![
            ("servers".to_string(), server_id.0),
            ("channels".to_string(), chan_id.0),
            ("messages".to_string(), msg_id.0),
        ],
        "changes replicate in write order",
    );
}

#[tokio::test]
async fn a_peer_store_applies_the_feed_without_echoing_it() {
    // Node A owns Server(7); node B replicates it. One shared control plane.
    let node_a = NodeKeypair::generate(NodeId(1));
    let reg = InMemoryRegistry::new();
    reg.publish_key(NodeId(1), &node_a.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap();

    // A authors a server, a channel, and a message.
    let store_a = Arc::new(MemoryStore::new().with_node(NodeId(1)));
    let now = Timestamp(1_000);
    let sid = ServerId(7);
    ServerStore::insert_server(&*store_a, Server::new(sid, "town", "Town", UserId(1), now));
    let chan = ChannelStore::next_channel_id(&*store_a);
    ChannelStore::insert_channel(&*store_a, Channel::new(chan, sid, "general", "", now));
    let msg = MessageStore::next_message_id(&*store_a);
    MessageStore::insert_message(&*store_a, Message::new(msg, chan, sid, UserId(1), "hi", None, now));

    let feed = sign_feed(&*store_a, &node_a, &reg, &NoParents, 0, 100).await;

    // Node B is a fresh store on node 2; it applies A's feed through the real sink.
    let store_b = Arc::new(MemoryStore::new().with_node(NodeId(2)));
    let res = ingest(&reg, &NoParents, &*store_b, &feed).await;
    assert_eq!(res.applied, 3);
    assert!(res.rejected.is_empty());

    // B's replica now holds A's rows verbatim…
    assert_eq!(ServerStore::get_server(&*store_b, sid).map(|s| s.name), Some("Town".to_string()));
    assert_eq!(ChannelStore::get_channel(&*store_b, chan).map(|c| c.name), Some("general".to_string()));
    assert_eq!(MessageStore::get_message(&*store_b, msg).map(|m| m.body), Some("hi".to_string()));

    // …and applying a peer's feed must NOT put those rows on B's own outbox, or the
    // change would loop around the network forever.
    let b_outbox = ChangeSource::changes_since(&*store_b, 0, 100).await;
    assert!(b_outbox.is_empty(), "an ingested row must not echo into the peer's outbox");
}

/// A direct message replicates as **ciphertext only**: the body a node stores and
/// puts on the wire is the sealed blob the client produced — the plaintext never
/// enters the payload, and a replicating node holds a message it cannot read.
#[tokio::test]
async fn a_sealed_dm_replicates_as_ciphertext_only() {
    // Node A homes user 1 (the sender); the DM's scope is that user's home.
    let node_a = NodeKeypair::generate(NodeId(1));
    let reg = InMemoryRegistry::new();
    reg.publish_key(NodeId(1), &node_a.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::UserHome(1), NodeId(1)).await.unwrap();

    let store_a = Arc::new(MemoryStore::new().with_node(NodeId(1)));
    let now = Timestamp(1_000);
    let dm_id = DmStore::next_dm_id(&*store_a);
    // The client already sealed the body; the store only ever sees these blobs.
    DmStore::insert_dm(
        &*store_a,
        DmMessage::new(dm_id, UserId(1), UserId(2), "5ea1edforbob00", "5ea1edforalice00", now),
    );

    // The signed feed carries the ciphertext payload — no `body`, no plaintext.
    let feed = sign_feed(&*store_a, &node_a, &reg, &NoParents, 0, 100).await;
    assert_eq!(feed.len(), 1);
    let part = feed[0].peek().unwrap();
    assert_eq!(part.entity, "dms");
    assert!(part.payload.get("body").is_none(), "no cleartext body field on the wire");
    assert_eq!(
        part.payload.get("sealed_for_recipient").and_then(|v| v.as_str()),
        Some("5ea1edforbob00"),
    );

    // Node B ingests and holds the row verbatim — still ciphertext it cannot open.
    let store_b = Arc::new(MemoryStore::new().with_node(NodeId(2)));
    let res = ingest(&reg, &NoParents, &*store_b, &feed).await;
    assert_eq!(res.applied, 1);
    let convo = DmStore::conversation(&*store_b, UserId(1), UserId(2));
    assert_eq!(convo.len(), 1);
    assert_eq!(convo[0].sealed_for_recipient, "5ea1edforbob00");
    assert_eq!(convo[0].sealed_for_sender, "5ea1edforalice00");
}

#[tokio::test]
async fn a_cursor_pull_only_returns_changes_after_it() {
    let node_a = NodeKeypair::generate(NodeId(1));
    let reg = InMemoryRegistry::new();
    reg.publish_key(NodeId(1), &node_a.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap();

    let store = Arc::new(MemoryStore::new().with_node(NodeId(1)));
    let now = Timestamp(1_000);
    ServerStore::insert_server(&*store, Server::new(ServerId(7), "town", "Town", UserId(1), now));
    let chan_id = ChannelStore::next_channel_id(&*store);
    ChannelStore::insert_channel(&*store, Channel::new(chan_id, ServerId(7), "general", "", now));

    // A peer that has already applied the first change pulls from cursor 1 and sees
    // only the second.
    let feed = sign_feed(&*store, &node_a, &reg, &NoParents, 1, 100).await;
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0].peek().unwrap().entity, "channels");
    assert_eq!(feed[0].peek().unwrap().seq, 2);
}
