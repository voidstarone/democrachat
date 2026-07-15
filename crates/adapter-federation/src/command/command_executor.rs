use async_trait::async_trait;

use crate::command::command::Command;
use crate::command::forward_error::ForwardError;

/// Applies a verified, owner-authorized command by running the domain use-case —
/// e.g. casting the vote through the governance service, which re-checks the
/// voter's citizenship and the proposal's phase and mints the canonical `votes`
/// change event that then replicates fleet-wide. Implemented in the composition
/// root, where `app::Services` is available; the adapter stays decoupled from it.
#[async_trait]
pub trait CommandExecutor: Send + Sync {
    async fn execute(&self, command: &Command) -> Result<(), ForwardError>;
}
