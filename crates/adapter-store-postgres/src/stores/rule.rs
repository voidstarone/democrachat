use app::{RuleStore, StoreError};
use async_trait::async_trait;
use domain::{Rule, RuleId, ServerId};
use federation::ChangeOp;

use crate::{decode, next_seq, push_outbox, to_json, to_store_err, PgStore};

#[async_trait]
impl RuleStore for PgStore {
    async fn next_rule_id(&self) -> Result<RuleId, StoreError> {
        Ok(RuleId(next_seq(self.pool(), "rule_id_seq").await?))
    }

    async fn insert_rule(&self, rule: Rule) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        sqlx::query("INSERT INTO rules (id, server_id, data) VALUES ($1, $2, $3)")
            .bind(rule.id.0 as i64)
            .bind(rule.server_id.0 as i64)
            .bind(to_json(&rule))
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "rules", ChangeOp::Upsert, &rule).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(())
    }

    async fn remove_rule(&self, id: RuleId) -> Result<bool, StoreError> {
        let mut tx = self.pool().begin().await.map_err(to_store_err)?;
        let row = sqlx::query("SELECT data FROM rules WHERE id = $1 FOR UPDATE")
            .bind(id.0 as i64)
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_store_err)?;
        let Some(row) = row else {
            return Ok(false);
        };
        let rule: Rule = decode(&row)?;
        sqlx::query("DELETE FROM rules WHERE id = $1")
            .bind(id.0 as i64)
            .execute(&mut *tx)
            .await
            .map_err(to_store_err)?;
        push_outbox(&mut *tx, "rules", ChangeOp::Delete, &rule).await?;
        tx.commit().await.map_err(to_store_err)?;
        Ok(true)
    }

    async fn list_for_server(&self, server: ServerId) -> Result<Vec<Rule>, StoreError> {
        let rows = sqlx::query("SELECT data FROM rules WHERE server_id = $1 ORDER BY id")
            .bind(server.0 as i64)
            .fetch_all(self.pool())
            .await
            .map_err(to_store_err)?;
        rows.iter().map(decode).collect()
    }
}
