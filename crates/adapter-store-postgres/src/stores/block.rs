use app::{BlockStore, StoreError};
use async_trait::async_trait;
use domain::{Block, UserId};
use sqlx::Row;

use crate::{decode, to_json, to_store_err, PgStore};

#[async_trait]
impl BlockStore for PgStore {
    async fn add(&self, block: Block) -> Result<bool, StoreError> {
        let done = sqlx::query(
            "INSERT INTO blocks (blocker_id, blocked_id, data) VALUES ($1, $2, $3) \
             ON CONFLICT (blocker_id, blocked_id) DO NOTHING",
        )
        .bind(block.blocker.0 as i64)
        .bind(block.blocked.0 as i64)
        .bind(to_json(&block))
        .execute(self.pool())
        .await
        .map_err(to_store_err)?;
        Ok(done.rows_affected() > 0)
    }

    async fn involving(&self, who: UserId) -> Result<Vec<Block>, StoreError> {
        let rows = sqlx::query(
            "SELECT data FROM blocks WHERE blocker_id = $1 OR blocked_id = $1 ORDER BY blocker_id, blocked_id",
        )
        .bind(who.0 as i64)
        .fetch_all(self.pool())
        .await
        .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn is_blocked_between(&self, a: UserId, b: UserId) -> Result<bool, StoreError> {
        let row = sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM blocks \
             WHERE (blocker_id = $1 AND blocked_id = $2) OR (blocker_id = $2 AND blocked_id = $1)) AS present",
        )
        .bind(a.0 as i64)
        .bind(b.0 as i64)
        .fetch_one(self.pool())
        .await
        .map_err(to_store_err)?;
        row.try_get("present").map_err(to_store_err)
    }
}
