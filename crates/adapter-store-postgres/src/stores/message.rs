use app::{MessageStore, StoreError};
use async_trait::async_trait;
use domain::{ChannelId, Message, MessageId};

use crate::{decode, next_seq, to_json, to_store_err, PgStore};

#[async_trait]
impl MessageStore for PgStore {
    async fn next_message_id(&self) -> Result<MessageId, StoreError> {
        Ok(MessageId(next_seq(self.pool(), "message_id_seq").await?))
    }

    async fn insert_message(&self, message: Message) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO messages (id, channel_id, data) VALUES ($1, $2, $3)")
            .bind(message.id.0 as i64)
            .bind(message.channel_id.0 as i64)
            .bind(to_json(&message))
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        Ok(())
    }

    async fn get_message(&self, id: MessageId) -> Result<Option<Message>, StoreError> {
        let row = sqlx::query("SELECT data FROM messages WHERE id = $1")
            .bind(id.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn update_message(&self, message: Message) -> Result<(), StoreError> {
        sqlx::query("UPDATE messages SET channel_id = $2, data = $3 WHERE id = $1")
            .bind(message.id.0 as i64)
            .bind(message.channel_id.0 as i64)
            .bind(to_json(&message))
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        Ok(())
    }

    async fn list_for_channel(&self, channel: ChannelId) -> Result<Vec<Message>, StoreError> {
        let rows = sqlx::query("SELECT data FROM messages WHERE channel_id = $1 ORDER BY id")
            .bind(channel.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
