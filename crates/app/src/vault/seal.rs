//! Seal plaintext into an on-disk envelope.

use rand::rngs::OsRng;
use rand::RngCore;

use crate::vault::cipher::encrypt;
use crate::vault::sealed::{Sealed, VAULT_VERSION};
use crate::vault::vault_key::VaultKey;

/// Seal `plaintext` under `key`. A fresh per-snapshot data key and two nonces are
/// drawn from the OS CSPRNG, so nonces are never reused even though the whole
/// snapshot is re-sealed on every save. The data is encrypted under the data key;
/// the data key is wrapped under the node's key (envelope encryption).
pub fn seal(key: &VaultKey, plaintext: &[u8]) -> Sealed {
    let mut dek = [0u8; 32];
    let mut key_nonce = [0u8; 12];
    let mut data_nonce = [0u8; 12];
    OsRng.fill_bytes(&mut dek);
    OsRng.fill_bytes(&mut key_nonce);
    OsRng.fill_bytes(&mut data_nonce);

    let ciphertext = encrypt(&dek, &data_nonce, plaintext);
    let wrapped_key = encrypt(key.bytes(), &key_nonce, &dek);

    Sealed {
        vault: VAULT_VERSION,
        wrapped_key: hex::encode(wrapped_key),
        key_nonce: hex::encode(key_nonce),
        nonce: hex::encode(data_nonce),
        ciphertext: hex::encode(ciphertext),
    }
}
