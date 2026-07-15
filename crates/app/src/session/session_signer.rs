//! Signs and verifies session cookies with HMAC-SHA256.

use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;

use crate::session::constant_time_eq::constant_time_eq;

type HmacSha256 = Hmac<Sha256>;

/// Signs and verifies the opaque part of a session cookie.
///
/// The token is `"<uid>.<expires_at>.<hex-hmac>"`, where the MAC covers **both**
/// the uid and the expiry (as bytes), so a client can neither change the uid to
/// impersonate another account nor extend its own session's lifetime. The key is
/// a shared secret (so sessions survive a restart and are valid fleet-wide) or a
/// random per-process key ([`SessionSigner::ephemeral`]) when no secret is set.
#[derive(Clone)]
pub struct SessionSigner {
    key: Vec<u8>,
}

impl SessionSigner {
    /// Build a signer from a shared secret string.
    pub fn from_secret(secret: &str) -> Self {
        Self { key: secret.as_bytes().to_vec() }
    }

    /// Build a signer with a fresh random 256-bit key — sessions then last only
    /// for this process's lifetime. The safe default when no secret is configured.
    pub fn ephemeral() -> Self {
        let mut key = vec![0u8; 32];
        OsRng.fill_bytes(&mut key);
        Self { key }
    }

    /// Sign a session for `uid` expiring at `expires_at` (epoch seconds).
    pub fn sign(&self, uid: u64, expires_at: i64) -> String {
        let mac = self.mac(uid, expires_at);
        format!("{uid}.{expires_at}.{}", hex::encode(mac))
    }

    /// Verify a token and, if the MAC is valid, return `(uid, expires_at)`. Does
    /// **not** check expiry — the caller compares `expires_at` against its clock,
    /// so a captured-but-expired cookie is rejected even if replayed.
    pub fn verify(&self, token: &str) -> Option<(u64, i64)> {
        let mut parts = token.splitn(3, '.');
        let uid: u64 = parts.next()?.parse().ok()?;
        let expires_at: i64 = parts.next()?.parse().ok()?;
        let sig = parts.next()?;
        let expected = self.mac(uid, expires_at);
        let given = hex::decode(sig).ok()?;
        if constant_time_eq(&given, &expected) {
            Some((uid, expires_at))
        } else {
            None
        }
    }

    fn mac(&self, uid: u64, expires_at: i64) -> Vec<u8> {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC accepts any key length");
        mac.update(&uid.to_le_bytes());
        mac.update(&expires_at.to_le_bytes());
        mac.finalize().into_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_uid_and_expiry() {
        let s = SessionSigner::from_secret("a-sufficiently-long-secret-value");
        let tok = s.sign(42, 1_000);
        assert_eq!(s.verify(&tok), Some((42, 1_000)));
    }

    #[test]
    fn rejects_a_tampered_uid() {
        let s = SessionSigner::from_secret("a-sufficiently-long-secret-value");
        let tok = s.sign(42, 1_000);
        // Forge uid=1 keeping the original MAC.
        let forged = format!("1.1000.{}", tok.rsplit('.').next().unwrap());
        assert_eq!(s.verify(&forged), None);
    }

    #[test]
    fn rejects_an_extended_expiry() {
        let s = SessionSigner::from_secret("a-sufficiently-long-secret-value");
        let tok = s.sign(42, 1_000);
        let forged = format!("42.99999999.{}", tok.rsplit('.').next().unwrap());
        assert_eq!(s.verify(&forged), None);
    }

    #[test]
    fn a_different_key_does_not_verify() {
        let a = SessionSigner::from_secret("secret-number-one-is-long-enough");
        let b = SessionSigner::from_secret("secret-number-two-is-long-enough");
        let tok = a.sign(7, 500);
        assert_eq!(b.verify(&tok), None);
    }

    #[test]
    fn garbage_is_rejected() {
        let s = SessionSigner::ephemeral();
        assert_eq!(s.verify("not-a-token"), None);
        assert_eq!(s.verify(""), None);
    }
}
