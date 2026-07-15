//! Adapt the federation [`WriteRouter`] to the app's [`app::FriendRouter`] port.

use std::sync::Arc;

use adapter_federation::{Command, WriteRouter};
use app::FriendRouter;
use async_trait::async_trait;

/// Turns a web friend request/acceptance into a command and routes it to **both**
/// users' homes (`WriteRouter::submit` fans a friendship's two scopes out to each
/// owner and requires both to commit). Installed in `AppState` only when the node is
/// federated; the single-box path never constructs one.
pub struct FederatedFriendRouter(pub Arc<WriteRouter>);

#[async_trait]
impl FriendRouter for FederatedFriendRouter {
    async fn request(&self, requester_id: u64, addressee_id: u64) -> Result<(), String> {
        self.0
            .submit(Command::RequestFriend { requester: requester_id, addressee: addressee_id })
            .await
            .map_err(|e| e.to_string())
    }

    async fn accept(&self, accepter_id: u64, requester_id: u64) -> Result<(), String> {
        self.0
            .submit(Command::AcceptFriend { accepter: accepter_id, requester: requester_id })
            .await
            .map_err(|e| e.to_string())
    }
}
