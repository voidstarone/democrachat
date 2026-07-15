use app::{InviteStore, StoreError};
use async_trait::async_trait;
use domain::{Invite, ServerId};
use federation::ChangeOp;

use crate::{decode, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl InviteStore for PgStore {
    async fn add(&self, invite: Invite) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query("INSERT INTO invites (code_hash, server_id, data) VALUES ($1, $2, $3)")
            .bind(&invite.code_hash)
            .bind(invite.server_id.0 as i64)
            .bind(to_json(&invite))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "invites", ChangeOp::Upsert, &invite).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn by_hash(&self, code_hash: &str) -> Result<Option<Invite>, StoreError> {
        let row = sqlx::query("SELECT data FROM invites WHERE code_hash = $1")
            .bind(code_hash)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn list_for_server(&self, server_id: ServerId) -> Result<Vec<Invite>, StoreError> {
        let rows = sqlx::query("SELECT data FROM invites WHERE server_id = $1 ORDER BY code_hash")
            .bind(server_id.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn revoke(&self, code_hash: &str) -> Result<(), StoreError> {
        // Flip the tombstone flag in place — the row is kept so the same code can
        // never be silently re-minted into a live invite. A revoke replicates as an
        // upsert of the now-revoked row (the memory store emits it the same way).
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        let row = sqlx::query(
            "UPDATE invites SET data = jsonb_set(data, '{is_revoked}', 'true') \
             WHERE code_hash = $1 RETURNING data",
        )
        .bind(code_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_store_err)?;
        if let Some(row) = row {
            let invite: Invite = decode(&row)?;
            push_outbox(&mut *tx, "invites", ChangeOp::Upsert, &invite).await?;
        }
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }
}
