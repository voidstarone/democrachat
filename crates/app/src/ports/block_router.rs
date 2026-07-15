//! Driven port: commit a permanent block to **both** users' homes synchronously.

use async_trait::async_trait;

/// Where the web block handler sends a block when the deployment is federated. A
/// block is safety-critical and silences DMs in both directions, and the DM gate
/// runs on the *sender's* home — so the block must be committed on **both** the
/// blocker's and the blocked user's homes before it can be trusted, rather than
/// left to the eventual feed.
///
/// The single-box deployment leaves this **unset** and blocks apply locally through
/// [`Services::block_user`](crate::Services::block_user); a federated deployment
/// supplies a router that fans the block out to both homes and reports success only
/// when both have committed it. The operation is idempotent, so the web layer may
/// re-drive a partial commit. Both parties are identified by home-stable user id.
#[async_trait]
pub trait BlockRouter: Send + Sync {
    async fn block(&self, blocker_id: u64, blocked_id: u64) -> Result<(), String>;
}
