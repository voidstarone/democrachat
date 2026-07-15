//! The producer side of federation replication: the Postgres store as a
//! [`ChangeSource`]. Its outbox table is written transactionally with every
//! mutation (see [`push_outbox`](crate::push_outbox)), so the feed a peer pulls is
//! always exactly the writes that committed — no gaps, no phantom rows.

use async_trait::async_trait;
use domain::{
    Block, Channel, ChannelKeyGrant, DmMessage, Emoji, EmojiVote, Friendship, Invite, Membership,
    Message, Proposal, Reaction, Role, RoleAssignment, RoleColorVote, Rule, Server, User, UserKeys,
    Vote,
};
use federation::{ChangeOp, ChangeRecord, ChangeSink, ChangeSource, SignedPart};
use serde::de::DeserializeOwned;
use sqlx::Row;

use crate::{to_json, PgStore};

#[async_trait]
impl ChangeSource for PgStore {
    async fn changes_since(&self, after_seq: u64, limit: u64) -> Vec<ChangeRecord> {
        let rows = sqlx::query(
            "SELECT seq, entity, op, payload FROM outbox WHERE seq > $1 ORDER BY seq LIMIT $2",
        )
        .bind(after_seq as i64)
        .bind(limit as i64)
        .fetch_all(self.pool())
        .await
        .unwrap_or_default();

        rows.iter()
            .filter_map(|r| {
                let seq: i64 = r.try_get("seq").ok()?;
                let entity: String = r.try_get("entity").ok()?;
                let op: String = r.try_get("op").ok()?;
                let payload: serde_json::Value = r.try_get("payload").ok()?;
                let op = match op.as_str() {
                    "upsert" => ChangeOp::Upsert,
                    "delete" => ChangeOp::Delete,
                    _ => return None,
                };
                Some(ChangeRecord { seq: seq as u64, entity, op, payload })
            })
            .collect()
    }
}

/// Deserialize a signed event's payload into its domain type.
fn from_part<T: DeserializeOwned>(part: &SignedPart) -> Result<T, String> {
    serde_json::from_value(part.payload.clone()).map_err(|e| e.to_string())
}

#[async_trait]
impl ChangeSink for PgStore {
    /// Apply a **verified** peer event into this node's replica. Writes go straight
    /// to the tables and are deliberately NOT recorded in the outbox — a replicated
    /// row must never echo back onto this node's feed, or it would loop the network
    /// forever. Every write is idempotent (`ON CONFLICT`), so a re-applied event is a
    /// no-op. The entity routing mirrors the memory store's `apply_row` exactly, so a
    /// Postgres node and an in-memory node replicate each other identically.
    async fn apply(&self, part: &SignedPart) -> Result<(), String> {
        let pool = self.pool();
        let err = |e: sqlx::Error| e.to_string();

        match (part.entity.as_str(), part.op) {
            ("users", ChangeOp::Upsert) => {
                let u: User = from_part(part)?;
                sqlx::query(
                    "INSERT INTO users (id, handle, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (id) DO UPDATE SET handle = EXCLUDED.handle, data = EXCLUDED.data",
                )
                .bind(u.id.0 as i64).bind(&u.handle).bind(to_json(&u))
                .execute(pool).await.map_err(err)?;
            }
            ("users", ChangeOp::Delete) => {
                let u: User = from_part(part)?;
                sqlx::query("DELETE FROM users WHERE id = $1").bind(u.id.0 as i64)
                    .execute(pool).await.map_err(err)?;
            }
            ("servers", ChangeOp::Upsert) => {
                let s: Server = from_part(part)?;
                sqlx::query(
                    "INSERT INTO servers (id, slug, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (id) DO UPDATE SET slug = EXCLUDED.slug, data = EXCLUDED.data",
                )
                .bind(s.id.0 as i64).bind(&s.slug).bind(to_json(&s))
                .execute(pool).await.map_err(err)?;
            }
            ("servers", ChangeOp::Delete) => {
                let s: Server = from_part(part)?;
                sqlx::query("DELETE FROM servers WHERE id = $1").bind(s.id.0 as i64)
                    .execute(pool).await.map_err(err)?;
            }
            ("invites", ChangeOp::Upsert) => {
                let i: Invite = from_part(part)?;
                sqlx::query(
                    "INSERT INTO invites (code_hash, server_id, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (code_hash) DO UPDATE SET server_id = EXCLUDED.server_id, data = EXCLUDED.data",
                )
                .bind(&i.code_hash).bind(i.server_id.0 as i64).bind(to_json(&i))
                .execute(pool).await.map_err(err)?;
            }
            ("memberships", ChangeOp::Upsert) => {
                let m: Membership = from_part(part)?;
                sqlx::query(
                    "INSERT INTO memberships (user_id, server_id, tier, enfranchised_at, data) \
                     VALUES ($1, $2, $3, $4, $5) \
                     ON CONFLICT (user_id, server_id) DO UPDATE \
                     SET tier = EXCLUDED.tier, enfranchised_at = EXCLUDED.enfranchised_at, data = EXCLUDED.data",
                )
                .bind(m.user_id.0 as i64).bind(m.server_id.0 as i64)
                .bind(format!("{:?}", m.tier)).bind(m.enfranchised_at.map(|t| t.0)).bind(to_json(&m))
                .execute(pool).await.map_err(err)?;
            }
            ("memberships", ChangeOp::Delete) => {
                let m: Membership = from_part(part)?;
                sqlx::query("DELETE FROM memberships WHERE user_id = $1 AND server_id = $2")
                    .bind(m.user_id.0 as i64).bind(m.server_id.0 as i64)
                    .execute(pool).await.map_err(err)?;
            }
            ("channels", ChangeOp::Upsert) => {
                let c: Channel = from_part(part)?;
                sqlx::query(
                    "INSERT INTO channels (id, server_id, name, data) VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (id) DO UPDATE SET server_id = EXCLUDED.server_id, name = EXCLUDED.name, data = EXCLUDED.data",
                )
                .bind(c.id.0 as i64).bind(c.server_id.0 as i64).bind(&c.name).bind(to_json(&c))
                .execute(pool).await.map_err(err)?;
            }
            ("channels", ChangeOp::Delete) => {
                let c: Channel = from_part(part)?;
                // Cascade the channel's messages, mirroring the memory store.
                sqlx::query("DELETE FROM messages WHERE channel_id = $1").bind(c.id.0 as i64)
                    .execute(pool).await.map_err(err)?;
                sqlx::query("DELETE FROM channels WHERE id = $1").bind(c.id.0 as i64)
                    .execute(pool).await.map_err(err)?;
            }
            ("messages", ChangeOp::Upsert) => {
                let m: Message = from_part(part)?;
                sqlx::query(
                    "INSERT INTO messages (id, channel_id, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (id) DO UPDATE SET channel_id = EXCLUDED.channel_id, data = EXCLUDED.data",
                )
                .bind(m.id.0 as i64).bind(m.channel_id.0 as i64).bind(to_json(&m))
                .execute(pool).await.map_err(err)?;
            }
            ("messages", ChangeOp::Delete) => {
                let m: Message = from_part(part)?;
                sqlx::query("DELETE FROM messages WHERE id = $1").bind(m.id.0 as i64)
                    .execute(pool).await.map_err(err)?;
            }
            ("reactions", ChangeOp::Upsert) => {
                let r: Reaction = from_part(part)?;
                sqlx::query(
                    "INSERT INTO reactions (message_id, user_id, emoji, data) VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (message_id, user_id, emoji) DO NOTHING",
                )
                .bind(r.message_id.0 as i64).bind(r.user.0 as i64).bind(&r.emoji).bind(to_json(&r))
                .execute(pool).await.map_err(err)?;
            }
            ("reactions", ChangeOp::Delete) => {
                let r: Reaction = from_part(part)?;
                sqlx::query("DELETE FROM reactions WHERE message_id = $1 AND user_id = $2 AND emoji = $3")
                    .bind(r.message_id.0 as i64).bind(r.user.0 as i64).bind(&r.emoji)
                    .execute(pool).await.map_err(err)?;
            }
            ("proposals", ChangeOp::Upsert) => {
                let p: Proposal = from_part(part)?;
                sqlx::query(
                    "INSERT INTO proposals (id, server_id, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (id) DO UPDATE SET server_id = EXCLUDED.server_id, data = EXCLUDED.data",
                )
                .bind(p.id.0 as i64).bind(p.server_id.0 as i64).bind(to_json(&p))
                .execute(pool).await.map_err(err)?;
            }
            ("proposals", ChangeOp::Delete) => {
                let p: Proposal = from_part(part)?;
                sqlx::query("DELETE FROM proposals WHERE id = $1").bind(p.id.0 as i64)
                    .execute(pool).await.map_err(err)?;
            }
            ("votes", ChangeOp::Upsert) => {
                let v: Vote = from_part(part)?;
                sqlx::query(
                    "INSERT INTO votes (proposal_id, voter_id, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (proposal_id, voter_id) DO UPDATE SET data = EXCLUDED.data",
                )
                .bind(v.proposal_id.0 as i64).bind(v.voter.0 as i64).bind(to_json(&v))
                .execute(pool).await.map_err(err)?;
            }
            ("emojis", ChangeOp::Upsert) => {
                let e: Emoji = from_part(part)?;
                sqlx::query(
                    "INSERT INTO emojis (id, server_id, name, data) VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (id) DO UPDATE SET server_id = EXCLUDED.server_id, name = EXCLUDED.name, data = EXCLUDED.data",
                )
                .bind(e.id.0 as i64).bind(e.server_id.0 as i64).bind(&e.name).bind(to_json(&e))
                .execute(pool).await.map_err(err)?;
            }
            ("emoji_votes", ChangeOp::Upsert) => {
                let v: EmojiVote = from_part(part)?;
                sqlx::query(
                    "INSERT INTO emoji_votes (emoji_id, voter_id, server_id, data) VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (emoji_id, voter_id) DO UPDATE SET server_id = EXCLUDED.server_id, data = EXCLUDED.data",
                )
                .bind(v.emoji_id.0 as i64).bind(v.voter.0 as i64).bind(v.server_id.0 as i64).bind(to_json(&v))
                .execute(pool).await.map_err(err)?;
            }
            ("rules", ChangeOp::Upsert) => {
                let r: Rule = from_part(part)?;
                sqlx::query(
                    "INSERT INTO rules (id, server_id, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (id) DO UPDATE SET server_id = EXCLUDED.server_id, data = EXCLUDED.data",
                )
                .bind(r.id.0 as i64).bind(r.server_id.0 as i64).bind(to_json(&r))
                .execute(pool).await.map_err(err)?;
            }
            ("rules", ChangeOp::Delete) => {
                let r: Rule = from_part(part)?;
                sqlx::query("DELETE FROM rules WHERE id = $1").bind(r.id.0 as i64)
                    .execute(pool).await.map_err(err)?;
            }
            ("dms", ChangeOp::Upsert) => {
                let m: DmMessage = from_part(part)?;
                sqlx::query(
                    "INSERT INTO dms (id, sender_id, recipient_id, data) VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (id) DO NOTHING",
                )
                .bind(m.id.0 as i64).bind(m.sender.0 as i64).bind(m.recipient.0 as i64).bind(to_json(&m))
                .execute(pool).await.map_err(err)?;
            }
            ("blocks", ChangeOp::Upsert) => {
                let b: Block = from_part(part)?;
                sqlx::query(
                    "INSERT INTO blocks (blocker_id, blocked_id, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (blocker_id, blocked_id) DO NOTHING",
                )
                .bind(b.blocker.0 as i64).bind(b.blocked.0 as i64).bind(to_json(&b))
                .execute(pool).await.map_err(err)?;
            }
            ("friendships", ChangeOp::Upsert) => {
                let f: Friendship = from_part(part)?;
                sqlx::query(
                    "INSERT INTO friendships (requester_id, addressee_id, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (requester_id, addressee_id) DO UPDATE SET data = EXCLUDED.data",
                )
                .bind(f.requester.0 as i64).bind(f.addressee.0 as i64).bind(to_json(&f))
                .execute(pool).await.map_err(err)?;
            }
            ("user_keys", ChangeOp::Upsert) => {
                let k: UserKeys = from_part(part)?;
                sqlx::query(
                    "INSERT INTO user_keys (user_id, data) VALUES ($1, $2) \
                     ON CONFLICT (user_id) DO UPDATE SET data = EXCLUDED.data",
                )
                .bind(k.user_id.0 as i64).bind(to_json(&k))
                .execute(pool).await.map_err(err)?;
            }
            ("channel_grants", ChangeOp::Upsert) => {
                let g: ChannelKeyGrant = from_part(part)?;
                sqlx::query(
                    "INSERT INTO channel_key_grants (channel_id, member_id, epoch, data) VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (channel_id, member_id, epoch) DO UPDATE SET data = EXCLUDED.data",
                )
                .bind(g.channel_id.0 as i64).bind(g.member.0 as i64).bind(g.epoch as i64).bind(to_json(&g))
                .execute(pool).await.map_err(err)?;
            }
            ("roles", ChangeOp::Upsert) => {
                let r: Role = from_part(part)?;
                sqlx::query(
                    "INSERT INTO roles (id, server_id, name, data) VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (id) DO UPDATE SET server_id = EXCLUDED.server_id, name = EXCLUDED.name, data = EXCLUDED.data",
                )
                .bind(r.id.0 as i64).bind(r.server_id.0 as i64).bind(&r.name).bind(to_json(&r))
                .execute(pool).await.map_err(err)?;
            }
            ("roles", ChangeOp::Delete) => {
                let r: Role = from_part(part)?;
                sqlx::query("DELETE FROM role_assignments WHERE role_id = $1").bind(r.id.0 as i64)
                    .execute(pool).await.map_err(err)?;
                sqlx::query("DELETE FROM roles WHERE id = $1").bind(r.id.0 as i64)
                    .execute(pool).await.map_err(err)?;
            }
            ("role_assignments", ChangeOp::Upsert) => {
                let a: RoleAssignment = from_part(part)?;
                sqlx::query(
                    "INSERT INTO role_assignments (role_id, user_id, data) VALUES ($1, $2, $3) \
                     ON CONFLICT (role_id, user_id) DO NOTHING",
                )
                .bind(a.role_id.0 as i64).bind(a.user.0 as i64).bind(to_json(&a))
                .execute(pool).await.map_err(err)?;
            }
            ("role_assignments", ChangeOp::Delete) => {
                let a: RoleAssignment = from_part(part)?;
                sqlx::query("DELETE FROM role_assignments WHERE role_id = $1 AND user_id = $2")
                    .bind(a.role_id.0 as i64).bind(a.user.0 as i64)
                    .execute(pool).await.map_err(err)?;
            }
            ("role_color_votes", ChangeOp::Upsert) => {
                let v: RoleColorVote = from_part(part)?;
                sqlx::query(
                    "INSERT INTO role_color_votes (role_id, voter_id, server_id, data) VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (role_id, voter_id) DO UPDATE SET server_id = EXCLUDED.server_id, data = EXCLUDED.data",
                )
                .bind(v.role_id.0 as i64).bind(v.voter.0 as i64).bind(v.server_id.0 as i64).bind(to_json(&v))
                .execute(pool).await.map_err(err)?;
            }
            (other, _) => return Err(format!("unknown replicated entity `{other}`")),
        }
        Ok(())
    }
}

#[async_trait]
impl federation::ReplicationCursor for PgStore {
    async fn replication_cursor(&self, peer: domain::NodeId) -> u64 {
        let seq: Option<i64> = sqlx::query("SELECT seq FROM replication_cursors WHERE peer = $1")
            .bind(peer.0 as i64)
            .fetch_optional(self.pool())
            .await
            .ok()
            .flatten()
            .and_then(|r| r.try_get("seq").ok());
        seq.unwrap_or(0) as u64
    }

    async fn advance_cursor(&self, peer: domain::NodeId, seq: u64) {
        // Monotonic: GREATEST keeps a reordered/duplicate pull from moving it back.
        let _ = sqlx::query(
            "INSERT INTO replication_cursors (peer, seq) VALUES ($1, $2) \
             ON CONFLICT (peer) DO UPDATE SET seq = GREATEST(replication_cursors.seq, EXCLUDED.seq)",
        )
        .bind(peer.0 as i64)
        .bind(seq as i64)
        .execute(self.pool())
        .await;
    }
}

impl PgStore {
    /// Record a forwarded command's `(node, nonce)`, returning `true` if it was
    /// newly seen. Durable in `fed_nonces`, so a captured command can't be replayed
    /// after a restart. Expired entries are swept on each call.
    pub async fn remember_nonce(&self, node: u16, nonce: &str, now: i64, expiry_at: i64) -> bool {
        let _ = sqlx::query("DELETE FROM fed_nonces WHERE expiry_at <= $1")
            .bind(now)
            .execute(self.pool())
            .await;
        sqlx::query(
            "INSERT INTO fed_nonces (node, nonce, expiry_at) VALUES ($1, $2, $3) \
             ON CONFLICT (node, nonce) DO NOTHING",
        )
        .bind(node as i64)
        .bind(nonce)
        .bind(expiry_at)
        .execute(self.pool())
        .await
        .map(|r| r.rows_affected() > 0)
        .unwrap_or(false)
    }
}
