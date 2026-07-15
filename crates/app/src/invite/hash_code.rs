//! Hash an invite code for storage and lookup.

use sha2::{Digest, Sha256};

/// The SHA-256 (lower-hex) digest of an invite code. Only this hash is stored or
/// queried, so a leaked snapshot yields no working invite codes — the same reason
/// password *hashes*, not passwords, are stored. A plain unsalted SHA-256 suffices:
/// the code is ~95 bits of CSPRNG entropy (see
/// [`new_invite_code`](crate::invite::new_invite_code::new_invite_code)), so it is
/// not brute-forceable and needs no per-code salt or slow KDF.
pub fn hash_code(code: &str) -> String {
    let digest = Sha256::digest(code.trim().as_bytes());
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
    fn is_stable_64_char_hex_and_trims() {
        let h = hash_code("some-code");
        assert_eq!(h.len(), 64);
        assert!(h.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(h, hash_code("  some-code  "), "hashing trims and is deterministic");
    }

    #[test]
    fn distinct_codes_hash_differently() {
        assert_ne!(hash_code("a"), hash_code("b"));
    }
}
