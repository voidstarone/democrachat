//! Open a sealed box with the recipient's secret device key.

use x25519_dalek::PublicKey;

use crate::e2ee::derive_key::derive_key;
use crate::e2ee::e2ee_error::E2eeError;
use crate::e2ee::identity_secret::IdentitySecret;
use crate::vault::cipher::decrypt;

/// Open a box sealed to this `secret`'s public key. `Err(Decrypt)` if the box was
/// sealed to a different key or tampered with; `Err(BadSealed)` if it is too short
/// to hold its header.
pub fn open_sealed(secret: &IdentitySecret, sealed: &[u8]) -> Result<Vec<u8>, E2eeError> {
    // 32 bytes ephemeral public key + at least the 16-byte AEAD tag.
    if sealed.len() < 32 + 16 {
        return Err(E2eeError::BadSealed);
    }
    let ephemeral_bytes: [u8; 32] = sealed[..32].try_into().expect("checked length");
    let ephemeral_public = PublicKey::from(ephemeral_bytes);
    let ciphertext = &sealed[32..];

    let shared = secret.0.diffie_hellman(&ephemeral_public);
    let recipient_public = PublicKey::from(&secret.0);
    let (key, nonce) = derive_key(shared.as_bytes(), ephemeral_public.as_bytes(), recipient_public.as_bytes());
    decrypt(&key, &nonce, ciphertext).map_err(|_| E2eeError::Decrypt)
}
