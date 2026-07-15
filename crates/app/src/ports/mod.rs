//! The port traits the adapters implement — one trait per file. `app` depends
//! only on these and on `domain`; it never names a database, a file, or a clock
//! source. A driven adapter (memory store, Postgres, …) provides them; the
//! composition root wires them together.
//!
//! Ports are synchronous for M0/M1 — the CLI is a synchronous driver and the
//! domain is pure. A future async web adapter can introduce async ports without
//! the domain caring.

pub mod block_router;
pub mod block_store;
pub mod channel_key_store;
pub mod channel_store;
pub mod clock;
pub mod dm_router;
pub mod dm_store;
pub mod emoji_store;
pub mod emoji_vote_store;
pub mod friend_router;
pub mod friend_store;
pub mod image_transcoder;
pub mod invite_store;
pub mod key_directory_store;
pub mod media_store;
pub mod membership_store;
pub mod message_store;
pub mod proposal_store;
pub mod reaction_store;
pub mod role_color_vote_store;
pub mod role_store;
pub mod rule_store;
pub mod server_store;
pub mod user_store;
pub mod vote_router;
pub mod vote_store;
