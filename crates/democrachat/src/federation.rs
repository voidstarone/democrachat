//! Composition-root wiring for federation (M5).
//!
//! When this node is configured for federation, bring up its signed change-feed
//! server and its peer puller, and register the node's ownership + public key with
//! the shared control plane (etcd). Everything here is **guarded**: with no
//! federation env set (the default single-box deployment) [`start`] returns
//! immediately and the node behaves exactly as before.
//!
//! Live multi-node verification needs a running etcd and ≥2 app instances (the
//! same Docker harness M4 used); this module is the wiring those instances run.

use std::sync::Arc;
use std::time::Duration;

use std::collections::HashMap;

use adapter_control_etcd::EtcdRegistry;
use adapter_federation::{
    spawn_puller, CommandClient, CommandExecutor, CommandState, FeedClient, FeedState, Peer,
    ReplayGuard, Replicator, StoreResolver, WriteRouter,
};
use app::{ServerStore, Services, UserStore};
use adapter_store_memory::MemoryStore;
use domain::{origin_node, NodeId};
use federation::{
    choose_new_standby, NodeLoad, NodeStatus, OwnedScope, OwnershipRegistry, RehomeOutcome,
    RehomingController,
};
use federation::NodeKeypair;

use crate::block_router::FederatedBlockRouter;
use crate::command_executor::ServiceCommandExecutor;
use crate::dm_router::FederatedDmRouter;
use crate::friend_router::FederatedFriendRouter;
use crate::nonce_log::StoreNonceLog;
use crate::vote_router::FederatedVoteRouter;

/// The write routers the web layer installs, all backed by the one `WriteRouter`.
/// Empty (`Default`) on the single-box deployment, where every write applies locally.
#[derive(Default)]
pub struct Routers {
    pub vote: Option<Arc<dyn app::VoteRouter>>,
    pub dm: Option<Arc<dyn app::DmRouter>>,
    pub block: Option<Arc<dyn app::BlockRouter>>,
    pub friend: Option<Arc<dyn app::FriendRouter>>,
}

/// A non-empty, non-whitespace environment value, or `None`.
fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Federation deployment config, read from the environment. Federation stays
/// **off** unless the node has a real id (>0), a persistent signing seed, an etcd
/// endpoint, and a feed bind — the four things a node cannot federate without.
struct FedConfig {
    node: NodeId,
    seed_hex: String,
    etcd: Vec<String>,
    feed_addr: String,
    token: Option<String>,
    peers: Vec<(NodeId, String)>,
    poll: Duration,
}

impl FedConfig {
    fn from_env(node: NodeId) -> Option<Self> {
        if node == NodeId(0) {
            return None; // the single-box identity has nothing to federate
        }
        let etcd: Vec<String> = env("DEMOCRACHAT_ETCD_ENDPOINTS")?
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if etcd.is_empty() {
            return None;
        }
        let poll = env("DEMOCRACHAT_FED_POLL_SECS")
            .and_then(|s| s.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(5));
        Some(Self {
            node,
            seed_hex: env("DEMOCRACHAT_NODE_SEED")?,
            etcd,
            feed_addr: env("DEMOCRACHAT_FED_ADDR")?,
            token: env("DEMOCRACHAT_FED_TOKEN"),
            peers: parse_peers(env("DEMOCRACHAT_PEERS").as_deref().unwrap_or("")),
            poll,
        })
    }
}

/// Parse a peer list of `node=url` pairs, e.g.
/// `1=http://10.0.0.1:4000,3=http://10.0.0.3:4000`. Malformed entries are skipped.
fn parse_peers(raw: &str) -> Vec<(NodeId, String)> {
    raw.split(',')
        .filter_map(|entry| {
            let (n, url) = entry.split_once('=')?;
            let id: u16 = n.trim().parse().ok()?;
            Some((NodeId(id), url.trim().to_string()))
        })
        .collect()
}

/// A fatal federation-startup error. A node told to federate that cannot reach its
/// control plane must **not** silently fall back to single-box (that would risk
/// serving a stale, un-replicated view under a federated id) — it exits.
fn fatal(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: federation startup failed: {msg}");
    std::process::exit(2);
}

/// Bring federation up for `store` on `node`, if configured. Spawns the feed +
/// command server and the puller onto the current tokio runtime and returns; the
/// caller's web server keeps running alongside them. `services`/`save` back the
/// command executor that runs forwarded writes (e.g. a federated citizen's vote).
///
/// Returns the [`VoteRouter`](app::VoteRouter) the web layer installs so a vote
/// reaches the proposal's server owner; `None` on the single-box deployment, where
/// votes apply locally.
pub async fn start(
    store: Arc<MemoryStore>,
    services: Arc<Services>,
    save: Arc<dyn Fn() + Send + Sync>,
    node: NodeId,
) -> Routers {
    let Some(cfg) = FedConfig::from_env(node) else {
        return Routers::default(); // not configured — single-box, unchanged
    };

    let keypair = Arc::new(
        NodeKeypair::from_seed_hex(cfg.node, &cfg.seed_hex)
            .unwrap_or_else(|e| fatal(format!("DEMOCRACHAT_NODE_SEED invalid: {e}"))),
    );
    let registry: Arc<dyn OwnershipRegistry> = Arc::new(
        EtcdRegistry::connect(&cfg.etcd, 15, cfg.node)
            .await
            .unwrap_or_else(|e| fatal(format!("cannot reach etcd control plane: {e}"))),
    );

    // Publish this node's key so peers can verify the feed it signs.
    if let Err(e) = registry.publish_key(cfg.node, &keypair.public().to_hex()).await {
        fatal(format!("could not publish node key: {e}"));
    }

    // Claim every scope this node homes — now, and then continuously. The node that
    // minted an id homes it, but ids are minted at *runtime* (a user registers, a
    // server is founded) long after boot, so a one-shot claim at startup would leave
    // every later entity unowned — its home writes unroutable and even its feed rows
    // unauthorized (a peer checks `owner_of(derived_scope) == signer`). The
    // reconciler runs the claim once before serving, then on a timer to pick up new
    // entities. `claim` is idempotent — an already-held scope returns `Held` without
    // bumping the epoch, so re-running is free and never churns the fencing token.
    reconcile_claims(store.as_ref(), registry.as_ref(), cfg.node).await;
    {
        let store = store.clone();
        let registry = registry.clone();
        let node = cfg.node;
        // A brisk cadence so a freshly minted user/server becomes routable quickly;
        // independent of (and faster than) the feed poll.
        let period = Duration::from_secs(3);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            tick.tick().await; // consume the immediate first tick (already reconciled)
            loop {
                tick.tick().await;
                reconcile_claims(store.as_ref(), registry.as_ref(), node).await;
            }
        });
    }

    // Auto-failover: watch the scopes homed on OTHER nodes that we replicate, and when
    // a home node genuinely goes DOWN, promote ourselves for its scopes if we are the
    // best live standby. The standby claim bumps the epoch, fencing the old owner so
    // it can't resurrect stale ownership on return. A scope whose community disabled
    // rehoming is deliberately left down.
    //
    // Crucially we only consider a scope whose **home node is not live**. A scope is
    // momentarily unowned every time its home mints it (a fresh user/server is
    // unowned until the home's next reconcile tick claims it); without this guard the
    // rehoming loop would race the home and *steal* its live, newly-created scopes.
    // "Home node absent from `live_nodes`" is the real, unambiguous down signal.
    {
        let store = store.clone();
        let registry = registry.clone();
        let controller = RehomingController::new(cfg.node, registry.clone());
        let node = cfg.node;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(4));
            loop {
                tick.tick().await;
                let live: std::collections::HashSet<u16> = registry
                    .live_nodes()
                    .await
                    .map(|v| v.into_iter().map(|s| s.node.0).collect())
                    .unwrap_or_default();
                let candidates: Vec<OwnedScope> = foreign_scopes(store.as_ref(), node)
                    .into_iter()
                    .filter(|scope| !live.contains(&scope_home(*scope)))
                    .collect();
                if candidates.is_empty() {
                    continue;
                }
                for outcome in controller.tick(&candidates).await {
                    if let RehomeOutcome::Promoted { scope, epoch } = outcome {
                        eprintln!("rehome: promoted {scope:?} to this node at epoch {epoch}");
                    }
                }
            }
        });
    }

    // The command executor is shared by the command endpoint (forwarded writes land
    // here) and the local branch of the write router (writes we own run here too),
    // so a local and a forwarded vote take exactly one code path.
    let executor: Arc<dyn CommandExecutor> =
        Arc::new(ServiceCommandExecutor::new(services, save.clone()));

    // A durable, store-backed replay guard: a remembered nonce is persisted, so a
    // captured command can't be replayed against this owner after a restart.
    let replay = Arc::new(ReplayGuard::new(Arc::new(StoreNonceLog::new(store.clone(), save))));

    // The consumer side: a replicator over the local store, pulling from peers.
    let replicator = Arc::new(Replicator::new(
        store.clone(),
        registry.clone(),
        Arc::new(StoreResolver(store.clone())),
    ));
    let feed_peers: Vec<Peer> = cfg
        .peers
        .iter()
        .map(|(node, url)| Peer {
            node: *node,
            client: FeedClient::new(url.clone(), cfg.token.clone()),
        })
        .collect();
    spawn_puller(replicator, feed_peers, cfg.poll, 500);

    // The caller side: route a local write to its scope's owner (apply here or
    // forward). Peers here are command endpoints, keyed by owner node id.
    let command_peers: HashMap<NodeId, CommandClient> = cfg
        .peers
        .iter()
        .map(|(node, url)| (*node, CommandClient::new(url.clone(), cfg.token.clone())))
        .collect();
    let write_router = Arc::new(WriteRouter::new(
        cfg.node,
        registry.clone(),
        Arc::new(StoreResolver(store.clone())),
        keypair.clone(),
        executor.clone(),
        command_peers,
    ));

    // The producer side: serve this node's signed feed *and* accept forwarded
    // commands (writes to scopes this node owns) on the one node-only address.
    let feed_state = FeedState {
        store: store.clone(),
        keypair,
        registry: registry.clone(),
        resolver: Arc::new(StoreResolver(store.clone())),
        token: cfg.token.clone(),
    };
    let command_state = CommandState {
        node: cfg.node,
        registry,
        resolver: Arc::new(StoreResolver(store)),
        replay,
        executor,
        token: cfg.token,
    };
    let feed_addr = cfg.feed_addr.clone();
    tokio::spawn(async move {
        if let Err(e) =
            adapter_federation::serve_federation(feed_state, command_state, &feed_addr).await
        {
            eprintln!("error: federation server exited: {e}");
        }
    });

    eprintln!("federation: node {} up — feed + command on {}", cfg.node.0, cfg.feed_addr);
    Routers {
        vote: Some(Arc::new(FederatedVoteRouter(write_router.clone()))),
        dm: Some(Arc::new(FederatedDmRouter(write_router.clone()))),
        block: Some(Arc::new(FederatedBlockRouter(write_router.clone()))),
        friend: Some(Arc::new(FederatedFriendRouter(write_router))),
    }
}

/// The scopes this node homes (the ones it minted the ids for): each minted server
/// and each minted user.
fn owned_scopes(store: &MemoryStore, node: NodeId) -> Vec<OwnedScope> {
    let mut out: Vec<OwnedScope> = ServerStore::list_all(store)
        .into_iter()
        .filter(|s| origin_node(s.id.0) == node)
        .map(|s| OwnedScope::Server(s.id.0))
        .collect();
    out.extend(
        UserStore::list_all(store)
            .into_iter()
            .filter(|u| origin_node(u.id.0) == node)
            .map(|u| OwnedScope::UserHome(u.id.0)),
    );
    out
}

/// The node that minted (and by default homes) a scope's id.
fn scope_home(scope: OwnedScope) -> u16 {
    let id = match scope {
        OwnedScope::Server(id) => id,
        OwnedScope::UserHome(id) => id,
    };
    origin_node(id).0
}

/// The scopes homed on OTHER nodes that this node has replicated — the failover
/// candidates it may have to take over if a peer goes down.
fn foreign_scopes(store: &MemoryStore, node: NodeId) -> Vec<OwnedScope> {
    let mut out: Vec<OwnedScope> = ServerStore::list_all(store)
        .into_iter()
        .filter(|s| origin_node(s.id.0) != node)
        .map(|s| OwnedScope::Server(s.id.0))
        .collect();
    out.extend(
        UserStore::list_all(store)
            .into_iter()
            .filter(|u| origin_node(u.id.0) != node)
            .map(|u| OwnedScope::UserHome(u.id.0)),
    );
    out
}

/// Claim, in the control plane, every scope this node homes: the `Server` scope of
/// each server it minted (mirroring the governance rehoming policy) and the
/// `UserHome` scope of each user it minted. Called once before serving and then on a
/// timer, so entities created after boot become owned — and thus routable and
/// feed-authorized — without a restart. Idempotent: a scope we already hold returns
/// `Held` and is left untouched.
///
/// Each tick also (a) reports this node's load, which is its **liveness heartbeat** —
/// its presence under `nodes/` (leased) is how peers know it is up and eligible to be
/// promoted, and (b) designates a live peer as the **standby** for each owned scope,
/// so failover has a known heir. Both underpin the rehoming loop above.
async fn reconcile_claims(
    store: &MemoryStore,
    registry: &dyn OwnershipRegistry,
    node: NodeId,
) {
    let owned = owned_scopes(store, node);

    // Liveness heartbeat + load hint (hosted-scope count breaks failover ties).
    let _ = registry
        .report_load(node, NodeLoad { hosted_scopes: owned.len() as u32, requests_per_sec: 0.0 })
        .await;
    // The quietest live peer that isn't us — the heir we designate for our scopes.
    let standby = registry
        .live_nodes()
        .await
        .ok()
        .and_then(|live: Vec<NodeStatus>| choose_new_standby(&[node], &live));

    for scope in owned {
        if let Err(e) = registry.claim(scope, node).await {
            eprintln!("warning: could not claim {scope:?}: {e}");
            continue;
        }
        // Pin a server that its citizens voted to disable rehoming for.
        if let OwnedScope::Server(id) = scope {
            let disabled = ServerStore::list_all(store)
                .into_iter()
                .find(|s| s.id.0 == id)
                .is_some_and(|s| s.is_rehoming_disabled);
            if disabled {
                if let Err(e) = registry.set_rehoming(scope, false).await {
                    eprintln!("warning: could not sync rehoming policy for {scope:?}: {e}");
                }
            }
        }
        // Designate the heir (owner-guarded: only works because we just claimed it).
        if let Some(sb) = standby {
            let _ = registry.set_standby(scope, sb).await;
        }
    }
}
