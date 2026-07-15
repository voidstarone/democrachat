//! Adapt the federation [`WriteRouter`] to the app's [`app::BlockRouter`] port.

use std::sync::Arc;

use adapter_federation::{Command, WriteRouter};
use app::BlockRouter;
use async_trait::async_trait;

/// Turns a web block into a `Block` command and routes it to **both** users' homes
/// (`WriteRouter::submit` fans a block's two scopes out to each owner and requires
/// both to commit). Installed in `AppState` only when the node is federated; the
/// single-box path never constructs one.
pub struct FederatedBlockRouter(pub Arc<WriteRouter>);

#[async_trait]
impl BlockRouter for FederatedBlockRouter {
    async fn block(&self, blocker_id: u64, blocked_id: u64) -> Result<(), String> {
        self.0
            .submit(Command::Block { blocker: blocker_id, blocked: blocked_id })
            .await
            .map_err(|e| e.to_string())
    }
}
