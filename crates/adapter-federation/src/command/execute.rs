//! The owner-side command pipeline: authenticate → owner-owns-scope → anti-replay
//! → run the use-case.

use domain::NodeId;
use federation::{OwnershipRegistry, ScopeResolver};

use crate::command::command_executor::CommandExecutor;
use crate::command::forward_error::ForwardError;
use crate::command::replay_guard::ReplayGuard;
use crate::command::signed_command::SignedCommand;
use crate::command::target_scope::target_scopes;
use crate::command::verify_signed::verify_signed;

/// Run a forwarded command on the owner. In order:
///
/// 1. **Authenticate** — [`verify_signed`] against the forwarding node's published
///    key (a mere bearer-token holder cannot inject a write).
/// 2. **Owner-owns-scope** — this node (`node`) must be the current owner of the
///    command's target scope; otherwise the write does not belong here (a
///    misrouted command, or one aimed at a scope that has rehomed away). This is
///    the command-path mirror of the feed's payload-derived authorization.
/// 3. **Anti-replay** — [`ReplayGuard::admit`] enforces freshness and rejects a
///    repeated `(node, nonce)`, checked *after* authenticity + ownership so a
///    misrouted command never burns a nonce.
/// 4. **Apply** — the [`CommandExecutor`] runs the domain use-case, which does its
///    own eligibility checks and mints the canonical change event.
#[allow(clippy::too_many_arguments)]
pub async fn execute(
    node: NodeId,
    registry: &dyn OwnershipRegistry,
    resolver: &dyn ScopeResolver,
    replay: &ReplayGuard,
    executor: &dyn CommandExecutor,
    signed: &SignedCommand,
    now: i64,
) -> Result<(), ForwardError> {
    let cmd = verify_signed(registry, signed).await?;

    // A command may name more than one scope (a block names both users' homes). This
    // node runs it if it owns *any* of them — the caller forwards the same command to
    // every distinct owner, so each owner commits its own share. If it owns none, the
    // command was misrouted (or its scope rehomed away) and must not be applied here.
    let scopes = target_scopes(&cmd, resolver).await;
    if scopes.is_empty() {
        return Err(ForwardError::Unowned);
    }
    let mut owns_a_scope = false;
    for scope in scopes {
        match registry.owner_of(scope).await.map_err(|e| ForwardError::OwnerUnreachable(e.0))? {
            Some(o) if o.owner == node => owns_a_scope = true,
            _ => {}
        }
    }
    if !owns_a_scope {
        return Err(ForwardError::Rejected("this node owns none of the command's scopes".into()));
    }

    replay.admit(signed.node, &signed.nonce, signed.issued_at, now).await?;
    executor.execute(&cmd).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    use crate::command::command::Command;
    use federation::{InMemoryRegistry, NodeKeypair, OwnedScope, OwnershipRegistry};

    /// Maps every proposal to server 7.
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

    /// Records the commands it was asked to apply.
    #[derive(Default)]
    struct RecordingExecutor(Mutex<Vec<Command>>);
    #[async_trait]
    impl CommandExecutor for RecordingExecutor {
        async fn execute(&self, command: &Command) -> Result<(), ForwardError> {
            self.0.lock().unwrap().push(command.clone());
            Ok(())
        }
    }

    fn vote() -> Command {
        Command::CastVote { proposal: 1, voter: 2, aye: true }
    }

    async fn owned_by(owner: NodeId) -> (InMemoryRegistry, NodeKeypair) {
        // The forwarding node (B) is keyed; the owner (A) holds Server(7).
        let reg = InMemoryRegistry::new();
        let forwarder = NodeKeypair::generate(NodeId(9));
        reg.publish_key(NodeId(9), &forwarder.public().to_hex()).await.unwrap();
        reg.claim(OwnedScope::Server(7), owner).await.unwrap();
        (reg, forwarder)
    }

    #[tokio::test]
    async fn the_owner_runs_a_valid_forwarded_command() {
        let me = NodeId(1);
        let (reg, forwarder) = owned_by(me).await;
        let exec = RecordingExecutor::default();
        let signed = SignedCommand::sign_at(&forwarder, &vote(), 1_000, "n1".into());

        execute(me, &reg, &FixedServer, &ReplayGuard::in_memory(), &exec, &signed, 1_000)
            .await
            .expect("applies");
        assert_eq!(exec.0.lock().unwrap().clone(), vec![vote()]);
    }

    #[tokio::test]
    async fn a_command_for_a_scope_this_node_does_not_own_is_refused() {
        let me = NodeId(1);
        // Server(7) is owned by node 2, not us.
        let (reg, forwarder) = owned_by(NodeId(2)).await;
        let exec = RecordingExecutor::default();
        let signed = SignedCommand::sign_at(&forwarder, &vote(), 1_000, "n1".into());

        let err = execute(me, &reg, &FixedServer, &ReplayGuard::in_memory(), &exec, &signed, 1_000)
            .await
            .unwrap_err();
        assert!(matches!(err, ForwardError::Rejected(_)));
        assert!(exec.0.lock().unwrap().is_empty(), "a non-owner never runs the use-case");
    }

    #[tokio::test]
    async fn the_same_command_cannot_be_replayed() {
        let me = NodeId(1);
        let (reg, forwarder) = owned_by(me).await;
        let exec = RecordingExecutor::default();
        let guard = ReplayGuard::in_memory();
        let signed = SignedCommand::sign_at(&forwarder, &vote(), 1_000, "n1".into());

        execute(me, &reg, &FixedServer, &guard, &exec, &signed, 1_000).await.expect("first applies");
        let err = execute(me, &reg, &FixedServer, &guard, &exec, &signed, 1_001).await.unwrap_err();
        assert!(matches!(err, ForwardError::Rejected(_)), "replay refused");
        assert_eq!(exec.0.lock().unwrap().len(), 1, "applied exactly once");
    }

    #[tokio::test]
    async fn a_misrouted_command_does_not_burn_its_nonce() {
        // Ownership is checked before the replay guard, so a command that arrives at
        // the wrong node can still be legitimately delivered to the right one.
        let (reg, forwarder) = owned_by(NodeId(2)).await; // owner is node 2
        let exec = RecordingExecutor::default();
        let guard = ReplayGuard::in_memory();
        let signed = SignedCommand::sign_at(&forwarder, &vote(), 1_000, "n1".into());

        // Delivered to the wrong node (1): refused without recording the nonce.
        let _ = execute(NodeId(1), &reg, &FixedServer, &guard, &exec, &signed, 1_000).await;
        // The nonce is still fresh, so the real owner (2) admits and applies it.
        execute(NodeId(2), &reg, &FixedServer, &guard, &exec, &signed, 1_000)
            .await
            .expect("the rightful owner applies it");
        assert_eq!(exec.0.lock().unwrap().len(), 1);
    }
}
