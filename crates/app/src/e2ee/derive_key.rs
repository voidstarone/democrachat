//! Derive the per-message symmetric key + nonce from an X25519 exchange.

use sha2::{Digest, Sha256};

/// Derive a `(key, nonce)` for one sealed message from the ECDH shared secret and
/// the two public keys. The sealing side always uses a **fresh ephemeral key**, so
/// the shared secret — and thus the derived key — is unique per message; that makes
/// the deterministic nonce safe (no key is ever reused under two nonces).
///
/// Domain-separated so this key stream can never collide with the at-rest vault's.
pub(crate) fn derive_key(shared: &[u8], ephemeral_pub: &[u8], recipient_pub: &[u8]) -> ([u8; 32], [u8; 12]) {
    let mut key = [0u8; 32];
    let digest = Sha256::new()
        .chain_update(b"democrachat:e2ee:key:v1")
        .chain_update(shared)
        .chain_update(ephemeral_pub)
        .chain_update(recipient_pub)
        .finalize();
    key.copy_from_slice(&digest);

    let mut nonce = [0u8; 12];
    let ndigest = Sha256::new()
        .chain_update(b"democrachat:e2ee:nonce:v1")
        .chain_update(ephemeral_pub)
        .chain_update(recipient_pub)
        .finalize();
    nonce.copy_from_slice(&ndigest[..12]);

    (key, nonce)
}
