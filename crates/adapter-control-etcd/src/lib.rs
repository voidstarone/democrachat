//! etcd-backed [`OwnershipRegistry`]: leases, epoch fencing, key/load/standby
//! registry, and the rehoming opt-out.
//!
//! Key layout (all under the `democrachat/` prefix):
//! ```text
//! democrachat/keys/<node>                → node public key hex     (persistent)
//! democrachat/nodes/<node>               → NodeLoad JSON           (lease → liveness)
//! democrachat/owners/<scope>/holder      → owner node id           (lease → ownership)
//! democrachat/owners/<scope>/epoch       → monotonic fencing token (persistent)
//! democrachat/owners/<scope>/standbys    → [node id, …] JSON        (persistent)
//! democrachat/rehoming/<scope>           → "off" iff disabled       (persistent)
//! ```
//! `<scope>` is `s/<id>` for a server and `u/<id>` for a user-home, so the two
//! ownable axes never collide (see [`federation::OwnedScope`]).
//!
//! **Ownership is a lease.** The holder key is written with this node's lease, so
//! if the node stops heart-beating, etcd expires the key within the TTL and the
//! scope becomes claimable — that is failover.
//!
//! **Claiming is a compare-and-swap.** Two nodes racing to claim a freed scope run
//! an optimistic loop: read the epoch and its `mod_revision`, then a txn guarded on
//! (holder still absent) AND (epoch unchanged) that writes the leased holder and
//! the bumped epoch. Exactly one txn's guard holds, so the epoch is a strict
//! fencing token — a returning old owner's stale epoch is rejected fleet-wide.

use std::time::Duration;

use async_trait::async_trait;
use etcd_client::{
    Certificate, Client, Compare, CompareOp, ConnectOptions, GetOptions, Identity, PutOptions,
    TlsOptions, Txn, TxnOp,
};
use serde::{Deserialize, Serialize};

use domain::NodeId;
use federation::{
    ClaimOutcome, NodeLoad, NodeStatus, NodePublicKey, OwnedScope, Ownership, OwnershipRegistry,
    RegistryError,
};

const PREFIX: &str = "democrachat";

fn err(e: impl std::fmt::Display) -> RegistryError {
    RegistryError(e.to_string())
}

/// The key fragment identifying an ownable scope. The two axes get distinct
/// prefixes (`s/` vs `u/`) so a server and a user-home with the same numeric id
/// map to different etcd keys.
fn scope_key(scope: OwnedScope) -> String {
    match scope {
        OwnedScope::Server(id) => format!("s/{id}"),
        OwnedScope::UserHome(id) => format!("u/{id}"),
    }
}

fn key_of(node: NodeId) -> String {
    format!("{PREFIX}/keys/{}", node.0)
}
fn node_of(node: NodeId) -> String {
    format!("{PREFIX}/nodes/{}", node.0)
}
fn holder_of(scope: OwnedScope) -> String {
    format!("{PREFIX}/owners/{}/holder", scope_key(scope))
}
fn epoch_of(scope: OwnedScope) -> String {
    format!("{PREFIX}/owners/{}/epoch", scope_key(scope))
}
fn standbys_of(scope: OwnedScope) -> String {
    format!("{PREFIX}/owners/{}/standbys", scope_key(scope))
}
fn rehoming_of(scope: OwnedScope) -> String {
    format!("{PREFIX}/rehoming/{}", scope_key(scope))
}

/// `NodeLoad` is a pure domain-adjacent type with no serde derive; this is its
/// wire form for the control plane.
#[derive(Serialize, Deserialize)]
struct LoadWire {
    hosted_scopes: u32,
    requests_per_sec: f64,
}
impl From<NodeLoad> for LoadWire {
    fn from(l: NodeLoad) -> Self {
        Self {
            hosted_scopes: l.hosted_scopes,
            requests_per_sec: l.requests_per_sec,
        }
    }
}
impl From<LoadWire> for NodeLoad {
    fn from(w: LoadWire) -> Self {
        NodeLoad {
            hosted_scopes: w.hosted_scopes,
            requests_per_sec: w.requests_per_sec,
        }
    }
}

/// Optional TLS (and mutual TLS) for the control-plane link, from env. The control
/// plane is the ownership trust anchor: without TLS, anyone who can reach etcd can
/// forge holder/epoch/key writes and seize any scope. Unset ⇒ plaintext (a
/// trusted LAN / test etcd). `DEMOCRACHAT_ETCD_CA` = CA pem, `_CERT`/`_KEY` = this
/// node's client identity for mTLS, `_DOMAIN` = the cert's SNI name.
fn etcd_tls_options() -> Result<Option<ConnectOptions>, RegistryError> {
    let ca = std::env::var("DEMOCRACHAT_ETCD_CA").ok();
    let cert = std::env::var("DEMOCRACHAT_ETCD_CERT").ok();
    let key = std::env::var("DEMOCRACHAT_ETCD_KEY").ok();
    if ca.is_none() && cert.is_none() && key.is_none() {
        return Ok(None);
    }
    let mut tls = TlsOptions::new();
    if let Some(ca) = ca {
        tls = tls.ca_certificate(Certificate::from_pem(std::fs::read(ca).map_err(err)?));
    }
    if let (Some(cert), Some(key)) = (cert, key) {
        let cert_pem = std::fs::read(cert).map_err(err)?;
        let key_pem = std::fs::read(key).map_err(err)?;
        tls = tls.identity(Identity::from_pem(cert_pem, key_pem));
    }
    if let Ok(domain) = std::env::var("DEMOCRACHAT_ETCD_DOMAIN") {
        tls = tls.domain_name(domain);
    }
    Ok(Some(ConnectOptions::new().with_tls(tls)))
}

/// An etcd-backed control plane bound to one node's lease.
pub struct EtcdRegistry {
    client: Client,
    /// This node's identity — used to authorize writes only the current owner may
    /// make (designating standbys).
    node: NodeId,
    /// This node's lease; holder/liveness keys are written under it, and a
    /// background task keeps it alive.
    lease_id: i64,
    lease_ttl: i64,
}

impl EtcdRegistry {
    /// Connect to an etcd cluster as `node`, granting a lease with `ttl_secs`. A
    /// background task renews it every `ttl_secs/3`; if this process dies, the lease
    /// lapses within the TTL and everything it owns becomes claimable.
    pub async fn connect(
        endpoints: &[String],
        ttl_secs: i64,
        node: NodeId,
    ) -> Result<Self, RegistryError> {
        let opts = etcd_tls_options()?;
        let mut client = Client::connect(endpoints, opts).await.map_err(err)?;
        let lease = client.lease_grant(ttl_secs, None).await.map_err(err)?;
        let lease_id = lease.id();

        // Keep the lease alive for the life of the process. A transient etcd blip
        // must NOT permanently stop renewal (that would silently expire the lease and
        // hand this node's scopes to a standby while the node is actually fine). Three
        // properties make a partition shorter than the TTL survivable:
        //   * healthy cadence — renew every TTL/3, giving three attempts per TTL;
        //   * bounded round-trips — a frozen etcd (e.g. `docker pause`) must not wedge
        //     the task in `message().await` past the TTL, so each renew is time-boxed;
        //   * fast recovery — after any failure we reconnect on a short backoff, not a
        //     full period, so the *moment* etcd returns we renew within the lease's
        //     remaining budget instead of sleeping through it.
        // Only a real process death stops renewal — the intended failover trigger.
        let period = Duration::from_secs((ttl_secs / 3).max(1) as u64);
        let backoff = Duration::from_millis(500);
        {
            let mut ka_client = client.clone();
            tokio::spawn(async move {
                let mut chan = None;
                loop {
                    // Ensure a live keep-alive channel, retrying quickly on failure so
                    // recovery after a blip is ~backoff, not a whole period.
                    if chan.is_none() {
                        match ka_client.lease_keep_alive(lease_id).await {
                            Ok(c) => chan = Some(c),
                            Err(_) => {
                                tokio::time::sleep(backoff).await;
                                continue;
                            }
                        }
                    }
                    let (keeper, stream) = chan.as_mut().expect("channel just set");
                    // Time-box the round-trip: a paused/partitioned etcd can otherwise
                    // block here for the whole outage, past the point the lease lapses.
                    let renewed = matches!(
                        tokio::time::timeout(period, async {
                            keeper.keep_alive().await.is_ok() && stream.message().await.is_ok()
                        })
                        .await,
                        Ok(true)
                    );
                    if !renewed {
                        chan = None; // drop the dead channel; reconnect on the backoff
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    tokio::time::sleep(period).await; // healthy — renew each third of the TTL
                }
            });
        }

        Ok(Self {
            client,
            node,
            lease_id,
            lease_ttl: ttl_secs,
        })
    }

    /// The lease TTL this node registered with (seconds).
    pub fn lease_ttl(&self) -> i64 {
        self.lease_ttl
    }

    async fn get_str(&self, key: &str) -> Result<Option<(String, i64)>, RegistryError> {
        let mut client = self.client.clone();
        let resp = client.get(key, None).await.map_err(err)?;
        match resp.kvs().first() {
            None => Ok(None),
            Some(kv) => Ok(Some((kv.value_str().map_err(err)?.to_string(), kv.mod_revision()))),
        }
    }
}

#[async_trait]
impl OwnershipRegistry for EtcdRegistry {
    async fn owner_of(&self, scope: OwnedScope) -> Result<Option<Ownership>, RegistryError> {
        // A holder key exists only while the owner's lease is alive.
        let Some((holder, _)) = self.get_str(&holder_of(scope)).await? else {
            return Ok(None);
        };
        let owner: u16 = holder.parse().map_err(err)?;
        let epoch = match self.get_str(&epoch_of(scope)).await? {
            Some((e, _)) => e.parse().map_err(err)?,
            None => 0,
        };
        Ok(Some(Ownership {
            scope,
            owner: NodeId(owner),
            epoch,
        }))
    }

    async fn claim(&self, scope: OwnedScope, node: NodeId) -> Result<ClaimOutcome, RegistryError> {
        let holder_key = holder_of(scope);
        let epoch_key = epoch_of(scope);
        let mut client = self.client.clone();

        // Optimistic CAS loop, bounded so a pathological livelock can't spin forever.
        for _ in 0..8 {
            if let Some((holder, _)) = self.get_str(&holder_key).await? {
                let by: u16 = holder.parse().map_err(err)?;
                let epoch = self
                    .get_str(&epoch_key)
                    .await?
                    .map(|(e, _)| e.parse().unwrap_or(0))
                    .unwrap_or(0);
                return Ok(ClaimOutcome::Held { by: NodeId(by), epoch });
            }

            let (cur_epoch, epoch_rev) = match self.get_str(&epoch_key).await? {
                Some((e, rev)) => (e.parse::<u64>().map_err(err)?, rev),
                None => (0, 0), // absent → guard on create_revision 0
            };
            let next_epoch = cur_epoch + 1;

            let txn = Txn::new()
                .when(vec![
                    // Holder still absent (nobody claimed since we looked)…
                    Compare::create_revision(holder_key.as_bytes(), CompareOp::Equal, 0),
                    // …and the epoch is unchanged since we read it.
                    Compare::mod_revision(epoch_key.as_bytes(), CompareOp::Equal, epoch_rev),
                ])
                .and_then(vec![
                    TxnOp::put(
                        holder_key.as_bytes(),
                        node.0.to_string(),
                        Some(PutOptions::new().with_lease(self.lease_id)),
                    ),
                    TxnOp::put(epoch_key.as_bytes(), next_epoch.to_string(), None),
                ]);

            if client.txn(txn).await.map_err(err)?.succeeded() {
                return Ok(ClaimOutcome::Claimed { epoch: next_epoch });
            }
            // Lost the race; re-read and retry.
        }
        Err(RegistryError("claim contended out after retries".into()))
    }

    async fn release(&self, scope: OwnedScope, node: NodeId) -> Result<(), RegistryError> {
        let holder_key = holder_of(scope);
        let mut client = self.client.clone();
        // Only delete if we still hold it (guard on the value being our node id).
        let txn = Txn::new()
            .when(vec![Compare::value(
                holder_key.as_bytes(),
                CompareOp::Equal,
                node.0.to_string(),
            )])
            .and_then(vec![TxnOp::delete(holder_key.as_bytes(), None)]);
        client.txn(txn).await.map_err(err)?;
        Ok(())
    }

    async fn set_standby(&self, scope: OwnedScope, node: NodeId) -> Result<(), RegistryError> {
        // Only the scope's CURRENT OWNER may designate its standbys: a standby is a
        // failover heir, so letting any node append *itself* would let a hostile
        // node make itself the heir and seize the scope on the owner's next
        // downtime. The put is guarded on "I am the current holder"; a non-owner's
        // write does not apply. (The in-memory registry has no hostile peers and
        // omits this guard.)
        let holder_key = holder_of(scope);
        let key = standbys_of(scope);
        let mut list: Vec<u16> = match self.get_str(&key).await? {
            Some((json, _)) => serde_json::from_str(&json).map_err(err)?,
            None => Vec::new(),
        };
        if list.contains(&node.0) {
            return Ok(());
        }
        list.push(node.0);
        let json = serde_json::to_string(&list).map_err(err)?;
        let mut client = self.client.clone();
        let txn = Txn::new()
            .when(vec![Compare::value(
                holder_key.as_bytes(),
                CompareOp::Equal,
                self.node.0.to_string(),
            )])
            .and_then(vec![TxnOp::put(key.as_bytes(), json, None)]);
        if !client.txn(txn).await.map_err(err)?.succeeded() {
            return Err(RegistryError(
                "refusing to set a standby: this node is not the scope's current owner".into(),
            ));
        }
        Ok(())
    }

    async fn standbys(&self, scope: OwnedScope) -> Result<Vec<NodeId>, RegistryError> {
        match self.get_str(&standbys_of(scope)).await? {
            None => Ok(Vec::new()),
            Some((json, _)) => {
                let list: Vec<u16> = serde_json::from_str(&json).map_err(err)?;
                Ok(list.into_iter().map(NodeId).collect())
            }
        }
    }

    async fn can_rehome(&self, scope: OwnedScope) -> Result<bool, RegistryError> {
        // Persistent policy: the opt-out survives owner death (it is the community's
        // standing choice). Present key ⇒ disabled.
        Ok(self.get_str(&rehoming_of(scope)).await?.is_none())
    }

    async fn set_rehoming(&self, scope: OwnedScope, enabled: bool) -> Result<(), RegistryError> {
        let key = rehoming_of(scope);
        let mut client = self.client.clone();
        if enabled {
            client.delete(key, None).await.map_err(err)?;
        } else {
            client.put(key, "off", None).await.map_err(err)?;
        }
        Ok(())
    }

    async fn renew(&self, _node: NodeId) -> Result<(), RegistryError> {
        Ok(()) // the background keep-alive task renews the lease
    }

    async fn publish_key(&self, node: NodeId, public_hex: &str) -> Result<(), RegistryError> {
        // Persistent (no lease): peers must verify a node's past events even while
        // it is down. First-write-wins — the signing key is the identity anchor
        // every authorization trusts, so refuse to overwrite it with a different
        // key; re-publishing the same key (on restart) is idempotent.
        let key = key_of(node);
        if let Some((existing, _)) = self.get_str(&key).await? {
            return if existing == public_hex {
                Ok(())
            } else {
                Err(RegistryError(
                    "node key already published; refusing to overwrite it (first-write-wins)".into(),
                ))
            };
        }
        // Absent → create only if still absent, so a racing writer can't slip in.
        let mut client = self.client.clone();
        let txn = Txn::new()
            .when(vec![Compare::create_revision(key.as_bytes(), CompareOp::Equal, 0)])
            .and_then(vec![TxnOp::put(key.as_bytes(), public_hex, None)]);
        if !client.txn(txn).await.map_err(err)?.succeeded() {
            return Err(RegistryError(
                "node key was published concurrently; refusing to overwrite it".into(),
            ));
        }
        Ok(())
    }

    async fn public_key(&self, node: NodeId) -> Result<Option<NodePublicKey>, RegistryError> {
        match self.get_str(&key_of(node)).await? {
            None => Ok(None),
            Some((hex, _)) => NodePublicKey::from_hex(node, &hex).map(Some).map_err(err),
        }
    }

    async fn report_load(&self, node: NodeId, load: NodeLoad) -> Result<(), RegistryError> {
        // Leased: presence under nodes/ is this node's liveness signal.
        let json = serde_json::to_string(&LoadWire::from(load)).map_err(err)?;
        let mut client = self.client.clone();
        client
            .put(node_of(node), json, Some(PutOptions::new().with_lease(self.lease_id)))
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn live_nodes(&self) -> Result<Vec<NodeStatus>, RegistryError> {
        let mut client = self.client.clone();
        let resp = client
            .get(format!("{PREFIX}/nodes/"), Some(GetOptions::new().with_prefix()))
            .await
            .map_err(err)?;
        let mut out = Vec::new();
        for kv in resp.kvs() {
            let key = kv.key_str().map_err(err)?;
            let Some(id) = key.rsplit('/').next().and_then(|s| s.parse::<u16>().ok()) else {
                continue;
            };
            let load: LoadWire = serde_json::from_slice(kv.value()).map_err(err)?;
            out.push(NodeStatus {
                node: NodeId(id),
                load: load.into(),
            });
        }
        Ok(out)
    }
}
