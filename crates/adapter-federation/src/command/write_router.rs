//! Route a write to the node that owns its scope: apply it locally when this node
//! is the owner, otherwise forward it as a signed command to the owner.

use std::collections::HashMap;
use std::sync::Arc;

use domain::NodeId;
use federation::{NodeKeypair, OwnershipRegistry, ScopeResolver};

use crate::command::command::Command;
use crate::command::command_executor::CommandExecutor;
use crate::command::forward_error::ForwardError;
use crate::command::signed_command::SignedCommand;
use crate::command::target_scope::target_scopes;
use crate::http::command_client::CommandClient;

/// The caller side of command forwarding. A write use-case (e.g. the web vote
/// handler) hands its [`Command`] here instead of touching the store directly; the
/// router resolves the scope's owner from the control plane and either runs the
/// command locally or forwards it, so the same call works whether or not this node
/// happens to own the target scope.
pub struct WriteRouter {
    node: NodeId,
    registry: Arc<dyn OwnershipRegistry>,
    resolver: Arc<dyn ScopeResolver>,
    keypair: Arc<NodeKeypair>,
    /// Applies a command locally when this node owns the scope — the same executor
    /// the command endpoint uses, so a local and a forwarded write take one path.
    local: Arc<dyn CommandExecutor>,
    /// Owner node → its command endpoint client.
    peers: HashMap<NodeId, CommandClient>,
}

impl WriteRouter {
    pub fn new(
        node: NodeId,
        registry: Arc<dyn OwnershipRegistry>,
        resolver: Arc<dyn ScopeResolver>,
        keypair: Arc<NodeKeypair>,
        local: Arc<dyn CommandExecutor>,
        peers: HashMap<NodeId, CommandClient>,
    ) -> Self {
        Self { node, registry, resolver, keypair, local, peers }
    }

    /// Submit `command` to **every** node that owns one of its scopes: apply locally
    /// where this node is an owner, forward the (single, shared) signed command to
    /// each distinct remote owner. Most commands name one scope and this is the
    /// familiar apply-or-forward; a block names two (both users' homes) and commits
    /// synchronously on both.
    ///
    /// All owners must succeed — the first failure is returned and the caller may
    /// re-drive (the block/DM use-cases are idempotent, so a partial commit is safe
    /// to retry). `Unowned` if any scope currently has no owner (so a full commit is
    /// impossible right now); `OwnerUnreachable` if an owner is known but unroutable.
    pub async fn submit(&self, command: Command) -> Result<(), ForwardError> {
        let scopes = target_scopes(&command, self.resolver.as_ref()).await;
        if scopes.is_empty() {
            return Err(ForwardError::Unowned);
        }

        // Resolve each scope to its owner node, collapsing duplicates: two scopes on
        // the same node (e.g. both users homed together) must be committed once, not
        // twice — a second apply of the same signed command would trip anti-replay.
        let mut owners: Vec<NodeId> = Vec::new();
        for scope in scopes {
            let owner = self
                .registry
                .owner_of(scope)
                .await
                .map_err(|e| ForwardError::OwnerUnreachable(e.0))?
                .ok_or(ForwardError::Unowned)?;
            if !owners.contains(&owner.owner) {
                owners.push(owner.owner);
            }
        }

        // Sign once; the same signed command is admitted independently by each owner
        // (each keeps its own nonce log, so one nonce is not a cross-node replay).
        let signed = SignedCommand::sign(&self.keypair, &command);
        for owner in owners {
            if owner == self.node {
                self.local.execute(&command).await?;
            } else {
                let client = self.peers.get(&owner).ok_or_else(|| {
                    ForwardError::OwnerUnreachable(format!("no route to owner node {}", owner.0))
                })?;
                client.forward(&signed).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    use federation::{InMemoryRegistry, OwnedScope};

    struct FixedServer;
    #[async_trait]
    impl ScopeResolver for FixedServer {
        async fn proposal_server(&self, _: u64) -> Option<u64> {
            Some(7)
        }
        async fn message_server(&self, _: u64) -> Option<u64> {
            None
        }
    }

    #[derive(Default)]
    struct RecordingLocal(Mutex<Vec<Command>>);
    #[async_trait]
    impl CommandExecutor for RecordingLocal {
        async fn execute(&self, command: &Command) -> Result<(), ForwardError> {
            self.0.lock().unwrap().push(command.clone());
            Ok(())
        }
    }

    fn vote() -> Command {
        Command::CastVote { proposal: 1, voter: 2, aye: true }
    }

    fn block() -> Command {
        Command::Block { blocker: 100, blocked: 200 }
    }

    #[tokio::test]
    async fn a_block_owned_wholly_by_this_node_is_applied_once() {
        // Both users are homed here, so the block's two scopes collapse to one owner
        // (us) — it must be applied exactly once, not twice (a second apply of the
        // same command would trip anti-replay on a remote owner).
        let me = NodeId(1);
        let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
        reg.claim(OwnedScope::UserHome(100), me).await.unwrap();
        reg.claim(OwnedScope::UserHome(200), me).await.unwrap();

        let local = Arc::new(RecordingLocal::default());
        let router = WriteRouter::new(
            me,
            reg,
            Arc::new(FixedServer),
            Arc::new(NodeKeypair::generate(me)),
            local.clone(),
            HashMap::new(),
        );

        router.submit(block()).await.expect("applied locally");
        assert_eq!(local.0.lock().unwrap().clone(), vec![block()], "applied exactly once");
    }

    #[tokio::test]
    async fn a_block_with_one_remote_home_and_no_route_fails() {
        // The blocker is homed here, the blocked user on node 2 with no route. A block
        // can't half-commit, so a missing route to either home fails the whole submit.
        let me = NodeId(1);
        let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
        reg.claim(OwnedScope::UserHome(100), me).await.unwrap();
        reg.claim(OwnedScope::UserHome(200), NodeId(2)).await.unwrap();

        let local = Arc::new(RecordingLocal::default());
        let router = WriteRouter::new(
            me,
            reg,
            Arc::new(FixedServer),
            Arc::new(NodeKeypair::generate(me)),
            local.clone(),
            HashMap::new(), // no route to node 2
        );

        let err = router.submit(block()).await.unwrap_err();
        assert!(matches!(err, ForwardError::OwnerUnreachable(_)), "got {err}");
    }

    #[tokio::test]
    async fn a_block_with_an_unowned_home_is_unowned() {
        // Nobody currently owns the blocked user's home → a full commit is impossible.
        let me = NodeId(1);
        let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
        reg.claim(OwnedScope::UserHome(100), me).await.unwrap();
        // UserHome(200) unclaimed.
        let router = WriteRouter::new(
            me,
            reg,
            Arc::new(FixedServer),
            Arc::new(NodeKeypair::generate(me)),
            Arc::new(RecordingLocal::default()),
            HashMap::new(),
        );
        assert!(matches!(router.submit(block()).await, Err(ForwardError::Unowned)));
    }

    #[tokio::test]
    async fn a_write_to_a_locally_owned_scope_is_applied_locally() {
        let me = NodeId(1);
        let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
        reg.claim(OwnedScope::Server(7), me).await.unwrap();

        let local = Arc::new(RecordingLocal::default());
        let router = WriteRouter::new(
            me,
            reg,
            Arc::new(FixedServer),
            Arc::new(NodeKeypair::generate(me)),
            local.clone(),
            HashMap::new(), // no peers needed — we own it
        );

        router.submit(vote()).await.expect("applied locally");
        assert_eq!(local.0.lock().unwrap().clone(), vec![vote()]);
    }

    #[tokio::test]
    async fn a_write_to_a_remote_scope_with_no_route_is_owner_unreachable() {
        let me = NodeId(1);
        let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
        // Server(7) is owned by node 2, and we have no peer client for it.
        reg.claim(OwnedScope::Server(7), NodeId(2)).await.unwrap();

        let local = Arc::new(RecordingLocal::default());
        let router = WriteRouter::new(
            me,
            reg,
            Arc::new(FixedServer),
            Arc::new(NodeKeypair::generate(me)),
            local.clone(),
            HashMap::new(),
        );

        let err = router.submit(vote()).await.unwrap_err();
        assert!(matches!(err, ForwardError::OwnerUnreachable(_)));
        assert!(local.0.lock().unwrap().is_empty(), "never applied locally when not the owner");
    }

    #[tokio::test]
    async fn a_write_to_an_unowned_scope_is_unowned() {
        let me = NodeId(1);
        let reg: Arc<dyn OwnershipRegistry> = Arc::new(InMemoryRegistry::new());
        // Nobody claimed Server(7).
        let router = WriteRouter::new(
            me,
            reg,
            Arc::new(FixedServer),
            Arc::new(NodeKeypair::generate(me)),
            Arc::new(RecordingLocal::default()),
            HashMap::new(),
        );
        assert!(matches!(router.submit(vote()).await, Err(ForwardError::Unowned)));
    }
}
