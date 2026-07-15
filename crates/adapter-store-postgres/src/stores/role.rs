use app::{RoleStore, StoreError};
use async_trait::async_trait;
use domain::{Role, RoleId, ServerId};
use federation::ChangeOp;

use crate::{decode, next_seq, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl RoleStore for PgStore {
    async fn next_role_id(&self) -> Result<RoleId, StoreError> {
        Ok(RoleId(next_seq(self.pool(), "role_id_seq").await?))
    }

    async fn insert_role(&self, role: Role) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query("INSERT INTO roles (id, server_id, name, data) VALUES ($1, $2, $3, $4)")
            .bind(role.id.0 as i64)
            .bind(role.server_id.0 as i64)
            .bind(&role.name)
            .bind(to_json(&role))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "roles", ChangeOp::Upsert, &role).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn get_role(&self, id: RoleId) -> Result<Option<Role>, StoreError> {
        let row = sqlx::query("SELECT data FROM roles WHERE id = $1")
            .bind(id.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn find_role(&self, server: ServerId, name: &str) -> Result<Option<Role>, StoreError> {
        let row = sqlx::query("SELECT data FROM roles WHERE server_id = $1 AND name = $2")
            .bind(server.0 as i64)
            .bind(name)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn remove_role(&self, id: RoleId) -> Result<bool, StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        let row = sqlx::query("SELECT data FROM roles WHERE id = $1 FOR UPDATE")
            .bind(id.0 as i64)
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_store_err)?;
        let Some(row) = row else {
            return Ok(false);
        };
        let role: Role = decode(&row)?;
        // Role membership is derived from criteria, not stored, so a role has no
        // holder rows to cascade — the single "roles" delete is the whole change.
        sqlx::query("DELETE FROM roles WHERE id = $1")
            .bind(id.0 as i64)
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "roles", ChangeOp::Delete, &role).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(true)
    }

    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Role>, StoreError> {
        let rows = sqlx::query("SELECT data FROM roles WHERE server_id = $1 ORDER BY id")
            .bind(server.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
