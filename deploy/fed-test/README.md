# Live 2-node federation harness

Proves the cross-home machinery (M5–M8) end to end against a **real etcd control
plane** and **two federated democrachat processes** — not mocks. What it exercises:

80 assertions across six areas — **functional** (incl. cross-home vote forwarding),
**federation security**, **web-auth security**, **social-graph authority & cross-home
DM policy**, **control-plane resiliency** (a transient etcd partition must not evict a
healthy node), and **automatic failover** — all against a real etcd + two federated
processes.

### Functional (cross-home correctness)

| Scenario | What it proves |
|---|---|
| Register users on their home nodes, read them on the other | signed **feed replication** + node-scoped authorization (`owner_of(scope) == signer`) |
| ben→ana and ana→ben DMs issued via the *wrong* node | **command forwarding**: a write to a scope this node doesn't own is signed and forwarded to the owner, which applies it |
| ana (node1) friends cyd (node2): request then accept | friendship as a **2-home synchronous commit** (visible on both homes) |
| ana (node1) blocks ben (node2) | permanent block as a **2-home synchronous commit** — verified by ben's DM gate on node2 refusing him afterward |
| ana founds a server + proposal on node1, then votes via node2 | **cross-home vote forwarding** (M5 headline): node2 signs and forwards the `CastVote` to the server's owner (node1), which re-checks citizenship and mints the canonical tally |

### Failover — automatic rehoming

Kill node 1 (which homes ana). Once its etcd lease lapses, node 2 (ana's designated
standby) **auto-promotes itself** for ana's home — a write that needs it starts
working again with no restart. When node 1 returns it is **epoch-fenced** (cannot
reclaim) and instead forwards ana's writes to the new owner.

### Security — federation node-only endpoints (the cluster trust boundary)

Adversarial probes against the real feed/command ports (`4101`/`4102`):

- feed + command reject a **missing or wrong bearer token** (401); correct token is a positive control.
- a malformed command body is rejected (4xx) without crashing the node.
- **the crypto gate**: a command with the right bearer token but a **forged Ed25519 signature** (claiming node 1) is refused (422) — knowing the cluster token is *not* enough to inject a write; you also need the node's private key.
- a command signed by a **node whose key was never published** to the control plane is refused.

### Security — web auth boundaries (checked on *both* nodes)

- unauthenticated read → 401; **forged `sid` session cookie** → 401.
- **IDOR**: one user cannot read another's social state, read their DM conversation, or act as them — including **across nodes** (the shared session secret authenticates the user but never lets them *be* someone else).
- **CSRF**: a mutation with no token, or a cookie/header mismatch, is refused (403).
- **DoS cap**: a >64 KB body is rejected (413) before any handler runs.
- hardening headers present (CSP, `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`).

### Security — social-graph authority & cross-home DM policy

Beyond the transport, the *rules* of the social graph are probed directly:

- a user cannot friend-request **themselves**, cannot **accept a request that doesn't
  exist**, and cannot accept a request they aren't the **addressee** of (only the
  recipient of a pending request may accept); a duplicate friend-request is an
  idempotent no-op.
- the DM-policy endpoint is **self-only** — one user cannot flip another's policy, even
  by aiming the call at the other node (the shared session authenticates *who* you are,
  never lets you act *as* someone else).
- **friends-only DM policy is a replicated, cross-home-enforced control.** eli (home
  node 2) switches to friends-only; a non-friend dot (home node 1) DMing her is refused
  — but the gate runs on dot's home, so it only holds once eli's policy **mutation**
  replicates node 2 → node 1. Reverting to *everyone* re-opens it after the revert
  replicates. This proves a privacy setting changed on one home is honored by a remote
  home, and that *updates* (not just account creation) flow over the signed feed.

### Resiliency — a transient etcd partition must NOT trigger failover

Freeze etcd (`docker pause`) for a blip **shorter than the lease TTL**, then thaw it.
The cluster must ride it out: nodes keep serving **local reads** during the outage
(those never touch etcd), node 1 **retains its lease** (zero new rehome promotions —
measured as a before/after delta so stale log history can't mask a regression), and a
**cross-node write** works again once etcd returns. A brief control-plane hiccup is
not a node death, so it must not silently evict a healthy owner and hand its scopes to
a standby. This is what makes it survivable: the lease keep-alive **reconnects on a
short backoff** (not a full renewal period) and **time-boxes each round-trip**, so the
instant etcd is reachable again it renews within the lease's remaining budget instead
of sleeping through it (`crates/adapter-control-etcd/src/lib.rs`).

### Failover — partition tolerance + no half-truth

Kill node 1, then: node 2 keeps serving its own users; a 2-home write that needs the
downed home **fails loudly** (never silently reports full success); and after node 1
restarts and re-claims its scopes, re-driving the write **converges** (idempotent).

## Run it (fast: local binaries + Docker etcd)

```sh
deploy/fed-test/run-local.sh
```

Needs Docker (for etcd) and a Rust toolchain. It builds the debug binary, launches
node 1 on `:5101` and node 2 on `:5102` wired to a throwaway etcd, runs the
scenarios, prints `PASS`/`FAIL` per assertion, and tears everything down. Exit code
is non-zero if any assertion fails, so it drops into CI as-is. Re-runnable — it wipes
the etcd prefix and uses fresh data files each time.

## Run it (fully containerized)

```sh
docker compose -f deploy/fed-test/docker-compose.yml up --build
# then point run-local.sh's scenarios (or your own curl) at :5101 / :5102
```

## Why a shared session secret

Both nodes share `DEMOCRACHAT_SESSION_SECRET`, so a session cookie minted on one node
is accepted on the other (sessions are stateless HMAC). That lets the harness issue a
user's write through *either* node — which is exactly how it forces the forwarding
path. **This is a test convenience, not a production topology**: real deployments run
one node per host behind its own TLS edge.

## Two bugs this harness caught

Building the live drill surfaced two races that every prior (mock-based) milestone
missed, both now fixed:

1. **Rehoming stole live nodes' scopes.** A scope is momentarily unowned every time
   its home mints it (a fresh user/server is unowned until the home's next reconcile
   claims it). The rehoming loop raced the home and promoted itself for the live
   scope. Fix: only rehome a scope whose **home node is absent from `live_nodes`** —
   the real "it's down" signal (`crates/democrachat/src/federation.rs`).

2. **A just-minted scope's rows were lost forever.** `sign_feed` signed rows for a
   scope the node didn't own yet, stamping a placeholder epoch 0. A peer received the
   row, then the scope got claimed at epoch 1, and the peer **permanently skipped**
   the row as `StaleEpoch` — never retrying. Fix: a node only signs feed rows for
   scopes it **currently owns**; an unowned scope's rows are simply offered on a
   later pull once claimed, with the real epoch (`crates/federation/src/sign_feed.rs`).

## The claim reconciler

Federation homes a scope on the node that minted its id, but ids are minted at
runtime (a user registers, a server is founded). The composition root
(`crates/democrachat/src/federation.rs`) runs a **claim reconciler** — once before
serving, then every few seconds — that claims every local-origin `Server`/`UserHome`
scope in etcd. Without it, entities created after boot would be unowned: their home
writes unroutable and even their feed rows unauthorized. `claim` is idempotent (an
already-held scope returns `Held` without bumping the epoch), so re-running is free.
