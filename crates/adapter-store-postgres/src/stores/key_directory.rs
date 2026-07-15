use app::{KeyDirectoryStore, StoreError};
use async_trait::async_trait;
use domain::{UserId, UserKeys};
use federation::ChangeOp;

use crate::{decode, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl KeyDirectoryStore for PgStore {
    async fn put_keys(&self, keys: UserKeys) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query(
            "INSERT INTO user_keys (user_id, data) VALUES ($1, $2) \
             ON CONFLICT (user_id) DO UPDATE SET data = EXCLUDED.data",
        )
        .bind(keys.user_id.0 as i64)
        .bind(to_json(&keys))
        .execute(&mut *tx)
        .await
        .map_err(to_store_err)?;
        push_outbox(&mut *tx, "user_keys", ChangeOp::Upsert, &keys).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn get_keys(&self, user: UserId) -> Result<Option<UserKeys>, StoreError> {
        let row = sqlx::query("SELECT data FROM user_keys WHERE user_id = $1")
            .bind(user.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }
}
