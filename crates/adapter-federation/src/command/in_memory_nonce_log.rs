use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::command::forward_error::ForwardError;
use crate::command::nonce_log::NonceLog;

/// Process-local nonce log — for single-box/dev and tests. **Not durable across a
/// restart**: a durable store-backed log is needed in a real cluster so a captured
/// command can't be replayed against a rebooted owner (deferred to M8, when a
/// persistent store replaces the in-memory one).
#[derive(Default)]
pub struct InMemoryNonceLog {
    seen: Mutex<HashMap<(u16, String), i64>>,
}

#[async_trait]
impl NonceLog for InMemoryNonceLog {
    async fn remember(
        &self,
        node: u16,
        nonce: &str,
        now: i64,
        expiry_at: i64,
    ) -> Result<bool, ForwardError> {
        let mut seen = self.seen.lock().expect("nonce log mutex");
        // Drop entries too old to still be replayable, so the map stays bounded.
        seen.retain(|_, expiry| *expiry > now);
        Ok(seen.insert((node, nonce.to_string()), expiry_at).is_none())
    }
}
