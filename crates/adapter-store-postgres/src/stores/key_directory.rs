use app::{KeyDirectoryStore, StoreError};
use async_trait::async_trait;
use domain::{UserId, UserKeys};

use crate::{decode, to_json, to_store_err, PgStore};

#[async_trait]
impl KeyDirectoryStore for PgStore {
    async fn put_keys(&self, keys: UserKeys) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO user_keys (user_id, data) VALUES ($1, $2) \
             ON CONFLICT (user_id) DO UPDATE SET data = EXCLUDED.data",
        )
        .bind(keys.user_id.0 as i64)
        .bind(to_json(&keys))
        .execute(self.pool())
        .await
        .map_err(to_store_err)?;
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
