//! The per-node key-encryption key (KEK) that wraps each snapshot's data key.

use crate::vault::vault_error::VaultError;

/// A node's **key-encryption key** — the root secret unwrapped at boot from a
/// KMS/secret env var and held only in RAM. It never encrypts data directly;
/// [`seal`](crate::vault::seal::seal) generates a fresh per-snapshot data key and
/// wraps it under this, so the root key can be rotated without re-encrypting every
/// snapshot (envelope encryption, per `docs/federation.md` §5b).
#[derive(Clone)]
pub struct VaultKey([u8; 32]);

impl VaultKey {
    /// Load a 32-byte key from a 64-char hex string (e.g. `DEMOCRACHAT_DATA_KEK`).
    pub fn from_hex(hex_str: &str) -> Result<Self, VaultError> {
        let bytes = hex::decode(hex_str.trim())
            .map_err(|e| VaultError::BadKey(e.to_string()))?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| VaultError::BadKey("key must be exactly 32 bytes (64 hex chars)".into()))?;
        Ok(Self(arr))
    }

    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
