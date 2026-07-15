use app::{StoreError, VoteStore};
use async_trait::async_trait;
use domain::{ProposalId, UserId, Vote};
use federation::ChangeOp;

use crate::{decode, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl VoteStore for PgStore {
    async fn upsert_vote(&self, vote: Vote) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query(
            "INSERT INTO votes (proposal_id, voter_id, data) VALUES ($1, $2, $3) \
             ON CONFLICT (proposal_id, voter_id) DO UPDATE SET data = EXCLUDED.data",
        )
        .bind(vote.proposal_id.0 as i64)
        .bind(vote.voter.0 as i64)
        .bind(to_json(&vote))
        .execute(&mut *tx)
        .await
        .map_err(to_store_err)?;
        push_outbox(&mut *tx, "votes", ChangeOp::Upsert, &vote).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn get_vote(&self, proposal: ProposalId, voter: UserId) -> Result<Option<Vote>, StoreError> {
        let row = sqlx::query("SELECT data FROM votes WHERE proposal_id = $1 AND voter_id = $2")
            .bind(proposal.0 as i64)
            .bind(voter.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn list_for_proposal(&self, proposal: ProposalId) -> Result<Vec<Vote>, StoreError> {
        let rows = sqlx::query("SELECT data FROM votes WHERE proposal_id = $1 ORDER BY voter_id")
            .bind(proposal.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }

    async fn clear_for_proposal(&self, proposal: ProposalId) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        // Read the votes first so each removal can be published to the change feed.
        let rows = sqlx::query("SELECT data FROM votes WHERE proposal_id = $1 FOR UPDATE")
            .bind(proposal.0 as i64)
            .fetch_all(&mut *tx)
            .await
            .map_err(to_store_err)?;
        let votes: Vec<Vote> = rows.iter().map(decode).collect::<Result<_, _>>()?;
        sqlx::query("DELETE FROM votes WHERE proposal_id = $1")
            .bind(proposal.0 as i64)
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        for vote in &votes {
            push_outbox(&mut *tx, "votes", ChangeOp::Delete, vote).await?;
        }
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }
}
