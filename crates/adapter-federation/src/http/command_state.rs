//! What the command endpoint needs to run a forwarded write on the owner.

use std::sync::Arc;

use domain::NodeId;
use federation::{OwnershipRegistry, ScopeResolver};

use crate::command::command_executor::CommandExecutor;
use crate::command::replay_guard::ReplayGuard;

/// Shared state for the command endpoint. `node` is this owner's identity, checked
/// against the target scope's owner so the node only runs commands that belong to it.
#[derive(Clone)]
pub struct CommandState {
    pub node: NodeId,
    pub registry: Arc<dyn OwnershipRegistry>,
    pub resolver: Arc<dyn ScopeResolver>,
    pub replay: Arc<ReplayGuard>,
    pub executor: Arc<dyn CommandExecutor>,
    /// Shared cluster bearer token; `None` disables the check (dev/local only).
    pub token: Option<String>,
}
