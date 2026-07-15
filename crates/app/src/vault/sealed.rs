//! The on-disk envelope: a sealed snapshot and everything needed to open it (but
//! the key).

use serde::{Deserialize, Serialize};

use crate::vault::vault_error::VaultError;

/// A sealed blob as written to disk. Self-describing so a reader can tell it apart
/// from a legacy plaintext snapshot (which has none of these fields): the data key
/// is wrapped under the node's [`VaultKey`](crate::vault::vault_key::VaultKey), the
/// data itself is encrypted under that per-snapshot key, and each layer carries its
/// own nonce. All binary fields are hex.
#[derive(Serialize, Deserialize)]
pub struct Sealed {
    /// Envelope format version, so the on-disk format can evolve.
    pub vault: u8,
    /// The per-snapshot data key, encrypted under the node's key.
    pub wrapped_key: String,
    /// Nonce used to wrap the data key.
    pub key_nonce: String,
    /// Nonce used to encrypt the payload.
    pub nonce: String,
    /// The snapshot ciphertext (AEAD over the plaintext JSON).
    pub ciphertext: String,
}

/// The current envelope version.
pub const VAULT_VERSION: u8 = 1;

impl Sealed {
    pub fn to_json(&self) -> String {
        // A fixed, small struct of strings — serialization cannot fail.
        serde_json::to_string(self).expect("Sealed serializes")
    }

    /// Parse a sealed envelope. Returns `Err(Format)` if the bytes are not a
    /// sealed envelope at all (e.g. a legacy plaintext snapshot), which the caller
    /// uses to fall back to reading plaintext.
    pub fn from_json(s: &str) -> Result<Self, VaultError> {
        let sealed: Sealed =
            serde_json::from_str(s).map_err(|e| VaultError::Format(e.to_string()))?;
        if sealed.vault != VAULT_VERSION {
            return Err(VaultError::Format(format!(
                "unsupported vault version {}",
                sealed.vault
            )));
        }
        Ok(sealed)
    }
}
