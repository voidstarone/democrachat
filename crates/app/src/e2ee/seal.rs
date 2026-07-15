//! Seal a message to a recipient's public device key (anonymous-sender sealed box).

use rand::rngs::OsRng;
use rand::RngCore;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::e2ee::derive_key::derive_key;
use crate::e2ee::public_identity::PublicIdentity;
use crate::vault::cipher::encrypt;

/// Seal `plaintext` to `recipient`. Only the holder of the matching secret key can
/// open it — the server, which stores the result, cannot. A fresh ephemeral X25519
/// keypair is used per message, so the same plaintext seals differently each time
/// and the ephemeral public key travels with the ciphertext.
///
/// Wire layout: `ephemeral_public (32) ‖ ciphertext‖tag`.
pub fn seal_to(recipient: &PublicIdentity, plaintext: &[u8]) -> Vec<u8> {
    let mut ephemeral_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut ephemeral_bytes);
    let ephemeral_secret = StaticSecret::from(ephemeral_bytes);
    let ephemeral_public = PublicKey::from(&ephemeral_secret);

    let shared = ephemeral_secret.diffie_hellman(&recipient.0);
    let (key, nonce) = derive_key(shared.as_bytes(), ephemeral_public.as_bytes(), recipient.0.as_bytes());
    let ciphertext = encrypt(&key, &nonce, plaintext);

    let mut out = Vec::with_capacity(32 + ciphertext.len());
    out.extend_from_slice(ephemeral_public.as_bytes());
    out.extend_from_slice(&ciphertext);
    out
}
