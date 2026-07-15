//! An in-memory implementation of every driven port, plus two clocks. Suitable
//! for tests and for the CLI demo. Nothing is persisted — the process exit loses
//! everything.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use app::{
    BlockStore, CapAdmission, ChannelKeyStore, ChannelStore, Clock, DmStore, EmojiStore,
    EmojiVoteStore, FriendStore, KeyDirectoryStore, MembershipStore, MessageStore, ProposalStore,
    ReactionStore, RoleColorVoteStore, RoleStore, RuleStore, ServerStore, StoreError, Stores,
    UserStore, VoteStore,
};
use domain::{
    compose_id, Block, Channel, ChannelId, ChannelKeyGrant, DmId, DmMessage, Emoji, EmojiId,
    EmojiVote, Friendship, Invite, Membership, Message, MessageId, NodeId, Proposal, ProposalId,
    Reaction,
    Role, RoleColor, RoleColorVote, RoleId, Rule, RuleId, Server, ServerId,
    Timestamp, User, UserId, UserKeys,
    Vote,
};
use federation::{ChangeOp, ChangeRecord, ChangeSink, ChangeSource, ReplicationCursor, SignedPart};
use serde::{Deserialize, Serialize};

/// A clock fixed at a chosen instant — the controllable clock the time-based
/// governance rules are tested and demonstrated against.
pub struct FixedClock(Mutex<Timestamp>);

impl FixedClock {
    pub fn new(at: Timestamp) -> Self {
        Self(Mutex::new(at))
    }
    /// Move the clock to `at` (the demo advances time to age accounts).
    pub fn set(&self, at: Timestamp) {
        *self.0.lock().unwrap() = at;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        *self.0.lock().unwrap()
    }
}

/// The wall clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Timestamp(secs)
    }
}

#[derive(Default)]
struct Inner {
    users: HashMap<UserId, User>,
    servers: HashMap<ServerId, Server>,
    memberships: HashMap<(UserId, ServerId), Membership>,
    channels: HashMap<ChannelId, Channel>,
    messages: HashMap<MessageId, Message>,
    reactions: Vec<Reaction>,
    proposals: HashMap<ProposalId, Proposal>,
    votes: Vec<Vote>,
    emojis: HashMap<EmojiId, Emoji>,
    emoji_votes: Vec<EmojiVote>,
    rules: HashMap<RuleId, Rule>,
    dms: Vec<DmMessage>,
    blocks: Vec<Block>,
    friends: Vec<Friendship>,
    roles: HashMap<RoleId, Role>,
    role_color_votes: Vec<RoleColorVote>,
    /// Server invite codes, keyed by their SHA-256 digest (never the raw code).
    invites: HashMap<String, Invite>,
    /// Transient media blobs (key → (content_type, bytes)). Deliberately outside the
    /// federation feed and the persistence snapshot — media is a separate storage
    /// concern (a dedicated media node in production; this in-memory map is only the
    /// dev/test adapter). `media_seq` gives each upload a distinct key.
    media: HashMap<String, (String, Vec<u8>)>,
    media_seq: u64,
    /// The server-blind key directory: one published entry per user.
    user_keys: HashMap<UserId, UserKeys>,
    /// Sealed channel-key grants for encrypted channels, keyed
    /// `(channel, epoch, member)`. Each blob is opaque to the server.
    channel_grants: HashMap<(ChannelId, u32, UserId), ChannelKeyGrant>,
    next_user: u64,
    next_server: u64,
    next_channel: u64,
    next_message: u64,
    next_proposal: u64,
    next_rule: u64,
    next_dm: u64,
    next_role: u64,
    next_emoji: u64,
    /// This deployment's node identity, mixed into every minted ID so a federated
    /// network stays collision-free without coordinating. Defaults to `NodeId(0)`
    /// (the single-box identity), which leaves IDs numerically unchanged.
    node: NodeId,
    /// The change-capture **outbox**: every mutation this node makes, in order, as
    /// the raw material [`federation::sign_feed`] turns into a signed replication
    /// feed. Scope is not stored — it is re-derived from each payload by
    /// `federation::classify`, so producer and consumer can never disagree.
    outbox: Vec<ChangeRecord>,
    /// Monotonic outbox sequence — a peer's replay cursor for this node's feed.
    next_outbox: u64,
    /// Per-origin replication cursor: the highest `seq` this node has applied from
    /// each peer's feed (keyed by the peer's [`NodeId`]). A pull resumes from here.
    cursors: HashMap<NodeId, u64>,
    /// Durable anti-replay log for forwarded commands: `(forwarding node, nonce) →
    /// expiry`. Persisted in the snapshot so a captured command can't be replayed
    /// against this node after a restart. Self-prunes once entries pass expiry.
    nonces: HashMap<(u16, String), i64>,
}

impl Inner {
    /// Append one mutation to the outbox. Called while the lock is held, right
    /// after the row is written, so the outbox order matches the write order.
    fn record(&mut self, entity: &'static str, op: ChangeOp, payload: serde_json::Value) {
        self.next_outbox += 1;
        self.outbox.push(ChangeRecord {
            seq: self.next_outbox,
            entity: entity.into(),
            op,
            payload,
        });
    }
}

/// Serialize a row for the outbox. The domain types all derive `Serialize` (they
/// are already snapshotted to JSON), so this is infallible in practice; a `Null`
/// fallback keeps the store panic-free rather than unwrapping.
fn to_payload<T: Serialize>(row: &T) -> serde_json::Value {
    serde_json::to_value(row).unwrap_or(serde_json::Value::Null)
}

/// Apply one **authorized** peer change into the replica — the mirror of the write
/// side of each port, minus the `record(...)` call: an ingested row must never
/// echo back into this node's outbox, or it would loop around the network forever.
/// `part` is trusted (signed by the row's rightful owner at a non-stale epoch, per
/// [`federation::authorize`]); an `Err` is a *local* apply failure — a payload that
/// does not deserialize into the domain type its entity claims, which would poison
/// the replica if written.
fn apply_row(inner: &mut Inner, part: &SignedPart) -> Result<(), String> {
    let p = &part.payload;
    macro_rules! row {
        ($t:ty) => {
            serde_json::from_value::<$t>(p.clone()).map_err(|e| e.to_string())?
        };
    }
    let u64_field = |key: &str| p.get(key).and_then(|v| v.as_u64());
    let str_field = |key: &str| p.get(key).and_then(|v| v.as_str());

    match (part.entity.as_str(), part.op) {
        ("users", ChangeOp::Upsert) => {
            let u: User = row!(User);
            inner.users.insert(u.id, u);
        }
        ("users", ChangeOp::Delete) => {
            let u: User = row!(User);
            inner.users.remove(&u.id);
        }
        ("servers", ChangeOp::Upsert) => {
            let s: Server = row!(Server);
            inner.servers.insert(s.id, s);
        }
        ("servers", ChangeOp::Delete) => {
            let s: Server = row!(Server);
            inner.servers.remove(&s.id);
        }
        ("invites", ChangeOp::Upsert) => {
            let i: Invite = row!(Invite);
            inner.invites.insert(i.code_hash.clone(), i);
        }
        ("memberships", ChangeOp::Upsert) => {
            let m: Membership = row!(Membership);
            inner.memberships.insert((m.user_id, m.server_id), m);
        }
        ("memberships", ChangeOp::Delete) => {
            let m: Membership = row!(Membership);
            inner.memberships.remove(&(m.user_id, m.server_id));
        }
        ("channels", ChangeOp::Upsert) => {
            let c: Channel = row!(Channel);
            inner.channels.insert(c.id, c);
        }
        ("channels", ChangeOp::Delete) => {
            let c: Channel = row!(Channel);
            inner.channels.remove(&c.id);
            inner.messages.retain(|_, m| m.channel_id != c.id);
        }
        ("messages", ChangeOp::Upsert) => {
            let m: Message = row!(Message);
            inner.messages.insert(m.id, m);
        }
        ("messages", ChangeOp::Delete) => {
            let m: Message = row!(Message);
            inner.messages.remove(&m.id);
        }
        ("reactions", ChangeOp::Upsert) => {
            let r: Reaction = row!(Reaction);
            let dup = inner
                .reactions
                .iter()
                .any(|x| x.message_id == r.message_id && x.user == r.user && x.emoji == r.emoji);
            if !dup {
                inner.reactions.push(r);
            }
        }
        ("reactions", ChangeOp::Delete) => {
            let (mid, uid, emoji) = (u64_field("message_id"), u64_field("user"), str_field("emoji"));
            inner.reactions.retain(|r| {
                !(Some(r.message_id.0) == mid && Some(r.user.0) == uid && Some(r.emoji.as_str()) == emoji)
            });
        }
        ("proposals", ChangeOp::Upsert) => {
            let pr: Proposal = row!(Proposal);
            inner.proposals.insert(pr.id, pr);
        }
        ("proposals", ChangeOp::Delete) => {
            let pr: Proposal = row!(Proposal);
            inner.proposals.remove(&pr.id);
        }
        ("votes", ChangeOp::Upsert) => {
            let v: Vote = row!(Vote);
            inner.votes.retain(|x| !(x.proposal_id == v.proposal_id && x.voter == v.voter));
            inner.votes.push(v);
        }
        ("votes", ChangeOp::Delete) => {
            let v: Vote = row!(Vote);
            inner.votes.retain(|x| !(x.proposal_id == v.proposal_id && x.voter == v.voter));
        }
        ("emojis", ChangeOp::Upsert) => {
            let e: Emoji = row!(Emoji);
            inner.emojis.insert(e.id, e);
        }
        ("emoji_votes", ChangeOp::Upsert) => {
            let v: EmojiVote = row!(EmojiVote);
            inner
                .emoji_votes
                .retain(|x| !(x.emoji_id == v.emoji_id && x.voter == v.voter));
            inner.emoji_votes.push(v);
        }
        ("rules", ChangeOp::Upsert) => {
            let r: Rule = row!(Rule);
            inner.rules.insert(r.id, r);
        }
        ("rules", ChangeOp::Delete) => {
            let r: Rule = row!(Rule);
            inner.rules.remove(&r.id);
        }
        ("dms", ChangeOp::Upsert) => {
            let m: DmMessage = row!(DmMessage);
            if !inner.dms.iter().any(|x| x.id == m.id) {
                inner.dms.push(m);
            }
        }
        ("blocks", ChangeOp::Upsert) => {
            let b: Block = row!(Block);
            let dup = inner.blocks.iter().any(|x| x.blocker == b.blocker && x.blocked == b.blocked);
            if !dup {
                inner.blocks.push(b);
            }
        }
        ("friendships", ChangeOp::Upsert) => {
            let f: Friendship = row!(Friendship);
            inner.friends.retain(|x| !x.is_between(f.requester, f.addressee));
            inner.friends.push(f);
        }
        ("user_keys", ChangeOp::Upsert) => {
            let k: UserKeys = row!(UserKeys);
            inner.user_keys.insert(k.user_id, k);
        }
        ("channel_grants", ChangeOp::Upsert) => {
            let g: ChannelKeyGrant = row!(ChannelKeyGrant);
            inner.channel_grants.insert((g.channel_id, g.epoch, g.member), g);
        }
        ("roles", ChangeOp::Upsert) => {
            let r: Role = row!(Role);
            inner.roles.insert(r.id, r);
        }
        ("roles", ChangeOp::Delete) => {
            let r: Role = row!(Role);
            inner.roles.remove(&r.id);
        }
        ("role_color_votes", ChangeOp::Upsert) => {
            let v: RoleColorVote = row!(RoleColorVote);
            inner
                .role_color_votes
                .retain(|x| !(x.role_id == v.role_id && x.voter == v.voter));
            inner.role_color_votes.push(v);
        }
        (other, _) => return Err(format!("unknown replicated entity `{other}`")),
    }
    Ok(())
}

/// One store backing all three persistence ports. Share a single `Arc<MemoryStore>`
/// as `UserStore`, `ServerStore`, and `MembershipStore` at once.
#[derive(Default)]
pub struct MemoryStore(Mutex<Inner>);

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set this store's node identity — mixed into every newly-minted ID so a
    /// federated network stays collision-free (see [`domain::compose_id`]). Node
    /// identity is a deployment property from config/env, not persisted data, so
    /// it is applied after `new()`/`from_json()` rather than restored from the
    /// snapshot. `NodeId(0)` (the default) leaves IDs numerically unchanged.
    pub fn with_node(self, node: NodeId) -> Self {
        self.0.lock().unwrap().node = node;
        self
    }

    /// Present this one store as the full [`Stores`] bundle — the same `Arc`
    /// implements every port, so the composition root wires them in one call.
    pub fn as_stores(self: &Arc<Self>) -> Stores {
        Stores {
            users: self.clone(),
            servers: self.clone(),
            memberships: self.clone(),
            channels: self.clone(),
            messages: self.clone(),
            reactions: self.clone(),
            proposals: self.clone(),
            votes: self.clone(),
            emojis: self.clone(),
            emoji_votes: self.clone(),
            rules: self.clone(),
            dms: self.clone(),
            blocks: self.clone(),
            friends: self.clone(),
            roles: self.clone(),
            role_color_votes: self.clone(),
            keys: self.clone(),
            channel_keys: self.clone(),
            invites: self.clone(),
            media: self.clone(),
            // No codec by default — the composition root overrides this with the
            // re-encoding adapter; tests and codec-free drivers keep the identity.
            image: Arc::new(app::PassthroughTranscoder),
        }
    }

    /// Serialize the whole dataset to a JSON string. The composition root uses
    /// this to persist between CLI invocations — the store itself stays a pure
    /// in-memory adapter; only the entry point decides whether to save.
    pub fn to_json(&self) -> serde_json::Result<String> {
        let inner = self.0.lock().unwrap();
        let snap = Snapshot {
            users: inner.users.values().cloned().collect(),
            servers: inner.servers.values().cloned().collect(),
            memberships: inner.memberships.values().cloned().collect(),
            channels: inner.channels.values().cloned().collect(),
            messages: inner.messages.values().cloned().collect(),
            reactions: inner.reactions.clone(),
            proposals: inner.proposals.values().cloned().collect(),
            votes: inner.votes.clone(),
            emojis: inner.emojis.values().cloned().collect(),
            emoji_votes: inner.emoji_votes.clone(),
            rules: inner.rules.values().cloned().collect(),
            dms: inner.dms.clone(),
            blocks: inner.blocks.clone(),
            friends: inner.friends.clone(),
            roles: inner.roles.values().cloned().collect(),
            role_color_votes: inner.role_color_votes.clone(),
            invites: inner.invites.values().cloned().collect(),
            user_keys: inner.user_keys.values().cloned().collect(),
            channel_grants: inner.channel_grants.values().cloned().collect(),
            nonces: inner.nonces.iter().map(|((n, nc), e)| (*n, nc.clone(), *e)).collect(),
            outbox: inner.outbox.clone(),
            next_outbox: inner.next_outbox,
            cursors: inner.cursors.iter().map(|(n, c)| (n.0, *c)).collect(),
            next_user: inner.next_user,
            next_server: inner.next_server,
            next_channel: inner.next_channel,
            next_message: inner.next_message,
            next_proposal: inner.next_proposal,
            next_rule: inner.next_rule,
            next_dm: inner.next_dm,
            next_role: inner.next_role,
            next_emoji: inner.next_emoji,
        };
        serde_json::to_string_pretty(&snap)
    }

    /// Load a dataset previously produced by [`MemoryStore::to_json`].
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        let snap: Snapshot = serde_json::from_str(json)?;
        let mut inner = Inner {
            reactions: snap.reactions,
            votes: snap.votes,
            emoji_votes: snap.emoji_votes,
            dms: snap.dms,
            blocks: snap.blocks,
            friends: snap.friends,
            role_color_votes: snap.role_color_votes,
            invites: snap
                .invites
                .into_iter()
                .map(|i| (i.code_hash.clone(), i))
                .collect(),
            outbox: snap.outbox,
            next_outbox: snap.next_outbox,
            cursors: snap.cursors.into_iter().map(|(n, c)| (NodeId(n), c)).collect(),
            next_user: snap.next_user,
            next_server: snap.next_server,
            next_channel: snap.next_channel,
            next_message: snap.next_message,
            next_proposal: snap.next_proposal,
            next_rule: snap.next_rule,
            next_dm: snap.next_dm,
            next_role: snap.next_role,
            next_emoji: snap.next_emoji,
            ..Inner::default()
        };
        for u in snap.users {
            inner.users.insert(u.id, u);
        }
        for g in snap.servers {
            inner.servers.insert(g.id, g);
        }
        for m in snap.memberships {
            inner.memberships.insert((m.user_id, m.server_id), m);
        }
        for c in snap.channels {
            inner.channels.insert(c.id, c);
        }
        for msg in snap.messages {
            inner.messages.insert(msg.id, msg);
        }
        for p in snap.proposals {
            inner.proposals.insert(p.id, p);
        }
        for r in snap.rules {
            inner.rules.insert(r.id, r);
        }
        for role in snap.roles {
            inner.roles.insert(role.id, role);
        }
        for e in snap.emojis {
            inner.emojis.insert(e.id, e);
        }
        for k in snap.user_keys {
            inner.user_keys.insert(k.user_id, k);
        }
        for g in snap.channel_grants {
            inner.channel_grants.insert((g.channel_id, g.epoch, g.member), g);
        }
        for (node, nonce, expiry) in snap.nonces {
            inner.nonces.insert((node, nonce), expiry);
        }
        Ok(Self(Mutex::new(inner)))
    }

    /// Record a forwarded command's `(node, nonce)`, returning `true` if it was
    /// newly seen (admit) or `false` if already present (a replay). Prunes entries
    /// past `now` first, so the log stays bounded. Backs the durable [`NonceLog`]
    /// (`adapter_federation::NonceLog`) the composition root wires up — because it
    /// lives in the persisted snapshot, a nonce survives a restart and a captured
    /// command still can't be replayed.
    pub fn remember_nonce(&self, node: u16, nonce: &str, now: i64, expiry_at: i64) -> bool {
        let mut inner = self.0.lock().unwrap();
        inner.nonces.retain(|_, expiry| *expiry > now);
        inner.nonces.insert((node, nonce.to_string()), expiry_at).is_none()
    }
}

/// A flat, serializable view of the whole store.
#[derive(Serialize, Deserialize)]
struct Snapshot {
    users: Vec<User>,
    servers: Vec<Server>,
    memberships: Vec<Membership>,
    #[serde(default)]
    channels: Vec<Channel>,
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default)]
    reactions: Vec<Reaction>,
    #[serde(default)]
    proposals: Vec<Proposal>,
    #[serde(default)]
    votes: Vec<Vote>,
    #[serde(default)]
    emojis: Vec<Emoji>,
    #[serde(default)]
    emoji_votes: Vec<EmojiVote>,
    #[serde(default)]
    rules: Vec<Rule>,
    #[serde(default)]
    dms: Vec<DmMessage>,
    #[serde(default)]
    blocks: Vec<Block>,
    #[serde(default)]
    friends: Vec<Friendship>,
    #[serde(default)]
    roles: Vec<Role>,
    #[serde(default)]
    role_color_votes: Vec<RoleColorVote>,
    #[serde(default)]
    invites: Vec<Invite>,
    #[serde(default)]
    user_keys: Vec<UserKeys>,
    #[serde(default)]
    channel_grants: Vec<ChannelKeyGrant>,
    /// Durable anti-replay nonces: `(node, nonce, expiry)`.
    #[serde(default)]
    nonces: Vec<(u16, String, i64)>,
    /// The change-capture outbox and its high-water `seq`. Persisted so a peer's
    /// replay cursor never runs ahead of the seqs this node mints: without it a
    /// restart would reset `next_outbox` to 0 and re-mint seq 1,2,3…, which every
    /// peer already past that cursor would silently filter out — a divergence.
    #[serde(default)]
    outbox: Vec<ChangeRecord>,
    #[serde(default)]
    next_outbox: u64,
    /// Per-origin replication cursors: how far this node has consumed each peer's
    /// feed. Persisted so a pull resumes where it left off instead of re-applying
    /// a peer's whole history after a restart.
    #[serde(default)]
    cursors: Vec<(u16, u64)>,
    next_user: u64,
    next_server: u64,
    #[serde(default)]
    next_channel: u64,
    #[serde(default)]
    next_message: u64,
    #[serde(default)]
    next_proposal: u64,
    #[serde(default)]
    next_rule: u64,
    #[serde(default)]
    next_dm: u64,
    #[serde(default)]
    next_role: u64,
    #[serde(default)]
    next_emoji: u64,
}

#[async_trait]
impl UserStore for MemoryStore {
    async fn next_user_id(&self) -> Result<UserId, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.next_user += 1;
        UserId(compose_id(inner.node, inner.next_user))
        })
    }
    async fn insert_user(&self, user: User) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("users", ChangeOp::Upsert, to_payload(&user));
        inner.users.insert(user.id, user);
        })
    }
    async fn update_user(&self, user: User) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("users", ChangeOp::Upsert, to_payload(&user));
        inner.users.insert(user.id, user);
        })
    }
    async fn get_user(&self, id: UserId) -> Result<Option<User>, StoreError> {
        Ok({
        self.0.lock().unwrap().users.get(&id).cloned()
        })
    }
    async fn find_by_handle(&self, handle: &str) -> Result<Option<User>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .users
            .values()
            .find(|u| u.handle == handle)
            .cloned()
        })
    }
    async fn list_all(&self) -> Result<Vec<User>, StoreError> {
        Ok({
        self.0.lock().unwrap().users.values().cloned().collect()
        })
    }
    async fn search_by_tag(&self, tag: &str) -> Result<Vec<User>, StoreError> {
        let Some(needle) = domain::Tags::search_needle(tag) else {
            return Ok(Vec::new());
        };
        Ok(self.0.lock().unwrap().users.values()
            .filter(|u| u.tags.as_str().contains(&needle))
            .cloned()
            .collect())
    }
}

#[async_trait]
impl ServerStore for MemoryStore {
    async fn next_server_id(&self) -> Result<ServerId, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.next_server += 1;
        ServerId(compose_id(inner.node, inner.next_server))
        })
    }
    async fn insert_server(&self, server: Server) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("servers", ChangeOp::Upsert, to_payload(&server));
        inner.servers.insert(server.id, server);
        })
    }
    async fn get_server(&self, id: ServerId) -> Result<Option<Server>, StoreError> {
        Ok({
        self.0.lock().unwrap().servers.get(&id).cloned()
        })
    }
    async fn find_by_slug(&self, slug: &str) -> Result<Option<Server>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .servers
            .values()
            .find(|g| g.slug == slug)
            .cloned()
        })
    }
    async fn update_server(&self, server: Server) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("servers", ChangeOp::Upsert, to_payload(&server));
        inner.servers.insert(server.id, server);
        })
    }
    async fn list_all(&self) -> Result<Vec<Server>, StoreError> {
        Ok({
        let mut v: Vec<Server> = self.0.lock().unwrap().servers.values().cloned().collect();
        v.sort_by_key(|g| g.id.0);
        v
        })
    }
    async fn search_by_tag(&self, tag: &str) -> Result<Vec<Server>, StoreError> {
        let Some(needle) = domain::Tags::search_needle(tag) else {
            return Ok(Vec::new());
        };
        Ok(self.0.lock().unwrap().servers.values()
            .filter(|s| s.tags.as_str().contains(&needle))
            .cloned()
            .collect())
    }
}

#[async_trait]
impl MembershipStore for MemoryStore {
    async fn upsert(&self, membership: Membership) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("memberships", ChangeOp::Upsert, to_payload(&membership));
        inner
            .memberships
            .insert((membership.user_id, membership.server_id), membership);
        })
    }
    async fn get(&self, user: UserId, server: ServerId) -> Result<Option<Membership>, StoreError> {
        Ok({
        self.0.lock().unwrap().memberships.get(&(user, server)).cloned()
        })
    }
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Membership>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .memberships
            .values()
            .filter(|m| m.server_id == server)
            .cloned()
            .collect()
        })
    }
    async fn citizen_count(&self, server: ServerId) -> Result<u64, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .memberships
            .values()
            .filter(|m| m.server_id == server && m.is_citizen())
            .count() as u64
        })
    }
    async fn admitted_since(&self, server: ServerId, since: Timestamp) -> Result<u64, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .memberships
            .values()
            .filter(|m| m.server_id == server && m.enfranchised_at.is_some_and(|at| at >= since))
            .count() as u64
        })
    }
    async fn admit_within_cap(
        &self,
        admitted: Membership,
        window_start: Timestamp,
        slots_open: &(dyn Fn(u64, u64) -> u64 + Send + Sync),
    ) -> Result<CapAdmission, StoreError> {
        // The whole check-then-write happens under the store's single lock, so it is
        // already atomic against any other admission — the memory analogue of the
        // Postgres `SELECT … FOR UPDATE` transaction.
        let mut inner = self.0.lock().unwrap();
        let server = admitted.server_id;
        let citizens = inner
            .memberships
            .values()
            .filter(|m| m.server_id == server && m.is_citizen())
            .count() as u64;
        let admitted_this_window = inner
            .memberships
            .values()
            .filter(|m| m.server_id == server && m.enfranchised_at.is_some_and(|at| at >= window_start))
            .count() as u64;
        if slots_open(citizens, admitted_this_window) == 0 {
            return Ok(CapAdmission::RateCapped { admitted_this_window });
        }
        inner.record("memberships", ChangeOp::Upsert, to_payload(&admitted));
        inner.memberships.insert((admitted.user_id, admitted.server_id), admitted);
        Ok(CapAdmission::Admitted)
    }
}

#[async_trait]
impl ChannelStore for MemoryStore {
    async fn next_channel_id(&self) -> Result<ChannelId, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.next_channel += 1;
        ChannelId(compose_id(inner.node, inner.next_channel))
        })
    }
    async fn insert_channel(&self, channel: Channel) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("channels", ChangeOp::Upsert, to_payload(&channel));
        inner.channels.insert(channel.id, channel);
        })
    }
    async fn get_channel(&self, id: ChannelId) -> Result<Option<Channel>, StoreError> {
        Ok({
        self.0.lock().unwrap().channels.get(&id).cloned()
        })
    }
    async fn find_by_name(&self, server: ServerId, name: &str) -> Result<Option<Channel>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .channels
            .values()
            .find(|c| c.server_id == server && c.name == name)
            .cloned()
        })
    }
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Channel>, StoreError> {
        Ok({
        let mut v: Vec<Channel> = self
            .0
            .lock()
            .unwrap()
            .channels
            .values()
            .filter(|c| c.server_id == server)
            .cloned()
            .collect();
        v.sort_by_key(|c| c.id.0);
        v
        })
    }
    async fn remove_channel(&self, id: ChannelId) -> Result<bool, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        let Some(removed) = inner.channels.remove(&id) else {
            return Ok(false);
        };
        // A channel delete carries the whole removed row so the consumer can derive
        // its server scope; the cascade prune of its messages follows on the peer.
        inner.record("channels", ChangeOp::Delete, to_payload(&removed));
        // Prune the deleted channel's messages so nothing dangles.
        inner.messages.retain(|_, m| m.channel_id != id);
        true
        })
    }
    async fn search_by_tag(&self, tag: &str) -> Result<Vec<Channel>, StoreError> {
        let Some(needle) = domain::Tags::search_needle(tag) else {
            return Ok(Vec::new());
        };
        Ok(self.0.lock().unwrap().channels.values()
            .filter(|c| c.tags.as_str().contains(&needle))
            .cloned()
            .collect())
    }
}

#[async_trait]
impl MessageStore for MemoryStore {
    async fn next_message_id(&self) -> Result<MessageId, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.next_message += 1;
        MessageId(compose_id(inner.node, inner.next_message))
        })
    }
    async fn insert_message(&self, message: Message) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("messages", ChangeOp::Upsert, to_payload(&message));
        inner.messages.insert(message.id, message);
        })
    }
    async fn get_message(&self, id: MessageId) -> Result<Option<Message>, StoreError> {
        Ok({
        self.0.lock().unwrap().messages.get(&id).cloned()
        })
    }
    async fn update_message(&self, message: Message) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("messages", ChangeOp::Upsert, to_payload(&message));
        inner.messages.insert(message.id, message);
        })
    }
    async fn list_for_channel(&self, channel: ChannelId) -> Result<Vec<Message>, StoreError> {
        Ok({
        let mut v: Vec<Message> = self
            .0
            .lock()
            .unwrap()
            .messages
            .values()
            .filter(|m| m.channel_id == channel)
            .cloned()
            .collect();
        v.sort_by_key(|m| m.id.0);
        v
        })
    }
}

#[async_trait]
impl ReactionStore for MemoryStore {
    async fn add(&self, reaction: Reaction) -> Result<bool, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        let exists = inner.reactions.iter().any(|r| {
            r.message_id == reaction.message_id && r.user == reaction.user && r.emoji == reaction.emoji
        });
        if exists {
            return Ok(false);
        }
        inner.record("reactions", ChangeOp::Upsert, to_payload(&reaction));
        inner.reactions.push(reaction);
        true
        })
    }
    async fn remove(&self, message: MessageId, user: UserId, emoji: &str) -> Result<bool, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        let before = inner.reactions.len();
        inner
            .reactions
            .retain(|r| !(r.message_id == message && r.user == user && r.emoji == emoji));
        let removed = inner.reactions.len() != before;
        if removed {
            // The message id lets the consumer derive scope (reaction → message → server).
            inner.record(
                "reactions",
                ChangeOp::Delete,
                serde_json::json!({ "message_id": message, "user": user, "emoji": emoji }),
            );
        }
        removed
        })
    }
    async fn list_for_message(&self, message: MessageId) -> Result<Vec<Reaction>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .reactions
            .iter()
            .filter(|r| r.message_id == message)
            .cloned()
            .collect()
        })
    }
    async fn user_has_any(&self, message: MessageId, user: UserId) -> Result<bool, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .reactions
            .iter()
            .any(|r| r.message_id == message && r.user == user)
        })
    }
}

#[async_trait]
impl ProposalStore for MemoryStore {
    async fn next_proposal_id(&self) -> Result<ProposalId, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.next_proposal += 1;
        ProposalId(compose_id(inner.node, inner.next_proposal))
        })
    }
    async fn insert_proposal(&self, proposal: Proposal) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("proposals", ChangeOp::Upsert, to_payload(&proposal));
        inner.proposals.insert(proposal.id, proposal);
        })
    }
    async fn get_proposal(&self, id: ProposalId) -> Result<Option<Proposal>, StoreError> {
        Ok({
        self.0.lock().unwrap().proposals.get(&id).cloned()
        })
    }
    async fn update_proposal(&self, proposal: Proposal) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("proposals", ChangeOp::Upsert, to_payload(&proposal));
        inner.proposals.insert(proposal.id, proposal);
        })
    }
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Proposal>, StoreError> {
        Ok({
        let mut v: Vec<Proposal> = self
            .0
            .lock()
            .unwrap()
            .proposals
            .values()
            .filter(|p| p.server_id == server)
            .cloned()
            .collect();
        v.sort_by_key(|p| p.id.0);
        v
        })
    }
}

#[async_trait]
impl VoteStore for MemoryStore {
    async fn upsert_vote(&self, vote: Vote) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner
            .votes
            .retain(|v| !(v.proposal_id == vote.proposal_id && v.voter == vote.voter));
        inner.record("votes", ChangeOp::Upsert, to_payload(&vote));
        inner.votes.push(vote);
        })
    }
    async fn get_vote(&self, proposal: ProposalId, voter: UserId) -> Result<Option<Vote>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .votes
            .iter()
            .find(|v| v.proposal_id == proposal && v.voter == voter)
            .copied()
        })
    }
    async fn list_for_proposal(&self, proposal: ProposalId) -> Result<Vec<Vote>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .votes
            .iter()
            .filter(|v| v.proposal_id == proposal)
            .copied()
            .collect()
        })
    }

    async fn clear_for_proposal(&self, proposal: ProposalId) -> Result<(), StoreError> {
        self.0.lock().unwrap().votes.retain(|v| v.proposal_id != proposal);
        Ok(())
    }
}

#[async_trait]
impl EmojiStore for MemoryStore {
    async fn next_emoji_id(&self) -> Result<EmojiId, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.next_emoji += 1;
        EmojiId(compose_id(inner.node, inner.next_emoji))
        })
    }
    async fn insert_emoji(&self, emoji: Emoji) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("emojis", ChangeOp::Upsert, to_payload(&emoji));
        inner.emojis.insert(emoji.id, emoji);
        })
    }
    async fn get_emoji(&self, id: EmojiId) -> Result<Option<Emoji>, StoreError> {
        Ok({
        self.0.lock().unwrap().emojis.get(&id).cloned()
        })
    }
    async fn find_emoji(&self, server: ServerId, name: &str) -> Result<Option<Emoji>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .emojis
            .values()
            .find(|e| e.server_id == server && e.name == name)
            .cloned()
        })
    }
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Emoji>, StoreError> {
        Ok({
        let mut v: Vec<Emoji> = self
            .0
            .lock()
            .unwrap()
            .emojis
            .values()
            .filter(|e| e.server_id == server)
            .cloned()
            .collect();
        v.sort_by_key(|e| e.id.0);
        v
        })
    }
}

#[async_trait]
impl EmojiVoteStore for MemoryStore {
    async fn upsert_emoji_vote(&self, vote: EmojiVote) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner
            .emoji_votes
            .retain(|x| !(x.emoji_id == vote.emoji_id && x.voter == vote.voter));
        inner.record("emoji_votes", ChangeOp::Upsert, to_payload(&vote));
        inner.emoji_votes.push(vote);
        })
    }
    async fn emoji_votes_for_server(&self, server: ServerId) -> Result<Vec<EmojiVote>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .emoji_votes
            .iter()
            .filter(|v| v.server_id == server)
            .copied()
            .collect()
        })
    }
    async fn my_emoji_vote(&self, emoji: EmojiId, voter: UserId) -> Result<Option<bool>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .emoji_votes
            .iter()
            .find(|v| v.emoji_id == emoji && v.voter == voter)
            .map(|v| v.is_up)
        })
    }
}

#[async_trait]
impl RoleColorVoteStore for MemoryStore {
    async fn upsert_role_color_vote(&self, vote: RoleColorVote) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner
            .role_color_votes
            .retain(|x| !(x.role_id == vote.role_id && x.voter == vote.voter));
        inner.record("role_color_votes", ChangeOp::Upsert, to_payload(&vote));
        inner.role_color_votes.push(vote);
        })
    }
    async fn role_color_votes_for_server(&self, server: ServerId) -> Result<Vec<RoleColorVote>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .role_color_votes
            .iter()
            .filter(|v| v.server_id == server)
            .cloned()
            .collect()
        })
    }
    async fn my_role_color_vote(&self, role: RoleId, voter: UserId) -> Result<Option<RoleColor>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .role_color_votes
            .iter()
            .find(|v| v.role_id == role && v.voter == voter)
            .map(|v| v.color.clone())
        })
    }
}

#[async_trait]
impl RuleStore for MemoryStore {
    async fn next_rule_id(&self) -> Result<RuleId, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.next_rule += 1;
        RuleId(compose_id(inner.node, inner.next_rule))
        })
    }
    async fn insert_rule(&self, rule: Rule) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("rules", ChangeOp::Upsert, to_payload(&rule));
        inner.rules.insert(rule.id, rule);
        })
    }
    async fn remove_rule(&self, id: RuleId) -> Result<bool, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        match inner.rules.remove(&id) {
            Some(removed) => {
                inner.record("rules", ChangeOp::Delete, to_payload(&removed));
                true
            }
            None => false,
        }
        })
    }
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Rule>, StoreError> {
        Ok({
        let mut v: Vec<Rule> = self
            .0
            .lock()
            .unwrap()
            .rules
            .values()
            .filter(|r| r.server_id == server)
            .cloned()
            .collect();
        v.sort_by_key(|r| r.id.0);
        v
        })
    }
}

#[async_trait]
impl KeyDirectoryStore for MemoryStore {
    async fn put_keys(&self, keys: UserKeys) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("user_keys", ChangeOp::Upsert, to_payload(&keys));
        inner.user_keys.insert(keys.user_id, keys);
        })
    }
    async fn get_keys(&self, user: UserId) -> Result<Option<UserKeys>, StoreError> {
        Ok({
        self.0.lock().unwrap().user_keys.get(&user).cloned()
        })
    }
}

#[async_trait]
impl ChannelKeyStore for MemoryStore {
    async fn put_grant(&self, grant: ChannelKeyGrant) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("channel_grants", ChangeOp::Upsert, to_payload(&grant));
        inner.channel_grants.insert((grant.channel_id, grant.epoch, grant.member), grant);
        })
    }
    async fn grants_for_member(&self, channel: ChannelId, member: UserId) -> Result<Vec<ChannelKeyGrant>, StoreError> {
        Ok({
        let mut v: Vec<ChannelKeyGrant> = self
            .0
            .lock()
            .unwrap()
            .channel_grants
            .values()
            .filter(|g| g.channel_id == channel && g.member == member)
            .cloned()
            .collect();
        v.sort_by_key(|g| g.epoch);
        v
        })
    }
}

#[async_trait]
impl DmStore for MemoryStore {
    async fn next_dm_id(&self) -> Result<DmId, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.next_dm += 1;
        DmId(compose_id(inner.node, inner.next_dm))
        })
    }
    async fn insert_dm(&self, message: DmMessage) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("dms", ChangeOp::Upsert, to_payload(&message));
        inner.dms.push(message);
        })
    }
    async fn conversation(&self, a: UserId, b: UserId) -> Result<Vec<DmMessage>, StoreError> {
        Ok({
        let mut v: Vec<DmMessage> = self
            .0
            .lock()
            .unwrap()
            .dms
            .iter()
            .filter(|m| m.is_between(a, b))
            .cloned()
            .collect();
        v.sort_by_key(|m| m.id.0);
        v
        })
    }
    async fn partners(&self, who: UserId) -> Result<Vec<UserId>, StoreError> {
        Ok({
        // Walk newest-first, keeping each partner's first (i.e. most-recent) sighting.
        let inner = self.0.lock().unwrap();
        let mut seen = Vec::new();
        for m in inner.dms.iter().rev() {
            let other = if m.sender == who {
                Some(m.recipient)
            } else if m.recipient == who {
                Some(m.sender)
            } else {
                None
            };
            if let Some(other) = other {
                if !seen.contains(&other) {
                    seen.push(other);
                }
            }
        }
        seen
        })
    }
}

#[async_trait]
impl BlockStore for MemoryStore {
    async fn add(&self, block: Block) -> Result<bool, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        let exists = inner
            .blocks
            .iter()
            .any(|b| b.blocker == block.blocker && b.blocked == block.blocked);
        if exists {
            return Ok(false);
        }
        inner.record("blocks", ChangeOp::Upsert, to_payload(&block));
        inner.blocks.push(block);
        true
        })
    }
    async fn involving(&self, who: UserId) -> Result<Vec<Block>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .blocks
            .iter()
            .filter(|b| b.blocker == who || b.blocked == who)
            .cloned()
            .collect()
        })
    }
    async fn is_blocked_between(&self, a: UserId, b: UserId) -> Result<bool, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .blocks
            .iter()
            .any(|block| block.is_between(a, b))
        })
    }
}

impl app::MediaStore for MemoryStore {
    fn put(&self, content_type: &str, bytes: &[u8]) -> Result<String, app::MediaError> {
        let mut inner = self.0.lock().unwrap();
        inner.media_seq += 1;
        let seq = inner.media_seq;
        let nanos =
            SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let key = format!("{nanos:x}{seq:x}");
        inner.media.insert(key.clone(), (content_type.to_string(), bytes.to_vec()));
        Ok(key)
    }
    fn get(&self, key: &str) -> Option<(String, Vec<u8>)> {
        self.0.lock().unwrap().media.get(key).cloned()
    }
    fn delete(&self, key: &str) {
        self.0.lock().unwrap().media.remove(key);
    }
}

#[async_trait]
impl app::InviteStore for MemoryStore {
    async fn add(&self, invite: Invite) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("invites", ChangeOp::Upsert, to_payload(&invite));
        inner.invites.insert(invite.code_hash.clone(), invite);
        })
    }
    async fn by_hash(&self, code_hash: &str) -> Result<Option<Invite>, StoreError> {
        Ok({
        self.0.lock().unwrap().invites.get(code_hash).cloned()
        })
    }
    async fn list_for_server(&self, server_id: ServerId) -> Result<Vec<Invite>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .invites
            .values()
            .filter(|i| i.server_id == server_id)
            .cloned()
            .collect()
        })
    }
    async fn revoke(&self, code_hash: &str) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        if let Some(mut invite) = inner.invites.get(code_hash).cloned() {
            invite.is_revoked = true;
            inner.record("invites", ChangeOp::Upsert, to_payload(&invite));
            inner.invites.insert(invite.code_hash.clone(), invite);
        }
        })
    }
}

#[async_trait]
impl FriendStore for MemoryStore {
    async fn add(&self, friendship: Friendship) -> Result<bool, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        let exists = inner
            .friends
            .iter()
            .any(|f| f.is_between(friendship.requester, friendship.addressee));
        if exists {
            return Ok(false);
        }
        inner.record("friendships", ChangeOp::Upsert, to_payload(&friendship));
        inner.friends.push(friendship);
        true
        })
    }
    async fn update(&self, friendship: Friendship) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        let payload = to_payload(&friendship);
        let found = match inner
            .friends
            .iter_mut()
            .find(|f| f.is_between(friendship.requester, friendship.addressee))
        {
            Some(slot) => {
                *slot = friendship;
                true
            }
            None => false,
        };
        if found {
            inner.record("friendships", ChangeOp::Upsert, payload);
        }
        })
    }
    async fn between(&self, a: UserId, b: UserId) -> Result<Option<Friendship>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .friends
            .iter()
            .find(|f| f.is_between(a, b))
            .cloned()
        })
    }
    async fn involving(&self, who: UserId) -> Result<Vec<Friendship>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .friends
            .iter()
            .filter(|f| f.requester == who || f.addressee == who)
            .cloned()
            .collect()
        })
    }
}

#[async_trait]
impl RoleStore for MemoryStore {
    async fn next_role_id(&self) -> Result<RoleId, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.next_role += 1;
        RoleId(compose_id(inner.node, inner.next_role))
        })
    }
    async fn insert_role(&self, role: Role) -> Result<(), StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        inner.record("roles", ChangeOp::Upsert, to_payload(&role));
        inner.roles.insert(role.id, role);
        })
    }
    async fn get_role(&self, id: RoleId) -> Result<Option<Role>, StoreError> {
        Ok({
        self.0.lock().unwrap().roles.get(&id).cloned()
        })
    }
    async fn find_role(&self, server: ServerId, name: &str) -> Result<Option<Role>, StoreError> {
        Ok({
        self.0
            .lock()
            .unwrap()
            .roles
            .values()
            .find(|r| r.server_id == server && r.name == name)
            .cloned()
        })
    }
    async fn remove_role(&self, id: RoleId) -> Result<bool, StoreError> {
        Ok({
        let mut inner = self.0.lock().unwrap();
        let Some(removed) = inner.roles.remove(&id) else {
            return Ok(false);
        };
        // Carry the removed row so the consumer can derive its server scope. Role
        // membership is derived from criteria, not stored, so nothing cascades.
        inner.record("roles", ChangeOp::Delete, to_payload(&removed));
        true
        })
    }
    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Role>, StoreError> {
        Ok({
        let mut v: Vec<Role> = self
            .0
            .lock()
            .unwrap()
            .roles
            .values()
            .filter(|r| r.server_id == server)
            .cloned()
            .collect();
        v.sort_by_key(|r| r.id.0);
        v
        })
    }
}

#[async_trait]
impl ReplicationCursor for MemoryStore {
    async fn replication_cursor(&self, peer: NodeId) -> u64 {
        self.0.lock().unwrap().cursors.get(&peer).copied().unwrap_or(0)
    }

    async fn advance_cursor(&self, peer: NodeId, seq: u64) {
        let mut inner = self.0.lock().unwrap();
        let slot = inner.cursors.entry(peer).or_insert(0);
        *slot = (*slot).max(seq);
    }
}

impl MemoryStore {
    /// Apply one authorized peer change into the replica, without echoing it back
    /// into this node's outbox. The federation adapter's replicator calls this per
    /// event, then advances the peer's cursor over the applied prefix.
    pub fn apply_incoming(&self, part: &SignedPart) -> Result<(), String> {
        apply_row(&mut self.0.lock().unwrap(), part)
    }
}

#[async_trait]
impl ChangeSink for MemoryStore {
    async fn apply(&self, part: &SignedPart) -> Result<(), String> {
        self.apply_incoming(part)
    }
}

#[async_trait]
impl ChangeSource for MemoryStore {
    async fn changes_since(&self, after_seq: u64, limit: u64) -> Vec<ChangeRecord> {
        // The outbox is append-only with monotonic `seq`, so filter-then-take is
        // already ascending and bounded — exactly what a peer's cursor pull wants.
        self.0
            .lock()
            .unwrap()
            .outbox
            .iter()
            .filter(|r| r.seq > after_seq)
            .take(limit as usize)
            .cloned()
            .collect()
    }
}
