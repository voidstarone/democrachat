//! Adapt the federation [`WriteRouter`] to the app's [`app::VoteRouter`] port.

use std::sync::Arc;

use adapter_federation::{Command, WriteRouter};
use app::VoteRouter;
use async_trait::async_trait;

/// Turns a web vote into a `CastVote` command and routes it to the node owning the
/// proposal's server (local apply or forward). Installed in `AppState` only when
/// the node is federated; the single-box path never constructs one.
pub struct FederatedVoteRouter(pub Arc<WriteRouter>);

#[async_trait]
impl VoteRouter for FederatedVoteRouter {
    async fn cast_vote(&self, voter_id: u64, proposal_id: u64, is_aye: bool) -> Result<(), String> {
        self.0
            .submit(Command::CastVote { proposal: proposal_id, voter: voter_id, aye: is_aye })
            .await
            .map_err(|e| e.to_string())
    }
}
