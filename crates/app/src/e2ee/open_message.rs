//! Open a channel message body with the channel key.

use crate::e2ee::channel_key::ChannelKey;
use crate::e2ee::e2ee_error::E2eeError;
use crate::vault::cipher::decrypt;

/// Recover the plaintext of a channel message from `nonce(12) ‖ ciphertext‖tag`
/// using the channel `key`. A wrong key or tampered body is an `Err(Decrypt)` (an
/// AEAD tag failure). Runs client-side in the real system.
pub fn open_channel_message(key: &ChannelKey, sealed: &[u8]) -> Result<Vec<u8>, E2eeError> {
    if sealed.len() < 12 {
        return Err(E2eeError::Decrypt);
    }
    let (nonce, ciphertext) = sealed.split_at(12);
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| E2eeError::Decrypt)?;
    decrypt(&key.to_bytes(), &nonce, ciphertext).map_err(|_| E2eeError::Decrypt)
}
