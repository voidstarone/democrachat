//! A durable anti-replay nonce log, backed by the persisted store (M8).
//!
//! The in-memory default forgets every seen nonce on restart, so a captured
//! command could be replayed against a rebooted owner. This impl records the nonce
//! in the store's snapshot and persists it, so replay protection survives a crash
//! or redeploy — the safety property a real cluster needs.

use std::sync::Arc;

use adapter_federation::{ForwardError, NonceLog};
use adapter_store_memory::MemoryStore;
use async_trait::async_trait;

/// A [`NonceLog`] that writes each remembered nonce through to the persisted store.
pub struct StoreNonceLog {
    store: Arc<MemoryStore>,
    save: Arc<dyn Fn() + Send + Sync>,
}

impl StoreNonceLog {
    pub fn new(store: Arc<MemoryStore>, save: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { store, save }
    }
}

#[async_trait]
impl NonceLog for StoreNonceLog {
    async fn remember(
        &self,
        node: u16,
        nonce: &str,
        now: i64,
        expiry_at: i64,
    ) -> Result<bool, ForwardError> {
        let newly = self.store.remember_nonce(node, nonce, now, expiry_at);
        // Persist a newly-seen nonce immediately, so a restart can't forget it.
        if newly {
            (self.save)();
        }
        Ok(newly)
    }
}
