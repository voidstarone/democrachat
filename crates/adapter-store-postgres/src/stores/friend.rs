use app::{FriendStore, StoreError};
use async_trait::async_trait;
use domain::{Friendship, UserId};

use crate::{decode, to_json, to_store_err, PgStore};

#[async_trait]
impl FriendStore for PgStore {
    async fn add(&self, friendship: Friendship) -> Result<bool, StoreError> {
        let done = sqlx::query(
            "INSERT INTO friendships (requester_id, addressee_id, data) VALUES ($1, $2, $3) \
             ON CONFLICT (requester_id, addressee_id) DO NOTHING",
        )
        .bind(friendship.requester.0 as i64)
        .bind(friendship.addressee.0 as i64)
        .bind(to_json(&friendship))
        .execute(self.pool())
        .await
        .map_err(to_store_err)?;
        Ok(done.rows_affected() > 0)
    }

    async fn update(&self, friendship: Friendship) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE friendships SET data = $3 WHERE requester_id = $1 AND addressee_id = $2",
        )
        .bind(friendship.requester.0 as i64)
        .bind(friendship.addressee.0 as i64)
        .bind(to_json(&friendship))
        .execute(self.pool())
        .await
        .map_err(to_store_err)?;
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
