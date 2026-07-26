//! Shared server state handed to every request.

use std::sync::Arc;

use app::{BlockRouter, DmRouter, EmailSender, FriendRouter, Services, SessionSigner, VoteRouter};
use tokio::sync::broadcast;

use crate::signal::SignalHub;

/// A live event pushed to connected clients, already JSON-encoded.
pub type Event = String;

/// Wiring the composition root supplies. Cheap to clone (all `Arc`s).
#[derive(Clone)]
pub struct AppState {
    pub services: Arc<Services>,
    /// Fan-out channel: every mutation publishes here; each WebSocket subscribes.
    pub events: broadcast::Sender<Event>,
    /// Voice signaling relay + ephemeral roster (targeted, per-connection routing).
    pub signal: Arc<SignalHub>,
    /// Signs/verifies session cookies. The request's authenticated identity comes
    /// from this — never from a client-supplied field.
    pub signer: Arc<SessionSigner>,
    /// Whether to add `Secure` to auth cookies (set behind TLS).
    pub secure_cookies: bool,
    /// Whether the dev clock-control endpoints are enabled.
    pub is_dev: bool,
    /// Advance the (dev) clock by N days — wired to the composition root's
    /// controllable clock so the browser can watch the time-based franchise rules.
    pub advance_secs: Arc<dyn Fn(i64) + Send + Sync>,
    /// Persist the store to disk — called after every successful mutation so a
    /// serve session survives a restart, matching the CLI.
    pub save: Arc<dyn Fn() + Send + Sync>,
    /// When federated, routes a vote to the node owning the proposal's server
    /// (local apply or forward). `None` on the single-box deployment, where votes
    /// apply directly through `Services`.
    pub vote_router: Option<Arc<dyn VoteRouter>>,
    /// When federated, routes a sealed DM to the node owning the **sender's** home
    /// (local apply or forward). `None` on the single-box deployment, where DMs
    /// apply directly through `Services`.
    pub dm_router: Option<Arc<dyn DmRouter>>,
    /// When federated, commits a block to **both** users' homes synchronously (a
    /// block must take effect immediately in both directions). `None` on the
    /// single-box deployment, where a block applies directly through `Services`.
    pub block_router: Option<Arc<dyn BlockRouter>>,
    /// When federated, commits a friend request/acceptance to **both** users' homes
    /// synchronously (a friendship is a two-user record and gates friends-only DMs on
    /// the sender's home). `None` on single-box, where it applies through `Services`.
    pub friend_router: Option<Arc<dyn FriendRouter>>,
    /// Delivers signup verification emails. `None` when email verification is off
    /// (no sender configured); the composition root fails closed if verification is
    /// required but this is unset.
    pub email: Option<Arc<dyn EmailSender>>,
    /// The public base URL (from `DEMOCRACHAT_BASE_URL`, e.g. `https://chat.example.com`),
    /// used to build the verification link `{base_url}/verify?token=…`.
    pub base_url: String,
}

impl AppState {
    /// Broadcast an event to all connected clients. Ignores the "no subscribers"
    /// error — a send with nobody listening is fine.
    pub fn publish(&self, event: Event) {
        let _ = self.events.send(event);
    }

    /// Persist the store after a mutation.
    pub fn persist(&self) {
        (self.save)();
    }
}
