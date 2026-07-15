//! One module per store port; each holds a single `impl <Port> for PgStore`,
//! following the workspace's one-definition-per-file convention.

mod block;
mod channel;
mod channel_key;
mod dm;
mod emoji;
mod emoji_vote;
mod friend;
mod invite;
mod key_directory;
mod membership;
mod message;
mod proposal;
mod reaction;
mod role;
mod role_color_vote;
mod rule;
mod server;
mod user;
mod vote;
