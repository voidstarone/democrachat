use app::{DmStore, StoreError};
use async_trait::async_trait;
use domain::{DmId, DmMessage, UserId};
use federation::ChangeOp;
use sqlx::Row;

use crate::{decode, next_seq, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl DmStore for PgStore {
    async fn next_dm_id(&self) -> Result<DmId, StoreError> {
        Ok(DmId(next_seq(self.pool(), "dm_id_seq").await?))
    }

    async fn insert_dm(&self, message: DmMessage) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query("INSERT INTO dms (id, sender_id, recipient_id, data) VALUES ($1, $2, $3, $4)")
            .bind(message.id.0 as i64)
            .bind(message.sender.0 as i64)
            .bind(message.recipient.0 as i64)
            .bind(to_json(&message))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "dms", ChangeOp::Upsert, &message).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn conversation(&self, a: UserId, b: UserId) -> Result<Vec<DmMessage>, StoreError> {
        let rows = sqlx::query(
            "SELECT data FROM dms \
             WHERE (sender_id = $1 AND recipient_id = $2) OR (sender_id = $2 AND recipient_id = $1) \
             ORDER BY id",
        )
        .bind(a.0 as i64)
        .bind(b.0 as i64)
        .fetch_all(self.pool())
        .await
        .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn partners(&self, who: UserId) -> Result<Vec<UserId>, StoreError> {
        // Each distinct correspondent, most-recently-messaged first.
        let rows = sqlx::query(
            "SELECT other FROM ( \
               SELECT CASE WHEN sender_id = $1 THEN recipient_id ELSE sender_id END AS other, id \
               FROM dms WHERE sender_id = $1 OR recipient_id = $1 \
             ) t GROUP BY other ORDER BY max(id) DESC",
        )
        .bind(who.0 as i64)
        .fetch_all(self.pool())
        .await
        .map_err(to_store_err)?;
        rows.iter()
            .map(|r| r.try_get::<i64, _>("other").map(|id| UserId(id as u64)).map_err(to_store_err))
            .collect()
    }
}
