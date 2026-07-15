//! Integration tests against a REAL etcd. Gated on `TEST_ETCD_ENDPOINT` — with it
//! unset the test returns immediately, so `cargo test` stays green without etcd.
//!
//!   docker run -d --rm -p 2379:2379 --name etcd \
//!     gcr.io/etcd-development/etcd:v3.5.16 \
//!     /usr/local/bin/etcd --advertise-client-urls http://0.0.0.0:2379 \
//!                         --listen-client-urls http://0.0.0.0:2379
//!   TEST_ETCD_ENDPOINT=http://127.0.0.1:2379 cargo test -p adapter-control-etcd

use adapter_control_etcd::EtcdRegistry;
use domain::NodeId;
use etcd_client::{Client, DeleteOptions};
use federation::{ClaimOutcome, NodeKeypair, NodeLoad, OwnedScope, OwnershipRegistry};

async fn wipe(ep: &str) {
    let mut c = Client::connect([ep], None).await.unwrap();
    c.delete("democrachat/", Some(DeleteOptions::new().with_prefix()))
        .await
        .unwrap();
}

#[tokio::test]
async fn etcd_control_plane_end_to_end() {
    let Ok(ep) = std::env::var("TEST_ETCD_ENDPOINT") else {
        eprintln!("skipping: set TEST_ETCD_ENDPOINT (e.g. http://127.0.0.1:2379) to run");
        return;
    };
    wipe(&ep).await; // fresh state so the run is repeatable

    let eps = vec![ep.clone()];
    let n1 = EtcdRegistry::connect(&eps, 10, NodeId(1)).await.unwrap();
    let n2 = EtcdRegistry::connect(&eps, 10, NodeId(2)).await.unwrap();
    let s = OwnedScope::Server(7);

    // --- claim + epoch fencing ------------------------------------------------
    let ClaimOutcome::Claimed { epoch: e1 } = n1.claim(s, NodeId(1)).await.unwrap() else {
        panic!("first claim should succeed");
    };
    assert_eq!(e1, 1, "first claim mints epoch 1");
    assert_eq!(n1.owner_of(s).await.unwrap().unwrap().owner, NodeId(1));

    // A live owner holds it — a competing claim does not take ownership.
    assert_eq!(
        n2.claim(s, NodeId(2)).await.unwrap(),
        ClaimOutcome::Held { by: NodeId(1), epoch: 1 }
    );

    // Owner releases; the scope becomes claimable and the re-claim bumps the epoch.
    n1.release(s, NodeId(1)).await.unwrap();
    assert!(n1.owner_of(s).await.unwrap().is_none());
    let ClaimOutcome::Claimed { epoch: e2 } = n2.claim(s, NodeId(2)).await.unwrap() else {
        panic!("re-claim of a freed scope should succeed");
    };
    assert!(e2 > e1, "the epoch is a strict fencing token ({e2} > {e1})");
    // The returning old owner is fenced — it cannot reclaim a live scope.
    assert_eq!(
        n1.claim(s, NodeId(1)).await.unwrap(),
        ClaimOutcome::Held { by: NodeId(2), epoch: e2 }
    );

    // --- keys -----------------------------------------------------------------
    let kp2 = NodeKeypair::generate(NodeId(2));
    n2.publish_key(NodeId(2), &kp2.public().to_hex()).await.unwrap();
    n2.publish_key(NodeId(2), &kp2.public().to_hex()).await.unwrap(); // idempotent
    let other = NodeKeypair::generate(NodeId(2));
    assert!(
        n2.publish_key(NodeId(2), &other.public().to_hex()).await.is_err(),
        "a different key for a node is refused (first-write-wins)"
    );
    assert_eq!(
        n1.public_key(NodeId(2)).await.unwrap().unwrap().to_hex(),
        kp2.public().to_hex()
    );

    // --- load + liveness ------------------------------------------------------
    n1.report_load(NodeId(1), NodeLoad { hosted_scopes: 3, requests_per_sec: 4.0 })
        .await
        .unwrap();
    n2.report_load(NodeId(2), NodeLoad { hosted_scopes: 1, requests_per_sec: 2.0 })
        .await
        .unwrap();
    let mut live: Vec<u16> = n1.live_nodes().await.unwrap().into_iter().map(|s| s.node.0).collect();
    live.sort_unstable();
    assert_eq!(live, vec![1, 2]);

    // --- standbys (owner-guarded) --------------------------------------------
    // node2 owns s, so it may designate a standby; node1 (non-owner) may not.
    n2.set_standby(s, NodeId(3)).await.unwrap();
    assert_eq!(n2.standbys(s).await.unwrap(), vec![NodeId(3)]);
    assert!(
        n1.set_standby(s, NodeId(9)).await.is_err(),
        "a non-owner cannot inject itself as a failover heir"
    );

    // --- rehoming opt-out -----------------------------------------------------
    assert!(n1.can_rehome(s).await.unwrap(), "rehoming on by default");
    n2.set_rehoming(s, false).await.unwrap(); // citizens opt out
    assert!(!n1.can_rehome(s).await.unwrap());
    // Per typed scope: a same-numbered user-home is unaffected.
    assert!(n1.can_rehome(OwnedScope::UserHome(7)).await.unwrap());
    n2.set_rehoming(s, true).await.unwrap();
    assert!(n1.can_rehome(s).await.unwrap());

    wipe(&ep).await;
}
