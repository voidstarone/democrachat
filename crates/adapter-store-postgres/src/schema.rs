//! The whole schema as one idempotent DDL script, run at boot.
//!
//! Every aggregate is stored as a JSONB `data` blob keyed by its id, with only the
//! columns needed for lookups and scans lifted out (handle, slug, server_id, …) and
//! kept in sync from the domain struct on write. Postgres is the source of truth;
//! JSONB is just the column type over the same serde model the dev JSON snapshot
//! uses, so a row can never drift from the domain shape. Id minting uses one
//! sequence per entity — atomic and concurrency-safe, unlike the in-memory counter.

/// Idempotent (`IF NOT EXISTS`) so it is safe to run on every start.
pub const DDL: &str = r#"
CREATE SEQUENCE IF NOT EXISTS user_id_seq;
CREATE SEQUENCE IF NOT EXISTS server_id_seq;
CREATE SEQUENCE IF NOT EXISTS channel_id_seq;
CREATE SEQUENCE IF NOT EXISTS message_id_seq;
CREATE SEQUENCE IF NOT EXISTS proposal_id_seq;
CREATE SEQUENCE IF NOT EXISTS emoji_id_seq;
CREATE SEQUENCE IF NOT EXISTS rule_id_seq;
CREATE SEQUENCE IF NOT EXISTS role_id_seq;
CREATE SEQUENCE IF NOT EXISTS dm_id_seq;

CREATE TABLE IF NOT EXISTS users (
  id BIGINT PRIMARY KEY,
  handle TEXT NOT NULL UNIQUE,
  data JSONB NOT NULL
);

CREATE TABLE IF NOT EXISTS servers (
  id BIGINT PRIMARY KEY,
  slug TEXT NOT NULL UNIQUE,
  data JSONB NOT NULL
);

CREATE TABLE IF NOT EXISTS memberships (
  user_id BIGINT NOT NULL,
  server_id BIGINT NOT NULL,
  tier TEXT NOT NULL,
  enfranchised_at BIGINT,
  data JSONB NOT NULL,
  PRIMARY KEY (user_id, server_id)
);
CREATE INDEX IF NOT EXISTS memberships_server ON memberships (server_id);

CREATE TABLE IF NOT EXISTS channels (
  id BIGINT PRIMARY KEY,
  server_id BIGINT NOT NULL,
  name TEXT NOT NULL,
  data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS channels_server ON channels (server_id);
CREATE UNIQUE INDEX IF NOT EXISTS channels_server_name ON channels (server_id, name);

CREATE TABLE IF NOT EXISTS messages (
  id BIGINT PRIMARY KEY,
  channel_id BIGINT NOT NULL,
  data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_channel ON messages (channel_id);

CREATE TABLE IF NOT EXISTS reactions (
  message_id BIGINT NOT NULL,
  user_id BIGINT NOT NULL,
  emoji TEXT NOT NULL,
  data JSONB NOT NULL,
  PRIMARY KEY (message_id, user_id, emoji)
);

CREATE TABLE IF NOT EXISTS proposals (
  id BIGINT PRIMARY KEY,
  server_id BIGINT NOT NULL,
  data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS proposals_server ON proposals (server_id);

CREATE TABLE IF NOT EXISTS votes (
  proposal_id BIGINT NOT NULL,
  voter_id BIGINT NOT NULL,
  data JSONB NOT NULL,
  PRIMARY KEY (proposal_id, voter_id)
);

CREATE TABLE IF NOT EXISTS emojis (
  id BIGINT PRIMARY KEY,
  server_id BIGINT NOT NULL,
  name TEXT NOT NULL,
  data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS emojis_server ON emojis (server_id);
CREATE UNIQUE INDEX IF NOT EXISTS emojis_server_name ON emojis (server_id, name);

CREATE TABLE IF NOT EXISTS emoji_votes (
  emoji_id BIGINT NOT NULL,
  voter_id BIGINT NOT NULL,
  server_id BIGINT NOT NULL,
  data JSONB NOT NULL,
  PRIMARY KEY (emoji_id, voter_id)
);
CREATE INDEX IF NOT EXISTS emoji_votes_server ON emoji_votes (server_id);

CREATE TABLE IF NOT EXISTS rules (
  id BIGINT PRIMARY KEY,
  server_id BIGINT NOT NULL,
  data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS rules_server ON rules (server_id);

CREATE TABLE IF NOT EXISTS dms (
  id BIGINT PRIMARY KEY,
  sender_id BIGINT NOT NULL,
  recipient_id BIGINT NOT NULL,
  data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS dms_sender ON dms (sender_id);
CREATE INDEX IF NOT EXISTS dms_recipient ON dms (recipient_id);

CREATE TABLE IF NOT EXISTS blocks (
  blocker_id BIGINT NOT NULL,
  blocked_id BIGINT NOT NULL,
  data JSONB NOT NULL,
  PRIMARY KEY (blocker_id, blocked_id)
);
CREATE INDEX IF NOT EXISTS blocks_blocked ON blocks (blocked_id);

CREATE TABLE IF NOT EXISTS friendships (
  requester_id BIGINT NOT NULL,
  addressee_id BIGINT NOT NULL,
  data JSONB NOT NULL,
  PRIMARY KEY (requester_id, addressee_id)
);
CREATE INDEX IF NOT EXISTS friendships_addressee ON friendships (addressee_id);

CREATE TABLE IF NOT EXISTS roles (
  id BIGINT PRIMARY KEY,
  server_id BIGINT NOT NULL,
  name TEXT NOT NULL,
  data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS roles_server ON roles (server_id);
CREATE UNIQUE INDEX IF NOT EXISTS roles_server_name ON roles (server_id, name);

CREATE TABLE IF NOT EXISTS role_assignments (
  role_id BIGINT NOT NULL,
  user_id BIGINT NOT NULL,
  data JSONB NOT NULL,
  PRIMARY KEY (role_id, user_id)
);

CREATE TABLE IF NOT EXISTS role_color_votes (
  role_id BIGINT NOT NULL,
  voter_id BIGINT NOT NULL,
  server_id BIGINT NOT NULL,
  data JSONB NOT NULL,
  PRIMARY KEY (role_id, voter_id)
);
CREATE INDEX IF NOT EXISTS role_color_votes_server ON role_color_votes (server_id);

CREATE TABLE IF NOT EXISTS user_keys (
  user_id BIGINT PRIMARY KEY,
  data JSONB NOT NULL
);

CREATE TABLE IF NOT EXISTS channel_key_grants (
  channel_id BIGINT NOT NULL,
  member_id BIGINT NOT NULL,
  epoch BIGINT NOT NULL,
  data JSONB NOT NULL,
  PRIMARY KEY (channel_id, member_id, epoch)
);

CREATE TABLE IF NOT EXISTS invites (
  code_hash TEXT PRIMARY KEY,
  server_id BIGINT NOT NULL,
  data JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS invites_server ON invites (server_id);

-- Federation change-capture outbox: every mutation is appended here in the SAME
-- transaction as the row it changed, so the feed can never diverge from the data.
-- `seq` (BIGSERIAL) is the peer's monotonic replay cursor; the scope is not stored
-- (the consumer derives it from the payload, exactly as the producer does).
CREATE TABLE IF NOT EXISTS outbox (
  seq BIGSERIAL PRIMARY KEY,
  entity TEXT NOT NULL,
  op TEXT NOT NULL,
  payload JSONB NOT NULL
);

-- Per-peer replication cursor: the highest feed seq applied from each peer, so a
-- pull resumes where it left off (and survives a restart, unlike an in-RAM map).
CREATE TABLE IF NOT EXISTS replication_cursors (
  peer BIGINT PRIMARY KEY,
  seq BIGINT NOT NULL
);

-- Durable anti-replay log for forwarded commands: a remembered (node, nonce) can't
-- be replayed against this owner, even across a restart.
CREATE TABLE IF NOT EXISTS fed_nonces (
  node BIGINT NOT NULL,
  nonce TEXT NOT NULL,
  expiry_at BIGINT NOT NULL,
  PRIMARY KEY (node, nonce)
);
"#;
