use async_trait::async_trait;

use crate::command::forward_error::ForwardError;

/// A backend that records seen command nonces. `remember` returns `true` if
/// `(node, nonce)` was newly recorded (admit the command) or `false` if it was
/// already present (a replay). Implementations may prune entries past `expiry_at`.
/// A durable, store-backed log (surviving an owner restart) is the M8 upgrade over
/// the in-memory default.
#[async_trait]
pub trait NonceLog: Send + Sync {
    async fn remember(
        &self,
        node: u16,
        nonce: &str,
        now: i64,
        expiry_at: i64,
    ) -> Result<bool, ForwardError>;
}
