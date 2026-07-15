//! At-rest envelope encryption for the persisted governance surface (M6).
//!
//! A node's data is decrypted only in RAM to compute (tally votes, evaluate
//! eligibility); it is **sealed** whenever it touches disk. Sealing is envelope
//! encryption: a fresh per-snapshot data key encrypts the data, and the node's
//! root [`VaultKey`] (unwrapped at boot from a KMS/secret env var, held only in
//! RAM) wraps the data key — so the root key can rotate without re-encrypting
//! history. See `docs/federation.md` §5b.
//!
//! The primitive is pure (`&[u8]` in, `&[u8]` out); the composition root decides
//! *when* to seal (only when a data key is configured — a single-box dev
//! deployment persists plaintext, unchanged).

pub mod cipher;
pub mod open;
pub mod seal;
pub mod sealed;
pub mod vault_error;
pub mod vault_key;

#[cfg(test)]
mod tests {
    use super::open::open;
    use super::seal::seal;
    use super::sealed::Sealed;
    use super::vault_key::VaultKey;

    fn key(byte: u8) -> VaultKey {
        VaultKey::from_hex(&hex::encode([byte; 32])).unwrap()
    }

    #[test]
    fn a_sealed_snapshot_round_trips() {
        let k = key(0x11);
        let plaintext = br#"{"users":[],"servers":[]}"#;
        let sealed = seal(&k, plaintext);
        // On disk it is opaque — none of the plaintext leaks into the envelope.
        let json = sealed.to_json();
        assert!(!json.contains("users"), "the entity names must not leak");
        let reopened = Sealed::from_json(&json).unwrap();
        assert_eq!(open(&k, &reopened).unwrap(), plaintext);
    }

    #[test]
    fn a_wrong_key_cannot_open_it() {
        let sealed = seal(&key(0x11), b"secret governance state");
        assert!(open(&key(0x22), &sealed).is_err(), "a different node's key must fail");
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let k = key(0x11);
        let mut sealed = seal(&k, b"aye");
        // Flip the last hex nibble of the ciphertext.
        let mut ct: Vec<char> = sealed.ciphertext.chars().collect();
        let last = ct.len() - 1;
        ct[last] = if ct[last] == '0' { '1' } else { '0' };
        sealed.ciphertext = ct.into_iter().collect();
        assert!(open(&k, &sealed).is_err(), "the AEAD tag must reject tampering");
    }

    #[test]
    fn each_seal_uses_a_fresh_nonce_and_key() {
        let k = key(0x11);
        let a = seal(&k, b"same plaintext");
        let b = seal(&k, b"same plaintext");
        // Fresh data key + nonces every time → no two seals of the same plaintext
        // are byte-identical (no deterministic-encryption leakage).
        assert_ne!(a.ciphertext, b.ciphertext);
        assert_ne!(a.wrapped_key, b.wrapped_key);
    }

    #[test]
    fn plaintext_is_not_mistaken_for_an_envelope() {
        // A legacy plaintext snapshot must fail Sealed parsing so the loader knows
        // to read it as plaintext (migration path).
        assert!(Sealed::from_json(r#"{"users":[],"servers":[]}"#).is_err());
    }

    #[test]
    fn a_bad_key_length_is_rejected() {
        assert!(VaultKey::from_hex("abcd").is_err());
        assert!(VaultKey::from_hex("nothex...").is_err());
    }
}
