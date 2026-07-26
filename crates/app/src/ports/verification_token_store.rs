//! Persistence for email-verification tokens (stored by digest only).

use async_trait::async_trait;

use domain::UserId;

use crate::StoreError;

/// Persistence for pending email-verification tokens. Like invite codes, only the
/// SHA-256 digest of the emailed token is stored, so a leaked snapshot yields no
/// working links. Each token carries an expiry (unix seconds); implementations
/// prune expired entries on access, mirroring the federation nonce log's TTL.
#[async_trait]
pub trait VerificationTokenStore: Send + Sync {
    /// Record a token digest for `user_id`, valid until `expires_at` (unix seconds).
    async fn add(
        &self,
        token_hash: String,
        user_id: UserId,
        expires_at: i64,
    ) -> Result<(), StoreError>;
    /// Consume the token with this digest if it exists and has not expired as of
    /// `now`, returning the account it was issued for. Single-use: a successful
    /// take removes it. Implementations should also prune expired entries.
    async fn take(&self, token_hash: &str, now: i64) -> Result<Option<UserId>, StoreError>;
}
