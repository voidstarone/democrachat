//! Bind the transport-level [`CommandExecutor`] to the domain use-cases.
//!
//! When this node owns a scope and receives a forwarded write for it, this is what
//! actually runs it — through `app::Services`, so the owner does its own
//! validation (citizenship, proposal phase) and mints the canonical change event.
//! The event then replicates back to the forwarder (and everyone else) through the
//! ordinary feed, so the originating user sees their write land.

use std::sync::Arc;

use adapter_federation::{Command, CommandExecutor, ForwardError};
use app::Services;
use async_trait::async_trait;

/// Runs forwarded commands by invoking `app::Services`, persisting after each so a
/// forwarded write is as durable as a local one.
pub struct ServiceCommandExecutor {
    services: Arc<Services>,
    save: Arc<dyn Fn() + Send + Sync>,
}

impl ServiceCommandExecutor {
    pub fn new(services: Arc<Services>, save: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { services, save }
    }
}

#[async_trait]
impl CommandExecutor for ServiceCommandExecutor {
    async fn execute(&self, command: &Command) -> Result<(), ForwardError> {
        match command {
            Command::CastVote { proposal, voter, aye } => {
                // The owner re-checks eligibility here — it never trusts the
                // forwarder's say-so about who may vote.
                self.services.governance()
                    .cast_vote_by_id(*voter, *proposal, *aye)
                    .map_err(|e| ForwardError::Rejected(e.to_string()))?;
            }
            Command::SendDm { from, to, sealed_for_recipient, sealed_for_sender } => {
                // The owner (the sender's home) re-checks the can_dm gate itself.
                self.services.social()
                    .send_sealed_dm_by_id(*from, *to, sealed_for_recipient, sealed_for_sender)
                    .map_err(|e| ForwardError::Rejected(e.to_string()))?;
            }
            Command::Block { blocker, blocked } => {
                // Each of the two homes runs this; the block is idempotent, so the
                // two commits (and any re-drive of a partial one) converge.
                self.services.social()
                    .block_user_by_id(*blocker, *blocked)
                    .map_err(|e| ForwardError::Rejected(e.to_string()))?;
            }
            Command::RequestFriend { requester, addressee } => {
                // Runs on both users' homes; idempotent re-affirm converges.
                self.services.social()
                    .request_friend_by_id(*requester, *addressee)
                    .map_err(|e| ForwardError::Rejected(e.to_string()))?;
            }
            Command::AcceptFriend { accepter, requester } => {
                self.services.social()
                    .accept_friend_by_id(*accepter, *requester)
                    .map_err(|e| ForwardError::Rejected(e.to_string()))?;
            }
        }
        (self.save)();
        Ok(())
    }
}
