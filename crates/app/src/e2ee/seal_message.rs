//! Seal a channel message body under the channel key.

use rand::rngs::OsRng;
use rand::RngCore;

use crate::e2ee::channel_key::ChannelKey;
use crate::vault::cipher::encrypt;

/// Encrypt `plaintext` under the channel `key` with a fresh random nonce, returning
/// `nonce(12) ‖ ciphertext‖tag`. Every member holding the key can open it; the
/// server (which only stores the result) cannot. Runs client-side in the real
/// system — here so the Rust client and tests exercise the format a JS client
/// mirrors.
pub fn seal_channel_message(key: &ChannelKey, plaintext: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = encrypt(&key.to_bytes(), &nonce, plaintext);
    let mut out = Vec::with_capacity(nonce.len() + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    out
}
