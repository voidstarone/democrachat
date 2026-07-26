//! Mint and hash email-verification tokens.

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use sha2::{Digest, Sha256};

/// A fresh verification token: 32 alphanumeric characters (~190 bits) from the
/// OS-seeded thread CSPRNG. This is the *raw* secret placed in the emailed link;
/// only its [`hash_token`] digest is stored, so a leaked snapshot yields no
/// working links (same rationale as invite codes).
pub fn new_verification_token() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

/// The SHA-256 (lower-hex) digest of a token — the only form persisted. Plain
/// unsalted SHA-256 suffices: the token is ~190 bits of CSPRNG entropy, so it is
/// not brute-forceable and needs no per-token salt or slow KDF.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.trim().as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for b in digest {
        hex.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        hex.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_32_alphanumeric_and_unpredictable() {
        let a = new_verification_token();
        let b = new_verification_token();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, b);
    }

    #[test]
    fn hash_is_stable_64_hex_and_trims() {
        let h = hash_token("some-token");
        assert_eq!(h.len(), 64);
        assert!(h.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(h, hash_token("  some-token  "));
    }
}
