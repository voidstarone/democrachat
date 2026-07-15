//! A user's device secret key — held only by the user (client-side), never by the
//! server in the clear.

use rand::rngs::OsRng;
use rand::RngCore;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::e2ee::e2ee_error::E2eeError;
use crate::e2ee::public_identity::PublicIdentity;

/// The secret half of a user's device key (X25519). This is the key that must
/// **never leave the client in the clear** — on the server it exists only wrapped
/// under the user's password (see [`wrap`](crate::e2ee::wrap)). Held here as raw
/// bytes so it can be generated, wrapped, and (client-side) unwrapped.
pub struct IdentitySecret(pub(crate) StaticSecret);

impl IdentitySecret {
    /// Generate a fresh device key from the OS CSPRNG.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(StaticSecret::from(bytes))
    }

    /// The public half to publish to the server / hand to senders.
    pub fn public(&self) -> PublicIdentity {
        PublicIdentity(PublicKey::from(&self.0))
    }

    /// The raw 32 secret bytes — only for wrapping/serialization, never for storage
    /// in the clear.
    pub(crate) fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(StaticSecret::from(bytes))
    }

    /// The secret as 64-char hex, for the client to persist locally (e.g. in the
    /// browser's IndexedDB) or wrap. Never sent to the server in this form.
    pub fn to_hex(&self) -> String {
        hex::encode(self.to_bytes())
    }

    /// Reconstruct from a 64-char hex secret (client-side, after unwrapping).
    pub fn from_hex(hex_str: &str) -> Result<Self, E2eeError> {
        let bytes: [u8; 32] = hex::decode(hex_str.trim())
            .map_err(|e| E2eeError::BadKey(e.to_string()))?
            .try_into()
            .map_err(|_| E2eeError::BadKey("secret key must be 32 bytes".into()))?;
        Ok(Self::from_bytes(bytes))
    }
}
