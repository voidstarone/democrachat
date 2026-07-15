//! Driven port: route a sealed DM to the node that homes its sender.

use async_trait::async_trait;

/// Where the web DM handler sends a sealed direct message when the deployment is
/// federated. The single-box deployment leaves this **unset** and DMs apply locally
/// through [`Services::send_sealed_dm`](crate::Services::send_sealed_dm); a
/// federated deployment supplies a router that either applies locally (this node
/// homes the sender) or forwards a signed command to the sender's home — the node
/// that authoritatively owns the sender's DMs.
///
/// The two ciphertexts are opaque; the server never sees the body. Both parties are
/// identified by user id (already resolved from the authenticated session). Any
/// failure collapses to a message the web layer surfaces as a `400`.
#[async_trait]
pub trait DmRouter: Send + Sync {
    async fn send_dm(
        &self,
        from_id: u64,
        to_id: u64,
        sealed_for_recipient: String,
        sealed_for_sender: String,
    ) -> Result<(), String>;
}
