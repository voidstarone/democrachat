//! End-to-end command forward over a real TCP socket: a node that does not own a
//! server forwards a signed `CastVote` to the owner, which authenticates it, checks
//! it owns the scope, guards against replay, and runs the use-case.

use std::sync::{Arc, Mutex};

use adapter_federation::{
    command_router, Command, CommandClient, CommandExecutor, CommandState, ForwardError,
    ReplayGuard, SignedCommand,
};
use async_trait::async_trait;
use domain::NodeId;
use federation::{InMemoryRegistry, NodeKeypair, OwnedScope, OwnershipRegistry, ScopeResolver};

struct FixedServer;
#[async_trait]
impl ScopeResolver for FixedServer {
    async fn proposal_server(&self, _: u64) -> Option<u64> {
        Some(7)
    }
    async fn message_server(&self, _: u64) -> Option<u64> {
        None
    }
}

#[derive(Default)]
struct RecordingExecutor(Mutex<Vec<Command>>);
#[async_trait]
impl CommandExecutor for RecordingExecutor {
    async fn execute(&self, command: &Command) -> Result<(), ForwardError> {
        self.0.lock().unwrap().push(command.clone());
        Ok(())
    }
}

async fn serve_owner(state: CommandState) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, command_router(state)).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn a_forwarded_vote_is_run_by_the_owner_and_cannot_replay() {
    // Owner = node 1, holds Server(7). Forwarder = node 9, keyed.
    let owner = NodeId(1);
    let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
    let forwarder = NodeKeypair::generate(NodeId(9));
    reg.publish_key(NodeId(9), &forwarder.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::Server(7), owner).await.unwrap();

    let executor = Arc::new(RecordingExecutor::default());
    let state = CommandState {
        node: owner,
        registry: reg.clone(),
        resolver: Arc::new(FixedServer),
        replay: Arc::new(ReplayGuard::in_memory()),
        executor: executor.clone(),
        token: Some("cluster-secret".into()),
    };
    let base = serve_owner(state).await;
    let client = CommandClient::new(base, Some("cluster-secret".into()));

    let cmd = Command::CastVote { proposal: 1, voter: 2, aye: true };
    let signed = SignedCommand::sign(&forwarder, &cmd);

    // The owner runs it.
    client.forward(&signed).await.expect("owner applies the forwarded vote");
    assert_eq!(executor.0.lock().unwrap().clone(), vec![cmd]);

    // Re-forwarding the exact same signed command is refused as a replay.
    let err = client.forward(&signed).await.unwrap_err();
    assert!(matches!(err, ForwardError::Rejected(_)), "replay refused: {err}");
    assert_eq!(executor.0.lock().unwrap().len(), 1, "applied exactly once");
}

#[tokio::test]
async fn a_forwarded_sealed_dm_is_run_by_the_senders_home() {
    // Owner = node 1, homes the sender UserHome(42). Forwarder = node 9, keyed.
    let owner = NodeId(1);
    let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
    let forwarder = NodeKeypair::generate(NodeId(9));
    reg.publish_key(NodeId(9), &forwarder.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::UserHome(42), owner).await.unwrap();

    let executor = Arc::new(RecordingExecutor::default());
    let state = CommandState {
        node: owner,
        registry: reg.clone(),
        resolver: Arc::new(FixedServer),
        replay: Arc::new(ReplayGuard::in_memory()),
        executor: executor.clone(),
        token: Some("cluster-secret".into()),
    };
    let base = serve_owner(state).await;
    let client = CommandClient::new(base, Some("cluster-secret".into()));

    // The sender (id 42) is homed on the owner; the DM carries opaque ciphertext.
    let cmd = Command::SendDm {
        from: 42,
        to: 7,
        sealed_for_recipient: "aa".into(),
        sealed_for_sender: "bb".into(),
    };
    let signed = SignedCommand::sign(&forwarder, &cmd);

    client.forward(&signed).await.expect("the sender's home applies the forwarded DM");
    assert_eq!(executor.0.lock().unwrap().clone(), vec![cmd]);

    // The same signed DM cannot be replayed against the owner.
    let err = client.forward(&signed).await.unwrap_err();
    assert!(matches!(err, ForwardError::Rejected(_)), "replay refused: {err}");
    assert_eq!(executor.0.lock().unwrap().len(), 1, "applied exactly once");
}

#[tokio::test]
async fn a_forwarded_block_is_run_by_a_node_owning_either_home() {
    // A block names two scopes — UserHome(blocker=100) and UserHome(blocked=200). This
    // owner homes only the blocked user (200); it must still run the forwarded block,
    // because it owns *one* of the command's scopes. (The caller forwards the same
    // block to the other home too; here we assert this owner commits its share.)
    let owner = NodeId(1);
    let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
    let forwarder = NodeKeypair::generate(NodeId(9));
    reg.publish_key(NodeId(9), &forwarder.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::UserHome(200), owner).await.unwrap();

    let executor = Arc::new(RecordingExecutor::default());
    let state = CommandState {
        node: owner,
        registry: reg.clone(),
        resolver: Arc::new(FixedServer),
        replay: Arc::new(ReplayGuard::in_memory()),
        executor: executor.clone(),
        token: Some("cluster-secret".into()),
    };
    let base = serve_owner(state).await;
    let client = CommandClient::new(base, Some("cluster-secret".into()));

    let cmd = Command::Block { blocker: 100, blocked: 200 };
    let signed = SignedCommand::sign(&forwarder, &cmd);

    client.forward(&signed).await.expect("the blocked user's home commits the block");
    assert_eq!(executor.0.lock().unwrap().clone(), vec![cmd]);
}

#[tokio::test]
async fn a_forwarded_block_is_refused_by_a_node_owning_neither_home() {
    // This owner homes neither user in the block, so it must refuse — the command was
    // misrouted here and applying it would silently drop one of the two required
    // commits without this node even holding a relevant scope.
    let owner = NodeId(1);
    let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
    let forwarder = NodeKeypair::generate(NodeId(9));
    reg.publish_key(NodeId(9), &forwarder.public().to_hex()).await.unwrap();
    // This node owns some unrelated home, not 100 or 200.
    reg.claim(OwnedScope::UserHome(555), owner).await.unwrap();

    let executor = Arc::new(RecordingExecutor::default());
    let state = CommandState {
        node: owner,
        registry: reg.clone(),
        resolver: Arc::new(FixedServer),
        replay: Arc::new(ReplayGuard::in_memory()),
        executor: executor.clone(),
        token: Some("cluster-secret".into()),
    };
    let base = serve_owner(state).await;
    let client = CommandClient::new(base, Some("cluster-secret".into()));

    let signed = SignedCommand::sign(&forwarder, &Command::Block { blocker: 100, blocked: 200 });
    assert!(client.forward(&signed).await.is_err(), "a node owning neither home refuses");
    assert!(executor.0.lock().unwrap().is_empty(), "never applied");
}

#[tokio::test]
async fn a_forwarded_friend_request_is_run_by_a_node_owning_either_home() {
    // Like a block, a friend request names both users' homes; a node owning either
    // one commits its share. This owner homes only the addressee (200).
    let owner = NodeId(1);
    let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
    let forwarder = NodeKeypair::generate(NodeId(9));
    reg.publish_key(NodeId(9), &forwarder.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::UserHome(200), owner).await.unwrap();

    let executor = Arc::new(RecordingExecutor::default());
    let state = CommandState {
        node: owner,
        registry: reg.clone(),
        resolver: Arc::new(FixedServer),
        replay: Arc::new(ReplayGuard::in_memory()),
        executor: executor.clone(),
        token: Some("cluster-secret".into()),
    };
    let base = serve_owner(state).await;
    let client = CommandClient::new(base, Some("cluster-secret".into()));

    let cmd = Command::RequestFriend { requester: 100, addressee: 200 };
    let signed = SignedCommand::sign(&forwarder, &cmd);
    client.forward(&signed).await.expect("the addressee's home commits the request");
    assert_eq!(executor.0.lock().unwrap().clone(), vec![cmd]);
}

#[tokio::test]
async fn a_forwarded_friend_accept_is_refused_by_a_node_owning_neither_home() {
    // A node homing neither party must refuse the acceptance — it holds no relevant
    // scope, so applying it would drop one of the two required commits.
    let owner = NodeId(1);
    let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
    let forwarder = NodeKeypair::generate(NodeId(9));
    reg.publish_key(NodeId(9), &forwarder.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::UserHome(555), owner).await.unwrap();

    let executor = Arc::new(RecordingExecutor::default());
    let state = CommandState {
        node: owner,
        registry: reg.clone(),
        resolver: Arc::new(FixedServer),
        replay: Arc::new(ReplayGuard::in_memory()),
        executor: executor.clone(),
        token: Some("cluster-secret".into()),
    };
    let base = serve_owner(state).await;
    let client = CommandClient::new(base, Some("cluster-secret".into()));

    let signed =
        SignedCommand::sign(&forwarder, &Command::AcceptFriend { accepter: 200, requester: 100 });
    assert!(client.forward(&signed).await.is_err(), "a node owning neither home refuses");
    assert!(executor.0.lock().unwrap().is_empty(), "never applied");
}

#[tokio::test]
async fn a_wrong_bearer_token_is_rejected() {
    let owner = NodeId(1);
    let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
    let forwarder = NodeKeypair::generate(NodeId(9));
    reg.publish_key(NodeId(9), &forwarder.public().to_hex()).await.unwrap();
    reg.claim(OwnedScope::Server(7), owner).await.unwrap();

    let state = CommandState {
        node: owner,
        registry: reg.clone(),
        resolver: Arc::new(FixedServer),
        replay: Arc::new(ReplayGuard::in_memory()),
        executor: Arc::new(RecordingExecutor::default()),
        token: Some("right".into()),
    };
    let base = serve_owner(state).await;
    let client = CommandClient::new(base, Some("wrong".into()));

    let signed = SignedCommand::sign(&forwarder, &Command::CastVote { proposal: 1, voter: 2, aye: true });
    // A 401 has no body, so the client surfaces it as a rejection.
    assert!(client.forward(&signed).await.is_err());
}
