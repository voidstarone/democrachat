//! A node's public verifying key — safe to distribute.

use ed25519_dalek::{Signature, VerifyingKey};

use domain::NodeId;

use crate::fed_error::FedError;
use crate::from_hex::from_hex;
use crate::to_hex::to_hex;

/// A node's public verifying key — safe to distribute. Peers use it to check
/// that an event really came from the node that claims to have produced it.
#[derive(Clone)]
pub struct NodePublicKey {
    node: NodeId,
    verifying: VerifyingKey,
}

impl NodePublicKey {
    /// Build from a node id and its already-parsed verifying key.
    pub(crate) fn from_verifying(node: NodeId, verifying: VerifyingKey) -> Self {
        Self { node, verifying }
    }

    /// Parse from `node` + a hex-encoded 32-byte public key.
    pub fn from_hex(node: NodeId, hex: &str) -> Result<Self, FedError> {
        let bytes = from_hex::<32>(hex, "public key")?;
        let verifying = VerifyingKey::from_bytes(&bytes).map_err(|_| FedError::BadKey)?;
        Ok(Self { node, verifying })
    }

    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn to_hex(&self) -> String {
        to_hex(self.verifying.to_bytes().as_slice())
    }

    pub(crate) fn verify(&self, msg: &[u8], sig: &Signature) -> Result<(), FedError> {
        // `verify_strict` rejects non-canonical / small-order signatures, closing
        // the Ed25519 malleability surface (distinct valid signatures over the
        // same bytes) that the lax `verify` permits.
        self.verifying
            .verify_strict(msg, sig)
            .map_err(|_| FedError::BadSignature)
    }

    /// Verify a hex Ed25519 signature over `msg` by this node's key. The public
    /// counterpart to [`NodeKeypair::sign_hex`](crate::NodeKeypair::sign_hex),
    /// used to authenticate forwarded commands against the producer's
    /// control-plane-published key.
    pub fn verify_hex(&self, msg: &[u8], sig_hex: &str) -> Result<(), FedError> {
        let sig_bytes = from_hex::<64>(sig_hex, "signature")?;
        let sig = Signature::from_bytes(&sig_bytes);
        self.verify(msg, &sig)
    }
}
