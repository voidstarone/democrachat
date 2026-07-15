//! Open (decrypt + verify) a sealed envelope.

use crate::vault::cipher::decrypt;
use crate::vault::sealed::Sealed;
use crate::vault::vault_error::VaultError;
use crate::vault::vault_key::VaultKey;

/// Open `sealed` under `key`: unwrap the per-snapshot data key, then decrypt the
/// payload. `Err(Decrypt)` if the key is wrong or either layer was tampered with.
pub fn open(key: &VaultKey, sealed: &Sealed) -> Result<Vec<u8>, VaultError> {
    let wrapped_key =
        hex::decode(&sealed.wrapped_key).map_err(|e| VaultError::Format(e.to_string()))?;
    let key_nonce = decode_nonce(&sealed.key_nonce)?;
    let data_nonce = decode_nonce(&sealed.nonce)?;
    let ciphertext =
        hex::decode(&sealed.ciphertext).map_err(|e| VaultError::Format(e.to_string()))?;

    let dek_bytes = decrypt(key.bytes(), &key_nonce, &wrapped_key)?;
    let dek: [u8; 32] = dek_bytes
        .try_into()
        .map_err(|_| VaultError::Format("unwrapped data key is not 32 bytes".into()))?;
    decrypt(&dek, &data_nonce, &ciphertext)
}

fn decode_nonce(s: &str) -> Result<[u8; 12], VaultError> {
    hex::decode(s)
        .map_err(|e| VaultError::Format(e.to_string()))?
        .try_into()
        .map_err(|_| VaultError::Format("nonce must be 12 bytes".into()))
}
