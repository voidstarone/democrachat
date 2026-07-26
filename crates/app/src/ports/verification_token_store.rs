//! Persistence for email-verification tokens (stored by digest only).

use domain::UserId;

/// Persistence for pending email-verification tokens. Like invite codes, only the
/// SHA-256 digest of the emailed token is stored, so a leaked snapshot yields no
/// working links. Each token carries an expiry (unix seconds); the store prunes
/// expired entries on access, mirroring the node-nonce TTL in the memory store.
pub trait VerificationTokenStore: Send + Sync {
    /// Record a token digest for `user_id`, valid until `expires_at` (unix seconds).
    fn add(&self, token_hash: String, user_id: UserId, expires_at: i64);
    /// Consume the token with this digest if it exists and has not expired as of
    /// `now`, returning the account it was issued for. Single-use: a successful
    /// `take` removes it. Implementations should also prune any expired entries.
    fn take(&self, token_hash: &str, now: i64) -> Option<UserId>;
}
