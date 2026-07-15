# democrachat federation + encryption architecture

_Status: design. **M1–M5 complete.** M1 node identity (`crates/domain/src/node/`),
M2 signed change events (`crates/federation/` — Ed25519 keypairs + `ChangeEvent`),
M3 the in-memory control plane (`federation::ownership` + `federation::rehome` —
epoch-fenced ownership keyed by `OwnedScope`, load-aware rehoming), M4 the etcd
adapter (`crates/adapter-control-etcd/` — leases + compare-and-swap epoch fencing,
verified against real etcd). **M5 complete** — its security core:
`federation::authorize` (scope derived from payload, not the envelope) +
`classify` + `ingest` (authorize→apply gate). Feed *production* is also done:
`federation::sign_feed` turns a node's change-capture outbox (`ChangeSource` →
`ChangeRecord`) into a signed, epoch-stamped feed that round-trips through
`ingest` (and is fenced when produced under a stale epoch). The in-memory store
now **implements `ChangeSource`** (produce) **and `ChangeSink`** (consume,
`apply_incoming` — no outbox echo) with per-peer cursors, so a node's ordinary
writes become its replication feed and a peer's feed applies into its replica. The
**`adapter-federation`** crate carries this over HTTP: an axum feed server
(`GET /federation/changes` → `sign_feed`) plus a `reqwest` puller and a
`Replicator` with an ordered per-peer cursor (transient rejection → retry,
permanent → skip). Proven end-to-end over a real socket (node B replicates node A,
cursor advances, bad bearer token refused). The **composition root** now wires it:
with federation env set (`DEMOCRACHAT_NODE_SEED` + etcd endpoints + feed bind), the
node connects to the etcd control plane, publishes its key, claims the server
scopes it minted, mirrors each server's rehoming policy into the control plane, and
spawns the feed server + puller — a guarded no-op on the default single-box
deployment. **Command forward is also done:** a node writing to a scope it doesn't
own sends a node-signed `SignedCommand` (nonce + freshness) to the owner, which
authenticates it against the forwarder's published key, checks it owns the target
scope, guards replay, then runs the domain use-case to mint the canonical event
(`CastVote` first; HTTP `POST /federation/command` + client, proven over a real
socket incl. replay refusal). Both sides are **wired into the composition root**:
the owner runs forwarded votes through `app::Services` (`cast_vote_by_id`, which
re-checks citizenship and mints the canonical event), and the web vote handler
routes through a `WriteRouter` (apply-here-or-forward) via the new `app::VoteRouter`
port — `None`, so unchanged, on the single-box deployment. **M5 done.** **M6 (at
rest) done:** the persisted snapshot is envelope-sealed by `app::vault`
(ChaCha20-Poly1305) under a per-node key from `DEMOCRACHAT_DATA_KEK`, unsealed only
in RAM; unset key ⇒ plaintext (unchanged default), verified end-to-end. Next: M7
(E2EE private surface + key mgmt). Still pending: live 2-node verification on the
Docker etcd harness; wire-confidentiality TLS at the edge (deployment). M7–M9
planned. This is the north-star document — each milestone links back to a section
here._

democrachat is being taken from a single in-memory process to a **federated,
privacy-first network**: communities (servers) sharded across nodes, a global
social graph homed per user, a real control plane, and an encryption model where
the private surface is server-blind. The design borrows the proven mechanics of
the `../democratos` sibling (composite IDs, Ed25519-signed change events, etcd
ownership leases, epoch fencing, rehoming) and adds the two things democratos
never had to solve: a **cross-cutting social graph** (DMs/friends/blocks that
belong to no community) and **end-to-end encryption** of the private surface.

## 1. Goals

1. **Shard servers across nodes.** Each server is owned by one node, which is the
   source of truth for its channels, messages, proposals, votes, roles, rules,
   emoji, and memberships. Reads are local to a replica; writes forward to the
   owner; a dead node's servers rehome onto a live one.
2. **Home the global social graph per user.** Accounts, DMs, friendships, and
   blocks are owned by a user's **home node** and replicated to the nodes that
   need them, with a block honoured on every node it touches.
3. **A real control plane from day one.** etcd holds ownership leases; ownership
   is fenced by monotonic **epochs**; a lease lapse triggers **rehoming**.
4. **Encrypt the private surface end-to-end** (server-blind) and the governance
   surface **at rest and on the wire** (node-readable only in RAM, to tally).
5. **Zero-coordination single box stays identical.** Node `0` is the bootstrap
   identity; an un-federated deployment mints the same `1, 2, 3…` IDs and runs
   exactly as today. Federation is strictly additive.

## 2. The two sharding axes

democrachat's state divides into two partitions with different owners:

| Surface | Stores | Sharded by | Owner |
| ------- | ------ | ---------- | ----- |
| **Server-scoped** | channel, message, reaction, proposal, vote, role, rule, emoji, membership | `ServerId` | the server's **owning node** (dynamic, via the control plane) |
| **User-global** | user, dm, friend, block | `UserId` | the user's **home node** (fixed at account creation) |

The governance rules are **unaware** either axis exists — they remain pure
functions over entities + `now`. Only *ID allocation*, *storage*, and *routing*
change.

## 3. Node identity & composite IDs  — **M1**

Every entity ID is a `u64`, partitioned so any node mints globally-unique IDs
with no coordinator:

```
 63            48 47                                   0
┌────────────────┬──────────────────────────────────────┐
│  node (16 bit) │            sequence (48 bit)          │
└────────────────┴──────────────────────────────────────┘
```

- High 16 bits = the **origin node** (who minted it). `origin_node(id)` recovers it.
- Low 48 bits = that node's local monotonic **sequence**.
- **Node 0 is reserved** for the single-box/bootstrap deployment: `compose_id(0,
  n) == n`, so existing data and the JSON store keep working untouched.

`crates/domain/src/node/`: `NodeId`, `compose_id`, `origin_node`,
`local_sequence`, `MAX_SEQUENCE`, `SEQUENCE_BITS`, `SEQUENCE_MASK`. The store
mints `compose_id(self.node, seq)`; with the default node 0 this is a numeric
no-op (proven by the existing suite still passing).

The origin node is the entity's *bootstrap* owner — a routing default. **Current**
ownership is authoritative only in the control plane (§6), because servers rehome.

## 4. Trust model & signed change events — **M2**

Once state replicates across a network, the transport and peers are **untrusted**.
A hostile node must not forge, tamper, replay, or impersonate. The control:

- Each node has a **persistent Ed25519 identity** — a 32-byte seed loaded from a
  keyfile / secret env var, never generated per boot (that would invalidate every
  signature it ever made).
- Every state change that leaves a node is a **`ChangeEvent`** carrying a
  signature over its canonical **`SignedPart`**:
  `(node, epoch, seq, scope, entity, op, payload_hash)`.
  - `node`+`epoch`+`seq` → replay & split-brain protection (an event minted under
    a stale ownership epoch, or below the consumer's cursor, is rejected even if
    correctly signed).
  - `scope` (a `ServerId` or a `UserId`) → stops an event being replayed against
    a different server/user.
  - A consumer verifies against **the rightful owner's** public key, resolved
    from the control plane — not from the event itself.
- **Account trust** (user homing) uses an offline **issuer root**: each node
  holds an issuer cert chaining to a `FEDERATION_TRUST_ROOT`, so a user's home
  node is a trusted issuer of that account fleet-wide. (democratos's issuer model,
  reused.)

`crates/federation/` (new): `NodeKeypair`, `NodePublicKey`, `SignedPart`,
`ChangeEvent`, `ChangeOp`, issuer certs. Pure crypto + types; no IO.

## 5. Encryption model — **M6 (at rest) + M7 (E2EE)**

The boundary (a decided design point): **the private surface is server-blind; the
governance surface is node-readable but encrypted at rest and on the wire.**

### 5a. Server-blind (E2EE to user/group keys) — **M7**
Nodes store and replicate only **ciphertext** for:
- **DMs** — sealed to the recipient's and sender's device keys.
- **Friend/block graph** — the *fact* of a relationship is metadata the owning
  node needs for routing/enforcement, but its human-visible content (notes, who)
  is minimised; blocks are enforced by opaque, salted user-pair tags so a node
  can honour a block without learning the pair in the clear where avoidable.
- **Channel message *bodies*** — encrypted to a per-channel **group key**.
  Crucially, **contribution scoring depends on *who reacted to whose message*, not
  on body text** (per the M1 chat design), so governance keeps working on bodies it
  can never read.
  - **History visibility is a governed, per-channel setting** (a community
    decision, consistent with the whole project). An **open-history** channel uses
    a long-lived channel key handed to every member, so a new member can read the
    backlog (the Discord/Slack expectation). An **ephemeral** channel **ratchets**
    the key on membership change (MLS-style), so a new — or compromised — member
    only ever sees messages from when they joined onward (forward secrecy). The
    server chooses per channel via its normal ballot/channel config.
  - **DMs always ratchet** — no history is ever shared with a third party.

Keys never leave clients in the clear. A user's key material is derived/unwrapped
client-side (§5c). The server is a ciphertext store + a blind relay.

### 5b. Node-readable, encrypted-at-rest (governance) — **M6 ✅ (at rest)**
Votes, memberships, proposals, roles, rules, and reaction *metadata* must be
readable by the owning node (to tally, evaluate eligibility, enforce roles) and by
peers (to validate replicated governance). These are:
- **Encrypted at rest** — **done.** The persisted snapshot is sealed by the
  `app::vault` module (`crates/app/src/vault/`, ChaCha20-Poly1305 AEAD): a fresh
  per-snapshot data key encrypts the data and the node's root key (`VaultKey`,
  loaded at boot from `DEMOCRACHAT_DATA_KEK`, held only in RAM) wraps it — envelope
  encryption, so the root key can rotate without re-encrypting history. The
  composition root seals on save and unseals on load; **unset key ⇒ plaintext**
  (the single-box dev default, unchanged), and an existing plaintext file migrates
  to sealed on its next save. A sealed file cannot be loaded without the key (a
  hard error, never a silent empty-over-encrypted). Verified end-to-end: the file
  is an opaque `{"vault":1,…}` envelope with no plaintext handles; reads back with
  the key; refused without it. *Scope note:* M6 seals the **whole** snapshot at
  rest (a superset of the governance surface); M7 layers E2EE on the private
  entities' content on top.
- **Encrypted on the wire** — the signed Ed25519 envelope (§4) gives integrity +
  authenticity today; **confidentiality on the wire is TLS** terminated at the
  node-only edge (a deployment concern, like the web tier — the `reqwest` puller
  already pins `rustls`).
- Decrypted **only in memory**, only to compute — the in-memory store holds
  plaintext in RAM (that *is* the compute state) and never writes it back in the
  clear.

### 5c. Key management — **M7 ◐ (crypto foundation done)**
Foundation shipped in `app::e2ee` (M7.1): X25519 device keys
(`IdentitySecret`/`PublicIdentity`), a sealed-box (`seal_to`/`open_sealed`, fresh
ephemeral key per message so the server is blind), and password-wrapping of the
device secret (`wrap_secret`/`unwrap_secret`, Argon2id KEK → the only form the
server holds). Runs client-side in the real system; in `app` so the CLI client and
tests exercise it and a JS client mirrors the formats. Remaining: the server-blind
key directory (publish/fetch keys), sealed DM + message-body storage, the governed
open-vs-ephemeral history modes, and the web JS client.
- **User identity key**: an Ed25519/X25519 device key. The private key is wrapped
  by a key derived from the user's password (Argon2id → KEK) so it can be
  recovered on a new device by re-entering the password; the server stores only
  the wrapped blob (server-blind).
- **Per-conversation / per-channel keys**: symmetric keys sealed to member device
  keys; rotated on membership change.
- **Recovery**: losing the password loses the E2EE history (documented, honest
  trade-off) unless the user set up a recovery key.

## 6. Control plane: ownership, epochs, rehoming — **M3 (model) + M4 (etcd)**

- **Ownership registry** — maps each `ServerId` (and each `UserId` home) to its
  current owner node + **epoch**. Backed by **etcd** leases: an owner holds a
  lease; losing it (crash, partition) frees the server to rehome.
- **Epoch fencing** — every ownership hand-off bumps the epoch. Signed events
  carry the epoch; a restarted old owner's stale-epoch events are rejected, so a
  split brain can't double-apply.
- **Rehoming controller** — on a lapsed lease, a standby node claims ownership
  (bumping the epoch), replays the server's log, and starts serving. Chooses the
  least-loaded node (load reported to etcd).
- **Community rehoming opt-out** (governed) — a server's citizens can **disable
  automatic rehoming** for their server (`can_rehome` / `set_rehoming` on the
  registry; `RehomeOutcome::RehomingDisabled`). A disabled server is left down
  until its home node returns rather than being migrated onto a node the community
  did not choose — trading availability for **sovereignty over where their data
  lives**, which matters more here than in democratos because even the replicated
  data is theirs to place. This gates only *automatic* failover; an explicit
  governance-approved handoff still works. **Both halves now exist:** the
  control-plane mechanism (M3) and the **ballot** —
  `ProposalKind::SetRehomingPolicy` (a Constitutional-class vote) sets the
  `Server::is_rehoming_disabled` policy field, exposed in the proposal UI. The one
  remaining wire is the **sync** from that domain flag to `registry.set_rehoming`
  when a federated node composes the registry in (M4+); until then the domain flag
  is authoritative and a single-box deployment has nothing to rehome anyway.

M3 builds the pure ownership/epoch/rehome logic with an **in-memory registry**
(fully testable, no etcd), including the rehoming opt-out. M4 is the
`adapter-control-etcd` binding.

## 7. Replication transport — **M5**

Two HTTP surfaces on an internal `:7400`-style port, never public:
- **Feed (pull)** — a node pulls `changes_since(cursor)` from each peer and
  applies verified events to its replica. Cursor = per-origin `(node → seq)`.
- **Command (forward)** — a write to a server this node doesn't own is forwarded
  to the owner as a **signed command**; the owner executes, mints the canonical
  event, and feeds it back. Replay-guarded by a **nonce log** + skew window.
- **Auth** — peers present a shared `DEMOCRACHAT_CLUSTER_TOKEN` (bearer) *and*
  per-event Ed25519 signatures; the token gates the port, the signatures gate
  trust.

`crates/adapter-federation/` (new).

## 8. The federated social graph — **M8** (the hard part)

DMs, friends, and blocks belong to no server, so they follow the **user home**:
- **DM between alice@node1 and bob@node2** — owned by the sender's home for its
  own copy; the event is replicated to the recipient's home so both users can read
  their thread from their own node. Bodies are E2EE (§5a), so nodes relay
  ciphertext.
- **Blocks are safety-critical and must hold everywhere.** A block is replicated
  to both users' homes (and to any node hosting a shared server) and is enforced
  before *any* DM delivery or friend action. Eventual consistency is unacceptable
  for blocks, so a block write is **synchronously acknowledged by the counterpart
  home** before it's reported successful (a small, bounded 2-home commit), unlike
  friends which may lag.
- **Directory** — global handle uniqueness + "which node homes this user" lives in
  etcd (a claim on `user/<handle>`), so two nodes can't mint the same handle.

## 9. Sessions

Already solved: sessions are **stateless HMAC** (`sid = uid.exp.mac`) over a
secret shared by all nodes, so a cookie minted on one node verifies on any node
for free. No session replication needed. (The shared `DEMOCRACHAT_SESSION_SECRET`
is already a deploy requirement.)

## 10. Milestone roadmap

Each milestone is independently testable and leaves the tree green.

| M | Title | Crate(s) | Delivers |
| - | ----- | -------- | -------- |
| **M1** ✅ | Node identity & composite IDs | `domain/node`, `adapter-store-memory` | globally-unique IDs; node 0 = today, unchanged |
| **M2** ✅ | Node keypairs + signed change events | new `federation` | forge/tamper/replay-proof event envelope |
| **M3** ✅ | Ownership + epochs + rehome (in-memory) | `federation/ownership`,`rehome` | pure control-plane logic, etcd-free tests |
| **M4** ✅ | etcd control-plane adapter | new `adapter-control-etcd` | real leases, fencing, load reporting |
| **M5** ✅ | Replication transport (feed + command) | `federation` (authorize/ingest/sign_feed) + `adapter-federation` (HTTP feed pull + command forward + WriteRouter) + composition wiring | replicas + write forwarding between nodes |
| **M6** ✅ (at rest) | Encryption at rest (governance surface) | `app::vault` (ChaCha20-Poly1305 envelope) + comp root save/load | node key wraps a per-snapshot DEK; ciphertext on disk. Wire confidentiality = TLS at the edge |
| **M7** | E2EE private surface + key management | `adapter-web` (client), `federation` | server-blind DMs/blocks/message bodies |
| **M8** | Federated social graph | `adapter-federation`, `app` | homed users; cross-home DMs; block 2-home commit |
| **M9** | Deploy: multi-node etcd + issuer ceremony | `deploy/` | prod federation, cross-host TLS |

## 11. Open questions / risks

- **Channel group-key rotation at scale** (MLS-style) is the hardest crypto piece;
  M7 may start with sender-keys + periodic rotation before full MLS.
- **Block privacy vs. enforcement** — a node must enforce a block it ideally
  shouldn't be able to read; §8 uses salted pair-tags, which limits but doesn't
  eliminate metadata exposure. Revisit in M7/M8.
- **Governance validation of replicated events** requires peers to recompute
  tallies; large servers make the feed heavy. Consider snapshot + delta.
- **Rehoming a server mid-ballot** must preserve the timelock/recall window; the
  epoch-fenced log replay must be deterministic.
