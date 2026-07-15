//! A symmetric channel key — the key an encrypted channel's message bodies are
//! sealed under, and which is itself sealed to each member's device key in a grant.

use rand::rngs::OsRng;
use rand::RngCore;

use crate::e2ee::e2ee_error::E2eeError;

/// A 32-byte symmetric key shared by the members of an encrypted channel. Message
/// bodies are sealed under it ([`seal_channel_message`](crate::seal_channel_message));
/// the key itself is handed to each member by sealing it to their device public key
/// (a [`ChannelKeyGrant`](domain::ChannelKeyGrant)). Generated and distributed
/// **client-side** — the server never holds it.
#[derive(Clone)]
pub struct ChannelKey([u8; 32]);

impl ChannelKey {
    /// Mint a fresh channel key from the OS CSPRNG (e.g. at channel creation, or on
    /// a ratchet in an `Ephemeral` channel).
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// The raw 32 key bytes — for sealing into a grant, or after recovering one.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// Reconstruct a key from the 32 bytes recovered by opening a grant.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(hex_str: &str) -> Result<Self, E2eeError> {
        let bytes: [u8; 32] = hex::decode(hex_str.trim())
            .map_err(|e| E2eeError::BadKey(e.to_string()))?
            .try_into()
            .map_err(|_| E2eeError::BadKey("channel key must be 32 bytes".into()))?;
        Ok(Self(bytes))
    }
}
