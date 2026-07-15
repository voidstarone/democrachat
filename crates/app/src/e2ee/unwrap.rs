//! Unwrap a device secret with the user's password.

use crate::e2ee::derive_kek::derive_kek;
use crate::e2ee::e2ee_error::E2eeError;
use crate::e2ee::identity_secret::IdentitySecret;
use crate::e2ee::wrapped_secret::WrappedSecret;
use crate::vault::cipher::decrypt;

/// Recover the device secret from `wrapped` using `password`. `Err(Decrypt)` on the
/// wrong password (an AEAD tag failure — indistinguishable from tampering, so a
/// guesser learns nothing beyond "wrong"). Runs **client-side** in the real system.
pub fn unwrap_secret(password: &str, wrapped: &WrappedSecret) -> Result<IdentitySecret, E2eeError> {
    let salt = hex::decode(&wrapped.salt).map_err(|e| E2eeError::BadKey(e.to_string()))?;
    let nonce: [u8; 12] = hex::decode(&wrapped.nonce)
        .map_err(|e| E2eeError::BadKey(e.to_string()))?
        .try_into()
        .map_err(|_| E2eeError::BadKey("nonce must be 12 bytes".into()))?;
    let ciphertext = hex::decode(&wrapped.ciphertext).map_err(|e| E2eeError::BadKey(e.to_string()))?;

    let kek = derive_kek(password, &salt)?;
    let secret_bytes: [u8; 32] = decrypt(&kek, &nonce, &ciphertext)
        .map_err(|_| E2eeError::Decrypt)?
        .try_into()
        .map_err(|_| E2eeError::BadKey("unwrapped secret is not 32 bytes".into()))?;
    IdentitySecret::from_hex(&hex::encode(secret_bytes))
}
