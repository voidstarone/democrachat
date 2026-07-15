//! What the feed server needs to answer a peer's pull.

use std::sync::Arc;

use federation::{ChangeSource, NodeKeypair, OwnershipRegistry, ScopeResolver};

/// Shared state for the feed endpoint. Everything is behind an `Arc` so the axum
/// handler can clone it per request cheaply.
///
/// The feed is produced from `store` purely through the `ChangeSource` port, so any
/// backend (in-memory or Postgres) can serve it.
#[derive(Clone)]
pub struct FeedState {
    pub store: Arc<dyn ChangeSource>,
    pub keypair: Arc<NodeKeypair>,
    pub registry: Arc<dyn OwnershipRegistry>,
    pub resolver: Arc<dyn ScopeResolver>,
    /// Shared cluster bearer token; `None` disables the check (dev/local only).
    pub token: Option<String>,
}
