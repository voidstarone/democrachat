use app::{ReactionStore, StoreError};
use async_trait::async_trait;
use domain::{MessageId, Reaction, UserId};
use sqlx::Row;

use crate::{decode, to_json, to_store_err, PgStore};

#[async_trait]
impl ReactionStore for PgStore {
    async fn add(&self, reaction: Reaction) -> Result<bool, StoreError> {
        let done = sqlx::query(
            "INSERT INTO reactions (message_id, user_id, emoji, data) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (message_id, user_id, emoji) DO NOTHING",
        )
        .bind(reaction.message_id.0 as i64)
        .bind(reaction.user.0 as i64)
        .bind(&reaction.emoji)
        .bind(to_json(&reaction))
        .execute(self.pool())
        .await
        .map_err(to_store_err)?;
        Ok(done.rows_affected() > 0)
    }

    async fn remove(&self, message: MessageId, user: UserId, emoji: &str) -> Result<bool, StoreError> {
        let done = sqlx::query(
            "DELETE FROM reactions WHERE message_id = $1 AND user_id = $2 AND emoji = $3",
        )
        .bind(message.0 as i64)
        .bind(user.0 as i64)
        .bind(emoji)
        .execute(self.pool())
        .await
        .map_err(to_store_err)?;
        Ok(done.rows_affected() > 0)
    }

    async fn list_for_message(&self, message: MessageId) -> Result<Vec<Reaction>, StoreError> {
        let rows = sqlx::query(
            "SELECT data FROM reactions WHERE message_id = $1 ORDER BY user_id, emoji",
        )
        .bind(message.0 as i64)
        .fetch_all(self.pool())
        .await
        .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn user_has_any(&self, message: MessageId, user: UserId) -> Result<bool, StoreError> {
        let row = sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM reactions WHERE message_id = $1 AND user_id = $2) AS present",
        )
        .bind(message.0 as i64)
        .bind(user.0 as i64)
        .fetch_one(self.pool())
        .await
        .map_err(to_store_err)?;
        row.try_get("present").map_err(to_store_err)
    }
}
