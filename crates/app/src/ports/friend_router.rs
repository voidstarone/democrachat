//! Driven port: commit a friend request / acceptance to **both** users' homes.

use async_trait::async_trait;

/// Where the web friend handlers send a request or acceptance when the deployment is
/// federated. A friendship is a two-user record — the addressee's home shows the
/// incoming request, the requester's the outgoing one, and once accepted the
/// friends-only DM gate (which runs on the *sender's* home) reads it there — so both
/// writes must land on both homes, not wait for the eventual feed.
///
/// The single-box deployment leaves this **unset** and friend writes apply locally
/// through [`Services`](crate::Services). Both parties are identified by home-stable
/// user id; the operations are idempotent, so a partial commit is safe to re-drive.
#[async_trait]
pub trait FriendRouter: Send + Sync {
    /// Send (or re-affirm) a friend request from `requester_id` to `addressee_id`.
    async fn request(&self, requester_id: u64, addressee_id: u64) -> Result<(), String>;
    /// Accept the pending request that `requester_id` sent to `accepter_id`.
    async fn accept(&self, accepter_id: u64, requester_id: u64) -> Result<(), String>;
}
