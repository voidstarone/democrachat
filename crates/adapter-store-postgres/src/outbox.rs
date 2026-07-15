//! The producer side of federation replication: the Postgres store as a
//! [`ChangeSource`]. Its outbox table is written transactionally with every
//! mutation (see [`push_outbox`](crate::push_outbox)), so the feed a peer pulls is
//! always exactly the writes that committed — no gaps, no phantom rows.

use async_trait::async_trait;
use federation::{ChangeOp, ChangeRecord, ChangeSource};
use sqlx::Row;

use crate::PgStore;

#[async_trait]
impl ChangeSource for PgStore {
    async fn changes_since(&self, after_seq: u64, limit: u64) -> Vec<ChangeRecord> {
        let rows = sqlx::query(
            "SELECT seq, entity, op, payload FROM outbox WHERE seq > $1 ORDER BY seq LIMIT $2",
        )
        .bind(after_seq as i64)
        .bind(limit as i64)
        .fetch_all(self.pool())
        .await
        .unwrap_or_default();

        rows.iter()
            .filter_map(|r| {
                let seq: i64 = r.try_get("seq").ok()?;
                let entity: String = r.try_get("entity").ok()?;
                let op: String = r.try_get("op").ok()?;
                let payload: serde_json::Value = r.try_get("payload").ok()?;
                let op = match op.as_str() {
                    "upsert" => ChangeOp::Upsert,
                    "delete" => ChangeOp::Delete,
                    _ => return None,
                };
                Some(ChangeRecord { seq: seq as u64, entity, op, payload })
            })
            .collect()
    }
}
