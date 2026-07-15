use app::{RuleStore, StoreError};
use async_trait::async_trait;
use domain::{Rule, RuleId, ServerId};

use crate::{decode, next_seq, to_json, to_store_err, PgStore};

#[async_trait]
impl RuleStore for PgStore {
    async fn next_rule_id(&self) -> Result<RuleId, StoreError> {
        Ok(RuleId(next_seq(self.pool(), "rule_id_seq").await?))
    }

    async fn insert_rule(&self, rule: Rule) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO rules (id, server_id, data) VALUES ($1, $2, $3)")
            .bind(rule.id.0 as i64)
            .bind(rule.server_id.0 as i64)
            .bind(to_json(&rule))
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        Ok(())
    }

    async fn remove_rule(&self, id: RuleId) -> Result<bool, StoreError> {
        let done = sqlx::query("DELETE FROM rules WHERE id = $1")
            .bind(id.0 as i64)
            .execute(self.pool())
            .await
            .map_err(to_store_err)?;
        Ok(done.rows_affected() > 0)
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
