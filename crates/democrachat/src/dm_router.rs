//! Adapt the federation [`WriteRouter`] to the app's [`app::DmRouter`] port.

use std::sync::Arc;

use adapter_federation::{Command, WriteRouter};
use app::DmRouter;
use async_trait::async_trait;

/// Turns a web sealed-DM into a `SendDm` command and routes it to the sender's home
/// node (local apply or forward). Installed in `AppState` only when the node is
/// federated; the single-box path never constructs one.
pub struct FederatedDmRouter(pub Arc<WriteRouter>);

#[async_trait]
impl DmRouter for FederatedDmRouter {
    async fn send_dm(
        &self,
        from_id: u64,
        to_id: u64,
        sealed_for_recipient: String,
        sealed_for_sender: String,
    ) -> Result<(), String> {
        self.0
            .submit(Command::SendDm { from: from_id, to: to_id, sealed_for_recipient, sealed_for_sender })
            .await
            .map_err(|e| e.to_string())
    }
}
