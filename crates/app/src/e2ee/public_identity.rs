//! A user's public device key — what a sender seals a message to.

use x25519_dalek::PublicKey;

use crate::e2ee::e2ee_error::E2eeError;

/// The public half of a user's device key (X25519). It is **not secret**: the
/// server stores it and hands it to anyone who wants to seal a DM or a message-key
/// to this user. Sealing to it requires no interaction with the user.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PublicIdentity(pub(crate) PublicKey);

impl PublicIdentity {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.as_bytes())
    }

    pub fn from_hex(hex_str: &str) -> Result<Self, E2eeError> {
        let bytes: [u8; 32] = hex::decode(hex_str.trim())
            .map_err(|e| E2eeError::BadKey(e.to_string()))?
            .try_into()
            .map_err(|_| E2eeError::BadKey("public key must be 32 bytes".into()))?;
        Ok(Self(PublicKey::from(bytes)))
    }
}
