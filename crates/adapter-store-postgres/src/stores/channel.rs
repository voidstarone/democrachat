use app::{ChannelStore, StoreError};
use async_trait::async_trait;
use domain::{Channel, ChannelId, ServerId};

use crate::{decode, next_seq, to_json, to_store_err, PgStore};

#[async_trait]
impl ChannelStore for PgStore {
    async fn next_channel_id(&self) -> Result<ChannelId, StoreError> {
        Ok(ChannelId(next_seq(self.pool(), "channel_id_seq").await?))
    }

    async fn insert_channel(&self, channel: Channel) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO channels (id, server_id, name, data) VALUES ($1, $2, $3, $4)")
            .bind(channel.id.0 as i64)
            .bind(channel.server_id.0 as i64)
            .bind(&channel.name)
            .bind(to_json(&channel))
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
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
        let done = sqlx::query("DELETE FROM channels WHERE id = $1")
            .bind(id.0 as i64)
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        Ok(done.rows_affected() > 0)
    }
}
