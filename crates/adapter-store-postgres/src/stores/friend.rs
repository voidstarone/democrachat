use app::{FriendStore, StoreError};
use async_trait::async_trait;
use domain::{Friendship, UserId};
use federation::ChangeOp;

use crate::{decode, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl FriendStore for PgStore {
    async fn add(&self, friendship: Friendship) -> Result<bool, StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        let done = sqlx::query(
            "INSERT INTO friendships (requester_id, addressee_id, data) VALUES ($1, $2, $3) \
             ON CONFLICT (requester_id, addressee_id) DO NOTHING",
        )
        .bind(friendship.requester.0 as i64)
        .bind(friendship.addressee.0 as i64)
        .bind(to_json(&friendship))
        .execute(&mut *tx)
        .await
        .map_err(to_store_err)?;
        let inserted = done.rows_affected() > 0;
        if inserted {
            push_outbox(&mut *tx, "friendships", ChangeOp::Upsert, &friendship).await?;
        }
        tx.commit().await.map_err(to_store_err)?;
        Ok(inserted)
    }

    async fn update(&self, friendship: Friendship) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query(
            "UPDATE friendships SET data = $3 WHERE requester_id = $1 AND addressee_id = $2",
        )
        .bind(friendship.requester.0 as i64)
        .bind(friendship.addressee.0 as i64)
        .bind(to_json(&friendship))
        .execute(&mut *tx)
        .await
        .map_err(to_store_err)?;
        push_outbox(&mut *tx, "friendships", ChangeOp::Upsert, &friendship).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn between(&self, a: UserId, b: UserId) -> Result<Option<Friendship>, StoreError> {
        let row = sqlx::query(
            "SELECT data FROM friendships \
             WHERE (requester_id = $1 AND addressee_id = $2) OR (requester_id = $2 AND addressee_id = $1) \
             LIMIT 1",
        )
        .bind(a.0 as i64)
        .bind(b.0 as i64)
        .fetch_optional(self.pool())
        .await
        .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn involving(&self, who: UserId) -> Result<Vec<Friendship>, StoreError> {
        let rows = sqlx::query(
            "SELECT data FROM friendships WHERE requester_id = $1 OR addressee_id = $1 \
             ORDER BY requester_id, addressee_id",
        )
        .bind(who.0 as i64)
        .fetch_all(self.pool())
        .await
        .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
