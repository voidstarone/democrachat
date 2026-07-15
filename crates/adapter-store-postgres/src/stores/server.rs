use app::{ServerStore, StoreError};
use async_trait::async_trait;
use domain::{Server, ServerId};
use federation::ChangeOp;

use crate::{decode, next_seq, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl ServerStore for PgStore {
    async fn next_server_id(&self) -> Result<ServerId, StoreError> {
        Ok(ServerId(next_seq(self.pool(), "server_id_seq").await?))
    }

    async fn insert_server(&self, server: Server) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query("INSERT INTO servers (id, slug, data) VALUES ($1, $2, $3)")
            .bind(server.id.0 as i64)
            .bind(&server.slug)
            .bind(to_json(&server))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "servers", ChangeOp::Upsert, &server).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn get_server(&self, id: ServerId) -> Result<Option<Server>, StoreError> {
        let row = sqlx::query("SELECT data FROM servers WHERE id = $1")
            .bind(id.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn find_by_slug(&self, slug: &str) -> Result<Option<Server>, StoreError> {
        let row = sqlx::query("SELECT data FROM servers WHERE slug = $1")
            .bind(slug)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn update_server(&self, server: Server) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query("UPDATE servers SET slug = $2, data = $3 WHERE id = $1")
            .bind(server.id.0 as i64)
            .bind(&server.slug)
            .bind(to_json(&server))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "servers", ChangeOp::Upsert, &server).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<Server>, StoreError> {
        let rows = sqlx::query("SELECT data FROM servers ORDER BY id")
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn search_by_tag(&self, tag: &str) -> Result<Vec<Server>, StoreError> {
        // Fence the plain tag into its `|tag|` needle here — the pipe convention is
        // an internal storage detail, never exposed to the caller. `strpos` is a
        // literal substring test, so there are no LIKE metacharacters to escape.
        let Some(needle) = domain::Tags::search_needle(tag) else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query("SELECT data FROM servers WHERE strpos(data->>'tags', $1) > 0 ORDER BY id")
            .bind(needle)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
