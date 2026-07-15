use app::{StoreError, VoteStore};
use async_trait::async_trait;
use domain::{ProposalId, UserId, Vote};

use crate::{decode, to_json, to_store_err, PgStore};

#[async_trait]
impl VoteStore for PgStore {
    async fn upsert_vote(&self, vote: Vote) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO votes (proposal_id, voter_id, data) VALUES ($1, $2, $3) \
             ON CONFLICT (proposal_id, voter_id) DO UPDATE SET data = EXCLUDED.data",
        )
        .bind(vote.proposal_id.0 as i64)
        .bind(vote.voter.0 as i64)
        .bind(to_json(&vote))
        .execute(self.pool())
        .await
        .map_err(to_store_err)?;
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
}
