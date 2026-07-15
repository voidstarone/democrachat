use app::{RoleColorVoteStore, StoreError};
use async_trait::async_trait;
use domain::{RoleColor, RoleColorVote, RoleId, ServerId, UserId};
use federation::ChangeOp;

use crate::{decode, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl RoleColorVoteStore for PgStore {
    async fn upsert_role_color_vote(&self, vote: RoleColorVote) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query(
            "INSERT INTO role_color_votes (role_id, voter_id, server_id, data) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (role_id, voter_id) DO UPDATE \
             SET server_id = EXCLUDED.server_id, data = EXCLUDED.data",
        )
        .bind(vote.role_id.0 as i64)
        .bind(vote.voter.0 as i64)
        .bind(vote.server_id.0 as i64)
        .bind(to_json(&vote))
        .execute(&mut *tx)
        .await
        .map_err(to_store_err)?;
        push_outbox(&mut *tx, "role_color_votes", ChangeOp::Upsert, &vote).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn role_color_votes_for_server(&self, server: ServerId) -> Result<Vec<RoleColorVote>, StoreError> {
        let rows = sqlx::query(
            "SELECT data FROM role_color_votes WHERE server_id = $1 ORDER BY role_id, voter_id",
        )
        .bind(server.0 as i64)
        .fetch_all(self.pool())
        .await
        .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn my_role_color_vote(&self, role: RoleId, voter: UserId) -> Result<Option<RoleColor>, StoreError> {
        let row = sqlx::query("SELECT data FROM role_color_votes WHERE role_id = $1 AND voter_id = $2")
            .bind(role.0 as i64)
            .bind(voter.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        let vote: Option<RoleColorVote> = row.map(|r| decode(&r)).transpose()?;
        Ok(vote.map(|v| v.color))
    }
}
