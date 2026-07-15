//! A durable anti-replay nonce log backed by Postgres (M8, PG backend).
//!
//! The Postgres store persists every remembered nonce in its own `fed_nonces`
//! table, so a captured command can't be replayed against a rebooted owner. Unlike
//! the in-memory [`StoreNonceLog`](crate::nonce_log::StoreNonceLog), there is no
//! snapshot to save through — the insert is already durable.

use std::sync::Arc;

use adapter_federation::{ForwardError, NonceLog};
use adapter_store_postgres::PgStore;
use async_trait::async_trait;

/// A [`NonceLog`] that records each remembered nonce in Postgres.
pub struct PgNonceLog {
    store: Arc<PgStore>,
}

impl PgNonceLog {
    pub fn new(store: Arc<PgStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl NonceLog for PgNonceLog {
    async fn remember(
        &self,
        node: u16,
        nonce: &str,
        now: i64,
        expiry_at: i64,
    ) -> Result<bool, ForwardError> {
        Ok(self.store.remember_nonce(node, nonce, now, expiry_at).await)
    }
}
