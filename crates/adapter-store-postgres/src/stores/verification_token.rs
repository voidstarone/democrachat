use app::{StoreError, VerificationTokenStore};
use async_trait::async_trait;
use domain::UserId;
use sqlx::Row;

use crate::{to_store_err, PgStore};

#[async_trait]
impl VerificationTokenStore for PgStore {
    async fn add(
        &self,
        token_hash: String,
        user_id: UserId,
        expires_at: i64,
    ) -> Result<(), StoreError> {
        // Node-local (not federated), so no outbox push. `ON CONFLICT` keeps `add`
        // idempotent even in the astronomically-unlikely event of a digest reuse.
        sqlx::query(
            "INSERT INTO verification_tokens (token_hash, user_id, expires_at) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (token_hash) \
             DO UPDATE SET user_id = EXCLUDED.user_id, expires_at = EXCLUDED.expires_at",
        )
        .bind(&token_hash)
        .bind(user_id.0 as i64)
        .bind(expires_at)
        .execute(self.pool())
        .await
        .map_err(to_store_err)?;
        Ok(())
    }

    async fn take(&self, token_hash: &str, now: i64) -> Result<Option<UserId>, StoreError> {
        // Prune expired tokens opportunistically so the table stays bounded.
        sqlx::query("DELETE FROM verification_tokens WHERE expires_at <= $1")
            .bind(now)
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        // Single-use: delete and return the row only if it exists and is unexpired.
        let row = sqlx::query(
            "DELETE FROM verification_tokens \
             WHERE token_hash = $1 AND expires_at > $2 RETURNING user_id",
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(self.pool())
        .await
        .map_err(to_store_err)?;
        match row {
            Some(r) => {
                let id: i64 = r.try_get("user_id").map_err(to_store_err)?;
                Ok(Some(UserId(id as u64)))
            }
            None => Ok(None),
        }
    }
}
