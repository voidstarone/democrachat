//! Wrap a device secret under a password for server-blind storage.

use rand::rngs::OsRng;
use rand::RngCore;

use crate::e2ee::derive_kek::derive_kek;
use crate::e2ee::e2ee_error::E2eeError;
use crate::e2ee::identity_secret::IdentitySecret;
use crate::e2ee::wrapped_secret::WrappedSecret;
use crate::vault::cipher::encrypt;

/// Wrap `secret` under `password`: derive a KEK (Argon2id + fresh salt) and encrypt
/// the 32 secret bytes under it. The result is safe to store server-side — without
/// the password it is opaque. Runs **client-side** in the real system; kept here so
/// the Rust CLI client and the tests can exercise the whole flow.
pub fn wrap_secret(password: &str, secret: &IdentitySecret) -> Result<WrappedSecret, E2eeError> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    let kek = derive_kek(password, &salt)?;
    let ciphertext = encrypt(&kek, &nonce, &secret.to_bytes());

    Ok(WrappedSecret {
        salt: hex::encode(salt),
        nonce: hex::encode(nonce),
        ciphertext: hex::encode(ciphertext),
    })
}
