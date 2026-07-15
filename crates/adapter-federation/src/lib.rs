//! The federation **transport**: node-to-node HTTP that carries a node's signed
//! change feed to its peers, and applies a peer's feed only after it is
//! authenticated and authorized.
//!
//! This is the thin, network-facing layer over the security-critical logic in the
//! `federation` crate. The feed *server* ([`serve_feed`]) hands
//! [`federation::sign_feed`] output to peers; the *puller* ([`spawn_puller`])
//! feeds what it pulls into [`Replicator::ingest`], which is the single choke
//! point where an untrusted peer's bytes cross into the local replica — every
//! event passes [`federation::authorize`] first. Keeping the crypto/authorization
//! in `federation` and only the wire here means the security boundary has one home
//! and stays testable without any network.
//!
//! The [`Replicator`] adds the transport's own concern on top of `authorize`: an
//! **ordered per-peer cursor** with a transient-vs-permanent rejection policy (see
//! [`is_transient`]), so a not-yet-owned scope is retried while a superseded or
//! junk event is skipped past rather than stalling the whole feed.

pub mod command;
pub mod is_transient;
pub mod replicator;
pub mod store_resolver;

pub mod http;

pub use is_transient::is_transient;
pub use replicator::Replicator;
pub use store_resolver::StoreResolver;

pub use command::command::Command;
pub use command::command_executor::CommandExecutor;
pub use command::execute::execute;
pub use command::forward_error::ForwardError;
pub use command::in_memory_nonce_log::InMemoryNonceLog;
pub use command::nonce_log::NonceLog;
pub use command::replay_guard::ReplayGuard;
pub use command::signed_command::SignedCommand;
pub use command::verify_signed::verify_signed;
pub use command::write_router::WriteRouter;

pub use http::bearer_ok::bearer_ok;
pub use http::command_client::CommandClient;
pub use http::command_router::command_router;
pub use http::command_state::CommandState;
pub use http::feed_client::FeedClient;
pub use http::feed_router::feed_router;
pub use http::feed_state::FeedState;
pub use http::peer::Peer;
pub use http::poll_peer::poll_peer;
pub use http::serve_federation::serve_federation;
pub use http::serve_feed::serve_feed;
pub use http::spawn_puller::spawn_puller;
