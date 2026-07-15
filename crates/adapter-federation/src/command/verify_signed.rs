use domain::NodeId;
use federation::OwnershipRegistry;

use crate::command::command::Command;
use crate::command::forward_error::ForwardError;
use crate::command::signed_command::SignedCommand;
use crate::command::signing_payload::signing_payload;

/// Verify a [`SignedCommand`] against the producing node's control-plane-published
/// key and return the parsed [`Command`]. Fails if the node is unknown, the
/// signature does not verify, or the body is malformed. This checks *authenticity*
/// only — freshness and replay are enforced separately by
/// [`ReplayGuard`](crate::command::replay_guard::ReplayGuard), which needs the
/// owner's clock.
pub async fn verify_signed(
    registry: &dyn OwnershipRegistry,
    signed: &SignedCommand,
) -> Result<Command, ForwardError> {
    let key = registry
        .public_key(NodeId(signed.node))
        .await
        .map_err(|e| ForwardError::OwnerUnreachable(e.0))?
        .ok_or_else(|| ForwardError::Rejected(format!("unknown forwarding node {}", signed.node)))?;
    let payload = signing_payload(signed.node, signed.issued_at, &signed.nonce, &signed.body);
    key.verify_hex(payload.as_bytes(), &signed.signature)
        .map_err(|_| ForwardError::Rejected("bad command signature".into()))?;
    signed
        .command()
        .ok_or_else(|| ForwardError::Rejected("malformed command body".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use federation::{InMemoryRegistry, NodeKeypair};

    fn sample() -> Command {
        Command::CastVote { proposal: 1, voter: 2, aye: true }
    }

    #[tokio::test]
    async fn a_node_signed_command_verifies_and_returns_the_intent() {
        let reg = InMemoryRegistry::new();
        let kp = NodeKeypair::generate(NodeId(5));
        reg.publish_key(NodeId(5), &kp.public().to_hex()).await.unwrap();
        let signed = SignedCommand::sign(&kp, &sample());
        assert_eq!(signed.node, 5, "sign() stamps the keypair's node");
        assert_eq!(verify_signed(&reg, &signed).await.expect("verifies"), sample());
    }

    #[tokio::test]
    async fn a_command_from_an_unknown_node_is_rejected() {
        let reg = InMemoryRegistry::new();
        let kp = NodeKeypair::generate(NodeId(5)); // key never published
        let signed = SignedCommand::sign(&kp, &sample());
        assert!(matches!(verify_signed(&reg, &signed).await, Err(ForwardError::Rejected(_))));
    }

    #[tokio::test]
    async fn a_tampered_body_fails_verification() {
        let reg = InMemoryRegistry::new();
        let kp = NodeKeypair::generate(NodeId(5));
        reg.publish_key(NodeId(5), &kp.public().to_hex()).await.unwrap();
        let mut signed = SignedCommand::sign(&kp, &sample());
        // The headline attack: rewrite the acting voter after signing.
        signed.body = signed.body.replace("\"voter\":2", "\"voter\":9");
        assert!(matches!(verify_signed(&reg, &signed).await, Err(ForwardError::Rejected(_))));
    }

    #[tokio::test]
    async fn tampering_with_replay_metadata_breaks_the_signature() {
        // issued_at and nonce are covered by the signature, so an attacker can't
        // freshen a captured command by rewriting them to slip past freshness.
        let reg = InMemoryRegistry::new();
        let kp = NodeKeypair::generate(NodeId(5));
        reg.publish_key(NodeId(5), &kp.public().to_hex()).await.unwrap();
        let base = SignedCommand::sign_at(&kp, &sample(), 1_000, "nonce-a".into());

        let mut future = base.clone();
        future.issued_at = 9_999;
        assert!(matches!(verify_signed(&reg, &future).await, Err(ForwardError::Rejected(_))));

        let mut renonced = base.clone();
        renonced.nonce = "nonce-b".into();
        assert!(matches!(verify_signed(&reg, &renonced).await, Err(ForwardError::Rejected(_))));
    }

    #[tokio::test]
    async fn a_forger_claiming_a_trusted_node_id_is_rejected() {
        // An attacker signs with its own key but stamps a victim node id; the
        // registry's published key for that id is the victim's, so it won't verify.
        let reg = InMemoryRegistry::new();
        let victim = NodeKeypair::generate(NodeId(5));
        reg.publish_key(NodeId(5), &victim.public().to_hex()).await.unwrap();
        let attacker = NodeKeypair::generate(NodeId(5)); // same claimed id, different key
        let signed = SignedCommand::sign(&attacker, &sample());
        assert!(matches!(verify_signed(&reg, &signed).await, Err(ForwardError::Rejected(_))));
    }
}
