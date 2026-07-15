use app::{RoleStore, StoreError};
use async_trait::async_trait;
use domain::{Role, RoleAssignment, RoleId, ServerId, UserId};
use sqlx::Row;

use crate::{decode, next_seq, to_json, to_store_err, PgStore};

#[async_trait]
impl RoleStore for PgStore {
    async fn next_role_id(&self) -> Result<RoleId, StoreError> {
        Ok(RoleId(next_seq(self.pool(), "role_id_seq").await?))
    }

    async fn insert_role(&self, role: Role) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO roles (id, server_id, name, data) VALUES ($1, $2, $3, $4)")
            .bind(role.id.0 as i64)
            .bind(role.server_id.0 as i64)
            .bind(&role.name)
            .bind(to_json(&role))
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
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
        // Drop the role's assignments too, so no orphaned holder rows linger.
        sqlx::query("DELETE FROM role_assignments WHERE role_id = $1")
            .bind(id.0 as i64)
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        let done = sqlx::query("DELETE FROM roles WHERE id = $1")
            .bind(id.0 as i64)
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        Ok(done.rows_affected() > 0)
    }

    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Role>, StoreError> {
        let rows = sqlx::query("SELECT data FROM roles WHERE server_id = $1 ORDER BY id")
            .bind(server.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn assign(&self, assignment: RoleAssignment) -> Result<bool, StoreError> {
        let done = sqlx::query(
            "INSERT INTO role_assignments (role_id, user_id, data) VALUES ($1, $2, $3) \
             ON CONFLICT (role_id, user_id) DO NOTHING",
        )
        .bind(assignment.role_id.0 as i64)
        .bind(assignment.user.0 as i64)
        .bind(to_json(&assignment))
        .execute(self.pool())
        .await
        .map_err(to_store_err)?;
        Ok(done.rows_affected() > 0)
    }

    async fn unassign(&self, role: RoleId, user: UserId) -> Result<bool, StoreError> {
        let done = sqlx::query("DELETE FROM role_assignments WHERE role_id = $1 AND user_id = $2")
            .bind(role.0 as i64)
            .bind(user.0 as i64)
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        Ok(done.rows_affected() > 0)
    }

    async fn holders(&self, role: RoleId) -> Result<Vec<UserId>, StoreError> {
        let rows = sqlx::query("SELECT user_id FROM role_assignments WHERE role_id = $1 ORDER BY user_id")
            .bind(role.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter()
            .map(|r| r.try_get::<i64, _>("user_id").map(|id| UserId(id as u64)).map_err(to_store_err))
            .collect()
    }
}
