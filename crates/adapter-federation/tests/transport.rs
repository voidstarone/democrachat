//! End-to-end federation transport over a real TCP socket: node A serves its
//! signed feed, node B pulls it over HTTP, authorizes it, applies it, and advances
//! its cursor. This exercises the whole feed-pull half of M5 across the wire.

use std::sync::Arc;

use adapter_federation::{feed_router, FeedClient, Peer, Replicator, StoreResolver};
use adapter_federation::FeedState;
use adapter_store_memory::MemoryStore;
use app::{ChannelStore, ServerStore};
use domain::{Channel, NodeId, Server, ServerId, Timestamp, UserId};
use federation::{InMemoryRegistry, NodeKeypair, OwnedScope, OwnershipRegistry};

/// Bring up node A's feed server on an ephemeral port and return its base URL.
async fn serve_a(state: FeedState) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, feed_router(state)).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn node_b_replicates_node_a_over_http() {
    // Shared control plane: A owns Server(7); both nodes know A's key.
    let node_a = NodeKeypair::generate(NodeId(1));
    let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
    reg.publish_key(NodeId(1), &node_a.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap();

    // Node A's store + writes.
    let store_a = Arc::new(MemoryStore::new().with_node(NodeId(1)));
    let now = Timestamp(1_000);
    let sid = ServerId(7);
    ServerStore::insert_server(&*store_a, Server::new(sid, "town", "Town", UserId(1), now));
    let chan = ChannelStore::next_channel_id(&*store_a);
    ChannelStore::insert_channel(&*store_a, Channel::new(chan, sid, "general", "", now));

    // A's feed server.
    let feed_state = FeedState {
        store: store_a.clone(),
        keypair: Arc::new(node_a),
        registry: reg.clone(),
        resolver: Arc::new(StoreResolver(store_a.clone())),
        token: Some("cluster-secret".into()),
    };
    let base_url = serve_a(feed_state).await;

    // Node B: a fresh store, a replicator, and a client pointed at A.
    let store_b = Arc::new(MemoryStore::new().with_node(NodeId(2)));
    let replicator = Replicator::new(
        store_b.clone(),
        reg.clone(),
        Arc::new(StoreResolver(store_b.clone())),
    );
    let peer = Peer {
        node: NodeId(1),
        client: FeedClient::new(base_url, Some("cluster-secret".into())),
    };

    // One poll replicates both rows and advances B's cursor for A.
    let applied = adapter_federation::poll_peer(&replicator, &peer, 100).await.unwrap();
    assert_eq!(applied, 2);
    assert_eq!(replicator.cursor(NodeId(1)), 2, "cursor advanced over the applied prefix");
    assert_eq!(ServerStore::get_server(&*store_b, sid).map(|s| s.name), Some("Town".into()));
    assert_eq!(ChannelStore::get_channel(&*store_b, chan).map(|c| c.name), Some("general".into()));

    // A second poll from the advanced cursor has nothing new.
    let again = adapter_federation::poll_peer(&replicator, &peer, 100).await.unwrap();
    assert_eq!(again, 0);
}

#[tokio::test]
async fn a_bad_bearer_token_is_rejected() {
    let node_a = NodeKeypair::generate(NodeId(1));
    let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
    reg.publish_key(NodeId(1), &node_a.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::Server(7), NodeId(1)).await.unwrap();

    let store_a = Arc::new(MemoryStore::new().with_node(NodeId(1)));
    ServerStore::insert_server(
        &*store_a,
        Server::new(ServerId(7), "town", "Town", UserId(1), Timestamp(1)),
    );

    let feed_state = FeedState {
        store: store_a.clone(),
        keypair: Arc::new(node_a),
        registry: reg.clone(),
        resolver: Arc::new(StoreResolver(store_a.clone())),
        token: Some("right".into()),
    };
    let base_url = serve_a(feed_state).await;

    let client = FeedClient::new(base_url, Some("wrong".into()));
    let err = client.changes_since(0, 100).await.unwrap_err();
    assert!(err.contains("401"), "a wrong token is refused: {err}");
}
