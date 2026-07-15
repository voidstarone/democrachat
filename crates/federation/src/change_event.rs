//! A change event on the wire.

use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};

use domain::NodeId;

use crate::fed_error::FedError;
use crate::from_hex::from_hex;
use crate::node_keypair::NodeKeypair;
use crate::node_public_key::NodePublicKey;
use crate::signed_part::SignedPart;
use crate::to_hex::to_hex;

/// A change event on the wire.
///
/// The signed part travels as its **canonical JSON text** (`body`) — the exact
/// bytes that were signed — kept verbatim rather than reconstructed. A consumer
/// verifies the signature against the bytes it *actually received*, never against
/// a re-serialization of a parsed value. That closes a subtle footgun: relying on
/// `serde_json` to reproduce byte-identical output across nodes (different crate
/// versions, number formatting, or the `preserve_order` feature) would otherwise
/// silently break verification fleet-wide. Here, verification depends only on the
/// bytes on the wire and the key — nothing else.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ChangeEvent {
    /// Canonical JSON of the [`SignedPart`] — the exact signed bytes.
    body: String,
    /// Hex-encoded Ed25519 signature over `body.as_bytes()`.
    signature: String,
}

impl ChangeEvent {
    /// Build and sign an event with `keypair`. The keypair's node is stamped into
    /// the part so the producing node can never be misdeclared.
    pub fn sign(keypair: &NodeKeypair, mut part: SignedPart) -> Self {
        part.node = keypair.node().0;
        let body = serde_json::to_string(&part).expect("SignedPart serializes");
        let sig = keypair.sign(body.as_bytes());
        Self {
            body,
            signature: to_hex(&sig.to_bytes()),
        }
    }

    /// Reconstruct an event from its wire fields (e.g. received over HTTP, or in
    /// a test). No validation happens here — call [`verify`](Self::verify).
    pub fn from_wire(body: String, signature: String) -> Self {
        Self { body, signature }
    }

    /// The canonical signed bytes, verbatim.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The hex signature.
    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// Verify this event was signed by `key`, returning the parsed signed part
    /// **only** after the signature over the received bytes checks out.
    ///
    /// The caller is still responsible for the *authorization* checks a signature
    /// cannot make: that `key` belongs to the rightful owner of `part.scope` at
    /// `part.epoch`, and that `part.seq` is past the consumer's cursor.
    pub fn verify(&self, key: &NodePublicKey) -> Result<SignedPart, FedError> {
        let sig_bytes = from_hex::<64>(&self.signature, "signature")?;
        let sig = Signature::from_bytes(&sig_bytes);
        // Verify against the exact bytes received — not a re-serialization.
        key.verify(self.body.as_bytes(), &sig)?;
        let part: SignedPart = serde_json::from_str(&self.body).map_err(|_| FedError::BadBody)?;
        if key.node() != NodeId(part.node) {
            return Err(FedError::BadSignature);
        }
        Ok(part)
    }

    /// Read the signed part **without** verifying — for routing/telemetry only
    /// (e.g. to learn which scope an event belongs to). Never trust its contents
    /// for an authorization decision.
    pub fn peek(&self) -> Result<SignedPart, FedError> {
        serde_json::from_str(&self.body).map_err(|_| FedError::BadBody)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change_op::ChangeOp;
    use crate::event_scope::EventScope;

    fn part() -> SignedPart {
        SignedPart {
            node: 0, // overwritten by sign()
            epoch: 3,
            seq: 42,
            scope: EventScope::Server(7),
            entity: "messages".into(),
            op: ChangeOp::Upsert,
            payload: serde_json::json!({ "id": 123, "body": "hello" }),
        }
    }

    #[test]
    fn a_signed_event_verifies_and_carries_the_signing_node() {
        let kp = NodeKeypair::generate(NodeId(9));
        let ev = ChangeEvent::sign(&kp, part());
        assert_eq!(ev.peek().unwrap().node, 9, "sign() stamps the keypair's node");
        let verified = ev.verify(&kp.public()).expect("verifies");
        assert_eq!(verified.seq, 42);
        assert_eq!(verified.scope, EventScope::Server(7));
    }

    #[test]
    fn a_tampered_payload_is_rejected() {
        let kp = NodeKeypair::generate(NodeId(9));
        let ev = ChangeEvent::sign(&kp, part());
        // Attacker rewrites the row in the signed body after signing.
        let forged = ChangeEvent::from_wire(
            ev.body().replace("hello", "HIJACKED"),
            ev.signature().to_string(),
        );
        assert_eq!(forged.verify(&kp.public()), Err(FedError::BadSignature));
    }

    #[test]
    fn tampering_with_any_signed_field_is_rejected() {
        let kp = NodeKeypair::generate(NodeId(9));
        // Each mutation rewrites one signed field; all must fail. Includes the
        // scope (replaying a server-7 event against server-8) and the epoch
        // (a returning stale owner) — the fencing guarantees.
        for (from, to) in [
            ("\"epoch\":3", "\"epoch\":999"),
            ("\"seq\":42", "\"seq\":1"),
            ("\"id\":7", "\"id\":8"), // scope: Server(7) -> Server(8)
            ("\"entity\":\"messages\"", "\"entity\":\"votes\""),
        ] {
            let ev = ChangeEvent::sign(&kp, part());
            let body = ev.body().replace(from, to);
            assert_ne!(body, ev.body(), "the tamper actually changed the body ({from})");
            let forged = ChangeEvent::from_wire(body, ev.signature().to_string());
            assert_eq!(
                forged.verify(&kp.public()),
                Err(FedError::BadSignature),
                "tampering {from} must fail"
            );
        }
    }

    #[test]
    fn verification_uses_transmitted_bytes_not_a_reserialization() {
        // Any byte change to the body invalidates the signature — even trailing
        // whitespace — proving verification is byte-exact, not semantic.
        let kp = NodeKeypair::generate(NodeId(9));
        let ev = ChangeEvent::sign(&kp, part());
        let with_space =
            ChangeEvent::from_wire(format!("{} ", ev.body()), ev.signature().to_string());
        assert_eq!(with_space.verify(&kp.public()), Err(FedError::BadSignature));
        assert!(ev.verify(&kp.public()).is_ok());
    }

    #[test]
    fn another_nodes_key_does_not_verify() {
        let owner = NodeKeypair::generate(NodeId(9));
        let attacker = NodeKeypair::generate(NodeId(9)); // same claimed node, different key
        let ev = ChangeEvent::sign(&owner, part());
        assert_eq!(ev.verify(&attacker.public()), Err(FedError::BadSignature));
    }

    #[test]
    fn a_forged_signature_is_rejected() {
        let kp = NodeKeypair::generate(NodeId(9));
        let ev = ChangeEvent::sign(&kp, part());
        let forged = ChangeEvent::from_wire(ev.body().to_string(), to_hex(&[0u8; 64]));
        assert_eq!(forged.verify(&kp.public()), Err(FedError::BadSignature));
    }

    #[test]
    fn a_malformed_body_is_rejected_not_panicked() {
        let kp = NodeKeypair::generate(NodeId(9));
        let ev = ChangeEvent::from_wire("not json".into(), to_hex(&[0u8; 64]));
        assert!(matches!(
            ev.verify(&kp.public()),
            Err(FedError::BadSignature) | Err(FedError::BadBody)
        ));
        assert_eq!(ev.peek(), Err(FedError::BadBody));
    }

    #[test]
    fn an_e2ee_ciphertext_payload_is_carried_opaquely() {
        // The envelope signs/transports an opaque ciphertext blob without reading
        // it — the property M7's server-blind DMs/message-bodies rely on.
        let kp = NodeKeypair::generate(NodeId(4));
        let mut p = part();
        p.entity = "dms".into();
        p.payload = serde_json::json!({ "ciphertext": "9f3ac1…", "nonce": "aa00" });
        let ev = ChangeEvent::sign(&kp, p);
        let verified = ev.verify(&kp.public()).expect("verifies opaque payload");
        assert_eq!(verified.payload["ciphertext"], "9f3ac1…");
    }
}
