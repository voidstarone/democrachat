use app::{CapAdmission, MembershipStore, StoreError};
use async_trait::async_trait;
use domain::{Membership, ServerId, Timestamp, UserId};
use federation::ChangeOp;
use sqlx::Row;

use crate::{decode, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl MembershipStore for PgStore {
    async fn upsert(&self, membership: Membership) -> Result<(), StoreError> {
        // `tier` and `enfranchised_at` are lifted into columns so `citizen_count`
        // and `admitted_since` are index scans, not full-table JSONB filters.
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query(
            "INSERT INTO memberships (user_id, server_id, tier, enfranchised_at, data) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (user_id, server_id) DO UPDATE \
             SET tier = EXCLUDED.tier, enfranchised_at = EXCLUDED.enfranchised_at, data = EXCLUDED.data",
        )
        .bind(membership.user_id.0 as i64)
        .bind(membership.server_id.0 as i64)
        .bind(format!("{:?}", membership.tier))
        .bind(membership.enfranchised_at.map(|t| t.0))
        .bind(to_json(&membership))
        .execute(&mut *tx)
        .await
        .map_err(to_store_err)?;
        push_outbox(&mut *tx, "memberships", ChangeOp::Upsert, &membership).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn get(&self, user: UserId, server: ServerId) -> Result<Option<Membership>, StoreError> {
        let row = sqlx::query("SELECT data FROM memberships WHERE user_id = $1 AND server_id = $2")
            .bind(user.0 as i64)
            .bind(server.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Membership>, StoreError> {
        let rows = sqlx::query("SELECT data FROM memberships WHERE server_id = $1 ORDER BY user_id")
            .bind(server.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn citizen_count(&self, server: ServerId) -> Result<u64, StoreError> {
        let row = sqlx::query(
            "SELECT count(*) AS n FROM memberships WHERE server_id = $1 AND tier = 'Citizen'",
        )
        .bind(server.0 as i64)
        .fetch_one(self.pool())
        .await
        .map_err(to_store_err)?;
        let n: i64 = row.try_get("n").map_err(to_store_err)?;
        Ok(n as u64)
    }

    async fn admitted_since(&self, server: ServerId, since: Timestamp) -> Result<u64, StoreError> {
        let row = sqlx::query(
            "SELECT count(*) AS n FROM memberships \
             WHERE server_id = $1 AND enfranchised_at IS NOT NULL AND enfranchised_at >= $2",
        )
        .bind(server.0 as i64)
        .bind(since.0)
        .fetch_one(self.pool())
        .await
        .map_err(to_store_err)?;
        let n: i64 = row.try_get("n").map_err(to_store_err)?;
        Ok(n as u64)
    }

    async fn admit_within_cap(
        &self,
        admitted: Membership,
        window_start: Timestamp,
        slots_open: &(dyn Fn(u64, u64) -> u64 + Send + Sync),
    ) -> Result<CapAdmission, StoreError> {
        let server = admitted.server_id;
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;

        // Serialize every admission on this server behind its row lock, so the
        // count-then-write below is atomic against a concurrent enfranchisement —
        // the two can no longer both observe the last open slot.
        sqlx::query("SELECT 1 FROM servers WHERE id = $1 FOR UPDATE")
            .bind(server.0 as i64)
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_store_err)?;

        let citizens: i64 = sqlx::query(
            "SELECT count(*) AS n FROM memberships WHERE server_id = $1 AND tier = 'Citizen'",
        )
        .bind(server.0 as i64)
        .fetch_one(&mut *tx)
        .await
        .map_err(to_store_err)?
        .try_get("n")
        .map_err(to_store_err)?;

        let admitted_window: i64 = sqlx::query(
            "SELECT count(*) AS n FROM memberships \
             WHERE server_id = $1 AND enfranchised_at IS NOT NULL AND enfranchised_at >= $2",
        )
        .bind(server.0 as i64)
        .bind(window_start.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(to_store_err)?
        .try_get("n")
        .map_err(to_store_err)?;

        let admitted_this_window = admitted_window as u64;
        if slots_open(citizens as u64, admitted_this_window) == 0 {
            // Nothing written; the lock releases on drop/rollback.
            tx.rollback().await.map_err(to_store_err)?;
            return Ok(CapAdmission::RateCapped { admitted_this_window });
        }

        sqlx::query(
            "INSERT INTO memberships (user_id, server_id, tier, enfranchised_at, data) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (user_id, server_id) DO UPDATE \
             SET tier = EXCLUDED.tier, enfranchised_at = EXCLUDED.enfranchised_at, data = EXCLUDED.data",
        )
        .bind(admitted.user_id.0 as i64)
        .bind(admitted.server_id.0 as i64)
        .bind(format!("{:?}", admitted.tier))
        .bind(admitted.enfranchised_at.map(|t| t.0))
        .bind(to_json(&admitted))
        .execute(&mut *tx)
        .await
        .map_err(to_store_err)?;
        push_outbox(&mut *tx, "memberships", ChangeOp::Upsert, &admitted).await?;

        tx.commit().await.map_err(to_store_err)?;
        Ok(CapAdmission::Admitted)
    }
}
