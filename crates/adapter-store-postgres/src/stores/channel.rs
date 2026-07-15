use app::{ChannelStore, StoreError};
use async_trait::async_trait;
use domain::{Channel, ChannelId, ServerId};
use federation::ChangeOp;

use crate::{decode, next_seq, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl ChannelStore for PgStore {
    async fn next_channel_id(&self) -> Result<ChannelId, StoreError> {
        Ok(ChannelId(next_seq(self.pool(), "channel_id_seq").await?))
    }

    async fn insert_channel(&self, channel: Channel) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        // Upsert: the memory store's insert overwrites, and callers rely on that to
        // persist channel edits (enable-encryption, tags) by re-inserting the row.
        sqlx::query(
            "INSERT INTO channels (id, server_id, name, data) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, data = EXCLUDED.data",
        )
            .bind(channel.id.0 as i64)
            .bind(channel.server_id.0 as i64)
            .bind(&channel.name)
            .bind(to_json(&channel))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "channels", ChangeOp::Upsert, &channel).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn get_channel(&self, id: ChannelId) -> Result<Option<Channel>, StoreError> {
        let row = sqlx::query("SELECT data FROM channels WHERE id = $1")
            .bind(id.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn find_by_name(&self, server: ServerId, name: &str) -> Result<Option<Channel>, StoreError> {
        let row = sqlx::query("SELECT data FROM channels WHERE server_id = $1 AND name = $2")
            .bind(server.0 as i64)
            .bind(name)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Channel>, StoreError> {
        let rows = sqlx::query("SELECT data FROM channels WHERE server_id = $1 ORDER BY id")
            .bind(server.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn remove_channel(&self, id: ChannelId) -> Result<bool, StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        // Fetch first so the outbox delete carries the whole row — the consumer
        // identifies (and cascades) from the payload, exactly as the memory store does.
        let row = sqlx::query("SELECT data FROM channels WHERE id = $1 FOR UPDATE")
            .bind(id.0 as i64)
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_store_err)?;
        let Some(row) = row else {
            return Ok(false);
        };
        let channel: Channel = decode(&row)?;
        sqlx::query("DELETE FROM channels WHERE id = $1")
            .bind(id.0 as i64)
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "channels", ChangeOp::Delete, &channel).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(true)
    }

    async fn search_by_tag(&self, tag: &str) -> Result<Vec<Channel>, StoreError> {
        let Some(needle) = domain::Tags::search_needle(tag) else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query("SELECT data FROM channels WHERE strpos(data->>'tags', $1) > 0 ORDER BY id")
            .bind(needle)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
