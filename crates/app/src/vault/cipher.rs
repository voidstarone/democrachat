//! The low-level AEAD used by both seal and open: ChaCha20-Poly1305.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

use crate::vault::vault_error::VaultError;

/// Encrypt `plaintext` under `key` with `nonce`, returning ciphertext‖tag. The
/// AEAD only errors on absurdly large inputs (far beyond a snapshot), so a failure
/// here is a programming error, not a runtime condition.
pub(crate) fn encrypt(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .encrypt(Nonce::from_slice(nonce), plaintext)
        .expect("ChaCha20-Poly1305 encrypt")
}

/// Decrypt and verify. A tag mismatch (wrong key or tampered data) is a
/// [`VaultError::Decrypt`] — the two are deliberately indistinguishable.
pub(crate) fn decrypt(key: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| VaultError::Decrypt)
}
