use app::{EmojiStore, StoreError};
use async_trait::async_trait;
use domain::{Emoji, EmojiId, ServerId};
use federation::ChangeOp;

use crate::{decode, next_seq, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl EmojiStore for PgStore {
    async fn next_emoji_id(&self) -> Result<EmojiId, StoreError> {
        Ok(EmojiId(next_seq(self.pool(), "emoji_id_seq").await?))
    }

    async fn insert_emoji(&self, emoji: Emoji) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query("INSERT INTO emojis (id, server_id, name, data) VALUES ($1, $2, $3, $4)")
            .bind(emoji.id.0 as i64)
            .bind(emoji.server_id.0 as i64)
            .bind(&emoji.name)
            .bind(to_json(&emoji))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "emojis", ChangeOp::Upsert, &emoji).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn get_emoji(&self, id: EmojiId) -> Result<Option<Emoji>, StoreError> {
        let row = sqlx::query("SELECT data FROM emojis WHERE id = $1")
            .bind(id.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn find_emoji(&self, server: ServerId, name: &str) -> Result<Option<Emoji>, StoreError> {
        let row = sqlx::query("SELECT data FROM emojis WHERE server_id = $1 AND name = $2")
            .bind(server.0 as i64)
            .bind(name)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Emoji>, StoreError> {
        let rows = sqlx::query("SELECT data FROM emojis WHERE server_id = $1 ORDER BY id")
            .bind(server.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
