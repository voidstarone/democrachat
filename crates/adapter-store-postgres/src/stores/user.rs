use app::{StoreError, UserStore};
use async_trait::async_trait;
use domain::{User, UserId};

use crate::{decode, next_seq, to_json, to_store_err, PgStore};

#[async_trait]
impl UserStore for PgStore {
    async fn next_user_id(&self) -> Result<UserId, StoreError> {
        Ok(UserId(next_seq(self.pool(), "user_id_seq").await?))
    }

    async fn insert_user(&self, user: User) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO users (id, handle, data) VALUES ($1, $2, $3)")
            .bind(user.id.0 as i64)
            .bind(&user.handle)
            .bind(to_json(&user))
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        Ok(())
    }

    async fn update_user(&self, user: User) -> Result<(), StoreError> {
        sqlx::query("UPDATE users SET handle = $2, data = $3 WHERE id = $1")
            .bind(user.id.0 as i64)
            .bind(&user.handle)
            .bind(to_json(&user))
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        Ok(())
    }

    async fn get_user(&self, id: UserId) -> Result<Option<User>, StoreError> {
        let row = sqlx::query("SELECT data FROM users WHERE id = $1")
            .bind(id.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn find_by_handle(&self, handle: &str) -> Result<Option<User>, StoreError> {
        let row = sqlx::query("SELECT data FROM users WHERE handle = $1")
            .bind(handle)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn list_all(&self) -> Result<Vec<User>, StoreError> {
        let rows = sqlx::query("SELECT data FROM users ORDER BY id")
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
