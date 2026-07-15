use app::{ProposalStore, StoreError};
use async_trait::async_trait;
use domain::{Proposal, ProposalId, ServerId};
use federation::ChangeOp;

use crate::{decode, next_seq, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl ProposalStore for PgStore {
    async fn next_proposal_id(&self) -> Result<ProposalId, StoreError> {
        Ok(ProposalId(next_seq(self.pool(), "proposal_id_seq").await?))
    }

    async fn insert_proposal(&self, proposal: Proposal) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query("INSERT INTO proposals (id, server_id, data) VALUES ($1, $2, $3)")
            .bind(proposal.id.0 as i64)
            .bind(proposal.server_id.0 as i64)
            .bind(to_json(&proposal))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "proposals", ChangeOp::Upsert, &proposal).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn get_proposal(&self, id: ProposalId) -> Result<Option<Proposal>, StoreError> {
        let row = sqlx::query("SELECT data FROM proposals WHERE id = $1")
            .bind(id.0 as i64)
            .fetch_optional(self.pool())
            .await
            .map_err(to_store_err)?;
        row.map(|r| decode(&r)).transpose()
    }

    async fn update_proposal(&self, proposal: Proposal) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query("UPDATE proposals SET server_id = $2, data = $3 WHERE id = $1")
            .bind(proposal.id.0 as i64)
            .bind(proposal.server_id.0 as i64)
            .bind(to_json(&proposal))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "proposals", ChangeOp::Upsert, &proposal).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Proposal>, StoreError> {
        let rows = sqlx::query("SELECT data FROM proposals WHERE server_id = $1 ORDER BY id")
            .bind(server.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
